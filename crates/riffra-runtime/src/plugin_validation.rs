use crate::plugins::{PluginScanState, ScanIssue, ScanReport};
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
    let envelope = stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .find_map(|line| serde_json::from_slice::<ScanEnvelope>(line).ok());
    if let Some(envelope) = envelope {
        if envelope.message_type == "pluginScanResult"
            && succeeded
            && let Some(plugin) = envelope.plugins.and_then(|mut plugins| plugins.pop())
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
    }
    let detail = String::from_utf8_lossy(stderr).trim().to_owned();
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!(
            " Diagnostic: {}",
            detail.chars().take(240).collect::<String>()
        )
    };
    ValidationOutcome::Quarantined(format!(
        "Plugin scanner exited unexpectedly. The candidate is quarantined; session data is safe.{detail}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_successful_scanner_output() {
        let output = br#"{"type":"pluginScanResult","plugins":[{"name":"Amp","vendor":"Vendor","version":"1.2"}],"loadTested":true}"#;
        assert!(matches!(
            interpret_result(output, b"", true),
            ValidationOutcome::Validated(_)
        ));
    }

    #[test]
    fn quarantines_a_plugin_that_cannot_be_loaded() {
        let output =
            br#"{"type":"pluginScanResult","plugins":[{"name":"Heavy"}],"loadTested":false}"#;
        assert!(matches!(
            interpret_result(output, b"", true),
            ValidationOutcome::Quarantined(_)
        ));
    }
}
