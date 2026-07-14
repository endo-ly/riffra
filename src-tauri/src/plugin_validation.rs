use crate::plugins::{ScanIssue, ScanReport};
use serde::Deserialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::AppHandle;
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandEvent, TerminatedPayload},
};

const SCANNER_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PARALLEL_SCANNERS: usize = 4;

#[derive(Debug)]
enum ValidationOutcome {
    Validated(PluginMetadata),
    Failed(String),
    Quarantined(String),
    Cancelled,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanEnvelope {
    #[serde(rename = "type")]
    message_type: String,
    plugins: Option<Vec<PluginMetadata>>,
    message: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginMetadata {
    name: String,
    vendor: Option<String>,
    version: Option<String>,
}

pub async fn validate_report(app: AppHandle, report: ScanReport) -> ScanReport {
    validate_report_with_cancel(app, report, None)
        .await
        .unwrap_or_else(|message| ScanReport {
            root: String::new(),
            started_at_ms: 0,
            finished_at_ms: 0,
            plugins: Vec::new(),
            issues: vec![ScanIssue {
                path: String::new(),
                message,
            }],
        })
}

pub async fn validate_report_with_cancel(
    app: AppHandle,
    mut report: ScanReport,
    cancelled: Option<Arc<AtomicBool>>,
) -> Result<ScanReport, String> {
    let candidates = report
        .plugins
        .iter()
        .map(|plugin| plugin.path.clone())
        .collect::<Vec<_>>();

    let mut results = Vec::with_capacity(candidates.len());
    for group in candidates.chunks(MAX_PARALLEL_SCANNERS) {
        let mut tasks = Vec::with_capacity(group.len());
        for path in group {
            if cancelled
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
            {
                return Err(
                    "VST3 validation cancelled; the previous catalog remains unchanged.".into(),
                );
            }
            let app = app.clone();
            let path = path.clone();
            let cancelled = cancelled.clone();
            tasks.push(tauri::async_runtime::spawn(async move {
                let outcome = validate_one(app, path.clone(), cancelled).await;
                (path, outcome)
            }));
        }
        for task in tasks {
            match task.await {
                Ok((path, ValidationOutcome::Cancelled)) => {
                    return Err(format!(
                        "VST3 validation cancelled while checking {path}; the previous catalog remains unchanged."
                    ));
                }
                Ok(result) => results.push(result),
                Err(error) => report.issues.push(ScanIssue {
                    path: report.root.clone(),
                    message: format!(
                        "A scanner supervisor task failed: {error}. The UI, audio engine, and session data are unaffected."
                    ),
                }),
            }
        }
    }

    for (path, outcome) in results {
        let Some(plugin) = report.plugins.iter_mut().find(|plugin| plugin.path == path) else {
            continue;
        };
        match outcome {
            ValidationOutcome::Validated(metadata) => {
                plugin.name = metadata.name;
                plugin.vendor = metadata.vendor.filter(|value| !value.trim().is_empty());
                plugin.version = metadata.version.filter(|value| !value.trim().is_empty());
                plugin.scan_state = "validated";
            }
            ValidationOutcome::Failed(message) => {
                plugin.scan_state = "failed";
                report.issues.push(ScanIssue { path, message });
            }
            ValidationOutcome::Quarantined(message) => {
                plugin.scan_state = "quarantined";
                report.issues.push(ScanIssue { path, message });
            }
            ValidationOutcome::Cancelled => {
                return Err(
                    "VST3 validation cancelled; the previous catalog remains unchanged.".into(),
                );
            }
        }
    }
    Ok(report)
}

async fn validate_one(
    app: AppHandle,
    path: String,
    cancelled: Option<Arc<AtomicBool>>,
) -> ValidationOutcome {
    let command = match app
        .shell()
        .sidecar("riffra-plugin-scan")
        .map(|command| command.args(["--scan", path.as_str()]))
    {
        Ok(command) => command,
        Err(error) => {
            return ValidationOutcome::Quarantined(format!(
                "Isolated scanner could not be prepared: {error}. The plugin was not loaded by Riffra."
            ));
        }
    };
    let (mut receiver, child) = match command.spawn() {
        Ok(pair) => pair,
        Err(error) => {
            return ValidationOutcome::Quarantined(format!(
                "Isolated scanner could not start: {error}. The plugin was not loaded by Riffra."
            ));
        }
    };

    let mut child = Some(child);
    let mut stdout = Vec::new();
    let mut stderr = String::new();
    let deadline = tokio::time::sleep(SCANNER_TIMEOUT);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) {
                    if let Some(child) = child.take() {
                        let _ = child.kill();
                    }
                    return ValidationOutcome::Cancelled;
                }
            }
            _ = &mut deadline => {
                if let Some(child) = child.take() {
                    let _ = child.kill();
                }
                return ValidationOutcome::Quarantined(format!(
                    "Plugin scan exceeded {} seconds and was terminated. The plugin is quarantined; session data is safe.",
                    SCANNER_TIMEOUT.as_secs()
                ));
            }
            event = receiver.recv() => {
                match event {
                    Some(CommandEvent::Stdout(mut bytes)) => {
                        stdout.append(&mut bytes);
                        stdout.push(b'\n');
                    }
                    Some(CommandEvent::Stderr(bytes)) => {
                        if stderr.len() < 1024 {
                            stderr.push_str(&String::from_utf8_lossy(&bytes));
                        }
                    }
                    Some(CommandEvent::Error(error)) => {
                        if let Some(child) = child.take() {
                            let _ = child.kill();
                        }
                        return ValidationOutcome::Quarantined(format!(
                            "Plugin scanner communication failed: {error}. The plugin was isolated and session data is safe."
                        ));
                    }
                    Some(CommandEvent::Terminated(payload)) => {
                        child.take();
                        return interpret_result(&stdout, &stderr, payload);
                    }
                    None => {
                        child.take();
                        return ValidationOutcome::Quarantined(
                            "Plugin scanner ended without a result. The plugin was isolated and session data is safe.".into()
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

fn interpret_result(
    stdout: &[u8],
    stderr: &str,
    terminated: TerminatedPayload,
) -> ValidationOutcome {
    let envelope = stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .find_map(|line| serde_json::from_slice::<ScanEnvelope>(line).ok());

    if let Some(envelope) = envelope {
        if envelope.message_type == "pluginScanResult"
            && terminated.code == Some(0)
            && let Some(plugin) = envelope.plugins.and_then(|mut plugins| {
                if plugins.is_empty() {
                    None
                } else {
                    Some(plugins.remove(0))
                }
            })
        {
            return ValidationOutcome::Validated(plugin);
        }
        if envelope.message_type == "pluginScanError" {
            return ValidationOutcome::Failed(envelope.message.unwrap_or_else(|| {
                "The isolated scanner found no usable VST3 component. Other plugins and session data are unaffected.".into()
            }));
        }
    }

    let detail = stderr.trim();
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!(
            " Diagnostic: {}",
            detail.chars().take(240).collect::<String>()
        )
    };
    ValidationOutcome::Quarantined(format!(
        "Plugin scanner exited unexpectedly with code {:?}. The candidate is quarantined; session data is safe.{detail}",
        terminated.code
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_successful_scanner_output() {
        let output = br#"{"type":"pluginScanResult","plugins":[{"name":"Amp","vendor":"Vendor","version":"1.2"}]}"#;
        let result = interpret_result(
            output,
            "",
            TerminatedPayload {
                code: Some(0),
                signal: None,
            },
        );
        match result {
            ValidationOutcome::Validated(plugin) => {
                assert_eq!(plugin.name, "Amp");
                assert_eq!(plugin.vendor.as_deref(), Some("Vendor"));
            }
            _ => panic!("expected validated plugin"),
        }
    }

    #[test]
    fn quarantines_unexpected_worker_exit() {
        let result = interpret_result(
            b"",
            "crashed",
            TerminatedPayload {
                code: Some(5),
                signal: None,
            },
        );
        assert!(matches!(result, ValidationOutcome::Quarantined(_)));
    }
}
