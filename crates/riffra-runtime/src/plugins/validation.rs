use crate::api::output::{PluginRole, PluginScanState, ScanIssue, ScanReport};
use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const SCANNER_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
enum ValidationOutcome {
    Validated(PluginMetadata),
    Failed(String),
    Quarantined(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanEnvelope {
    #[serde(rename = "type")]
    message_type: String,
    plugins: Option<Vec<PluginMetadata>>,
    message: Option<String>,
    load_tested: Option<bool>,
    load_test_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginMetadata {
    name: String,
    vendor: Option<String>,
    version: Option<String>,
    is_instrument: bool,
}

/// Validates every discovered plugin through the isolated scanner process.
pub fn validate_report(report: ScanReport, scanner: &Path) -> Result<ScanReport, String> {
    validate_report_with_cancel(report, scanner, None)
}

/// Validates discovered plugins while honoring a caller-owned cancellation flag.
pub fn validate_report_with_cancel(
    mut report: ScanReport,
    scanner: &Path,
    cancelled: Option<Arc<AtomicBool>>,
) -> Result<ScanReport, String> {
    let candidates = report
        .plugins
        .iter()
        .filter(|plugin| plugin.scan_state == PluginScanState::Discovered)
        .map(|plugin| plugin.path.clone())
        .collect::<Vec<_>>();

    for path in candidates {
        if cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            return Err(
                "VST3 validation cancelled; the previous catalog remains unchanged.".into(),
            );
        }
        let outcome = validate_one(scanner, &path, cancelled.as_deref())?;
        let Some(plugin) = report.plugins.iter_mut().find(|plugin| plugin.path == path) else {
            continue;
        };
        match outcome {
            ValidationOutcome::Validated(metadata) => {
                plugin.name = metadata.name;
                plugin.vendor = metadata.vendor.filter(|value| !value.trim().is_empty());
                plugin.version = metadata.version.filter(|value| !value.trim().is_empty());
                plugin.role = Some(if metadata.is_instrument {
                    PluginRole::Instrument
                } else {
                    PluginRole::Effect
                });
                plugin.scan_state = PluginScanState::Validated;
            }
            ValidationOutcome::Failed(message) => {
                plugin.scan_state = PluginScanState::Failed;
                report.issues.push(ScanIssue { path, message });
            }
            ValidationOutcome::Quarantined(message) => {
                plugin.scan_state = PluginScanState::Quarantined;
                report.issues.push(ScanIssue { path, message });
            }
        }
    }
    Ok(report)
}

fn validate_one(
    scanner: &Path,
    path: &str,
    cancelled: Option<&AtomicBool>,
) -> Result<ValidationOutcome, String> {
    let mut child = Command::new(scanner)
        .args(["--scan", path])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("isolated scanner could not start: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "isolated scanner stdout is unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "isolated scanner stderr is unavailable".to_string())?;
    let stdout_reader = thread::spawn(move || read_to_end(stdout));
    let stderr_reader = thread::spawn(move || read_to_end(stderr));
    let deadline = Instant::now() + SCANNER_TIMEOUT;
    let status = loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "VST3 validation cancelled; the previous catalog remains unchanged.".into(),
            );
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(ValidationOutcome::Quarantined(format!(
                "Plugin scan exceeded {} seconds and was terminated. The plugin is quarantined; session data is safe.",
                SCANNER_TIMEOUT.as_secs()
            )));
        }
        match child
            .try_wait()
            .map_err(|error| format!("isolated scanner status could not be read: {error}"))?
        {
            Some(status) => break status,
            None => thread::sleep(Duration::from_millis(25)),
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "isolated scanner stdout reader panicked".to_string())??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "isolated scanner stderr reader panicked".to_string())??;
    Ok(interpret_result(&stdout, &stderr, status.success()))
}

fn read_to_end<R: Read>(mut reader: R) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| format!("scanner output could not be read: {error}"))?;
    Ok(bytes)
}

fn interpret_result(stdout: &[u8], stderr: &[u8], succeeded: bool) -> ValidationOutcome {
    let envelope = match serde_json::from_slice::<ScanEnvelope>(stdout) {
        Ok(envelope) => envelope,
        Err(error) => {
            return ValidationOutcome::Quarantined(scanner_failure(
                &format!("the scanner did not return one JSON envelope ({error})"),
                stdout,
                stderr,
            ));
        }
    };
    if envelope.message_type == "pluginScanResult"
        && succeeded
        && let Some(plugin) = envelope
            .plugins
            .and_then(|plugins| plugins.into_iter().next())
    {
        if envelope.load_tested == Some(false) {
            return ValidationOutcome::Quarantined(format!(
                "VST3 load validation failed: {} The plugin is quarantined to prevent the audio engine from freezing.",
                envelope
                    .load_test_message
                    .unwrap_or_else(|| "the plugin could not be safely instantiated.".into())
            ));
        }
        return ValidationOutcome::Validated(plugin);
    }
    if envelope.message_type == "pluginScanError" {
        return ValidationOutcome::Failed(envelope.message.unwrap_or_else(|| {
            "The isolated scanner found no usable VST3 component. Other plugins and session data are unaffected.".into()
        }));
    }
    ValidationOutcome::Quarantined(scanner_failure(
        "the scanner did not return a usable scan result",
        stdout,
        stderr,
    ))
}

/// Builds the quarantine message for a scanner that returned no usable scan
/// result, with bounded views of both isolated streams.
fn scanner_failure(reason: &str, stdout: &[u8], stderr: &[u8]) -> String {
    let mut message = format!(
        "Plugin scanner exited unexpectedly. The candidate is quarantined; session data is safe. Diagnostic: {reason}"
    );
    for (label, bytes) in [("stdout", stdout), ("stderr", stderr)] {
        let excerpt = excerpt(bytes);
        if !excerpt.is_empty() {
            message.push_str(&format!(". {label}: {excerpt}"));
        }
    }
    message
}

/// Bounded single-line view of the tail of a scanner byte stream for error
/// messages.
fn excerpt(bytes: &[u8]) -> String {
    const LIMIT: usize = 240;
    let flattened = String::from_utf8_lossy(bytes)
        .chars()
        .map(|ch| match ch {
            '\r' | '\n' => ' ',
            other => other,
        })
        .collect::<String>();
    let trimmed = flattened.trim();
    let start = trimmed
        .char_indices()
        .rev()
        .nth(LIMIT - 1)
        .map_or(0, |(index, _)| index);
    trimmed[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_successful_scanner_output() {
        let output = br#"{"type":"pluginScanResult","plugins":[{"name":"Amp","vendor":"Vendor","version":"1.2","isInstrument":true}],"loadTested":true}"#;
        assert!(matches!(
            interpret_result(output, b"", true),
            ValidationOutcome::Validated(_)
        ));
    }

    #[test]
    fn quarantines_a_plugin_that_cannot_be_loaded() {
        let output =
            br#"{"type":"pluginScanResult","plugins":[{"name":"Heavy","isInstrument":false}],"loadTested":false}"#;
        assert!(matches!(
            interpret_result(output, b"", true),
            ValidationOutcome::Quarantined(_)
        ));
    }

    #[test]
    fn quarantines_scanner_output_contaminated_by_plugin_stdout() {
        // Arrange
        let output = b"riffra-test-plugin-stdout-crt\n{\"type\":\"pluginScanResult\",\"plugins\":[{\"name\":\"Amp\",\"isInstrument\":false}],\"loadTested\":true}\n";

        // Act
        let ValidationOutcome::Quarantined(message) = interpret_result(output, b"", true) else {
            panic!("contaminated scanner output must be quarantined");
        };

        // Assert
        assert!(message.contains("riffra-test-plugin-stdout-crt"));
    }
}
