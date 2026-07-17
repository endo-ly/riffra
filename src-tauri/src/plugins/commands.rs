//! Thin Tauri command boundary for VST3 discovery and background scan jobs.
//!
//! [`scan_vst3_folder`] runs discovery + validation + catalog persistence
//! inline so the user-driven "refresh now" path returns the final report.
//! [`start_scan_job`] runs the same pipeline as a cancellable background job.

use std::path::{Path, PathBuf};

use tauri::State;

use crate::AppState;
use crate::jobs::{self, JobStatus};
use crate::plugins::{self, ScanReport};
use crate::{library, plugin_catalog, plugin_validation};

const DEFAULT_VST3_ROOT: &str = r"C:\Program Files\Common Files\VST3";

#[tauri::command]
pub async fn scan_vst3_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<ScanReport, String> {
    let root = PathBuf::from(path.unwrap_or_else(|| DEFAULT_VST3_ROOT.into()));
    if state.safe_mode {
        let now = crate::storage::now_ms();
        return Ok(ScanReport {
            root: root.to_string_lossy().into_owned(),
            started_at_ms: now,
            finished_at_ms: now,
            plugins: Vec::new(),
            issues: vec![plugins::ScanIssue {
                path: root.to_string_lossy().into_owned(),
                message: "Safe Mode skipped VST3 discovery; saved project data remains available."
                    .into(),
            }],
        });
    }
    let data_root = state.data_root.clone();
    let report = tauri::async_runtime::spawn_blocking(move || plugins::discover(&root))
        .await
        .map_err(|error| {
            format!("VST3 discovery task failed; no session data was changed: {error}")
        })?;
    let mut report = plugin_validation::validate_report(app, report).await;
    report.finished_at_ms = crate::storage::now_ms();
    tauri::async_runtime::spawn_blocking(move || {
        persist_scan_results(&data_root, &mut report);
        report
    })
    .await
    .map_err(|error| format!("Plugin catalog task failed: {error}"))
}

#[tauri::command]
pub async fn start_scan_job(
    app: tauri::AppHandle,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<JobStatus, String> {
    let (id, status) = state.jobs.start("scan");
    let registry = state.jobs.clone();
    let data_root = state.data_root.clone();
    let root = PathBuf::from(path.unwrap_or_else(|| DEFAULT_VST3_ROOT.into()));
    if state.safe_mode {
        let report = plugins::ScanReport {
            root: root.to_string_lossy().into_owned(),
            started_at_ms: crate::storage::now_ms(),
            finished_at_ms: crate::storage::now_ms(),
            plugins: Vec::new(),
            issues: vec![plugins::ScanIssue {
                path: root.to_string_lossy().into_owned(),
                message: "Safe Mode skipped VST3 discovery; saved project data remains available."
                    .into(),
            }],
        };
        registry.complete(
            &id,
            jobs::serialize_result(&report)?,
            "VST3 scan skipped in Safe Mode.",
        );
        return Ok(status);
    }
    tauri::async_runtime::spawn(async move {
        registry.set_running(
            &id,
            "Discovering and validating VST3 plugins in the background.",
        );
        let Some(cancelled) = registry.cancellation_flag(&id) else {
            return;
        };
        let discovered = tauri::async_runtime::spawn_blocking({
            let root = root.clone();
            let cancelled = cancelled.clone();
            move || plugins::discover_with_cancel(&root, Some(cancelled.as_ref()))
        })
        .await;
        let report = match discovered {
            Ok(Ok(report)) => report,
            Ok(Err(message)) => {
                jobs::fail(&registry, &data_root, &id, message);
                return;
            }
            Err(error) => {
                jobs::fail(
                    &registry,
                    &data_root,
                    &id,
                    format!("VST3 discovery task failed: {error}"),
                );
                return;
            }
        };
        let report = match plugin_validation::validate_report_with_cancel(
            app,
            report,
            Some(cancelled.clone()),
        )
        .await
        {
            Ok(mut report) => {
                report.finished_at_ms = crate::storage::now_ms();
                report
            }
            Err(message) => {
                jobs::fail(&registry, &data_root, &id, message);
                return;
            }
        };
        if registry.is_cancelled(&id) {
            registry.mark_cancelled(&id);
            return;
        }
        let data_root_for_persist = data_root.clone();
        let report_clone = report.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut report_mut = report_clone;
            persist_scan_results(&data_root_for_persist, &mut report_mut);
        })
        .await
        .ok();
        match jobs::serialize_result(&report) {
            Ok(value) => registry.complete(&id, value, "VST3 scan completed."),
            Err(message) => jobs::fail(&registry, &data_root, &id, message),
        }
    });
    Ok(status)
}

/// Persists the validated scan report to the on-disk catalog and the Library
/// Read Model. Failures are recorded to diagnostics but never surface as a
/// scan failure: the report itself remains usable for the session.
fn persist_scan_results(data_root: &Path, report: &mut ScanReport) {
    if let Err(error) = plugin_catalog::save(data_root, report) {
        let _ = crate::diagnostics::record(data_root, "scan", &error.to_string());
    }
    if let Err(error) = library::sync_plugins(data_root, &report.plugins) {
        let _ = crate::diagnostics::record(data_root, "scan", &error.to_string());
    }
}
