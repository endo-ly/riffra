use super::*;

impl HostState {
    pub(super) fn scan_plugins(&self, root: PathBuf) -> Result<plugins::ScanReport, String> {
        if self.core.safe_mode() {
            return Err("Safe Mode blocks VST3 discovery and load validation".into());
        }
        let mut report = plugins::discover(&root);
        plugins::reuse_cached_scan_results(&self.data_root, &mut report);
        let mut report = plugins::validate_report(report, &self.binaries.plugin_scan)?;
        report.finished_at_ms = now_ms();
        plugins::save(&self.data_root, &report)
            .map_err(|error| format!("plugin catalog could not be saved: {error}"))?;
        library::sync_plugins(&self.data_root, &report.plugins)?;
        Ok(report)
    }

    pub(super) fn start_plugin_scan(&self, root: PathBuf) -> Result<BackgroundJobStatus, String> {
        if self.core.safe_mode() {
            return Err("Safe Mode blocks VST3 discovery and load validation".into());
        }
        let (id, status) = self.jobs.start(JobKind::Scan);
        let registry = self.jobs.clone();
        let data_root = self.data_root.clone();
        let scanner = self.binaries.plugin_scan.clone();
        let Some(cancelled) = registry.cancellation_flag(&id) else {
            return Err("plugin scan job could not be registered".into());
        };
        let job_id = id.clone();
        self.jobs
            .spawn_worker(&id, "riffra-plugin-scan-job", move || {
                registry.set_running(
                    &job_id,
                    "Discovering and validating VST3 plugins in the background.",
                );
                let mut report =
                    match plugins::discover_with_cancel(&root, Some(cancelled.as_ref())) {
                        Ok(report) => report,
                        Err(error) => {
                            jobs::fail(&registry, &data_root, &job_id, error);
                            return;
                        }
                    };
                plugins::reuse_cached_scan_results(&data_root, &mut report);
                let report = match plugins::validate_report_with_cancel(
                    report,
                    &scanner,
                    Some(cancelled.clone()),
                ) {
                    Ok(mut report) => {
                        report.finished_at_ms = now_ms();
                        report
                    }
                    Err(error) => {
                        jobs::fail(&registry, &data_root, &job_id, error);
                        return;
                    }
                };
                if registry.is_cancelled(&job_id) {
                    registry.mark_cancelled(&job_id);
                    return;
                }
                if let Err(error) = plugins::save(&data_root, &report) {
                    jobs::fail(
                        &registry,
                        &data_root,
                        &job_id,
                        format!("plugin catalog could not be saved: {error}"),
                    );
                    return;
                }
                if let Err(error) = library::sync_plugins(&data_root, &report.plugins) {
                    jobs::fail(&registry, &data_root, &job_id, error);
                    return;
                }
                match jobs::serialize_result(&report) {
                    Ok(value) => registry.complete(&job_id, value, "VST3 scan completed."),
                    Err(error) => jobs::fail(&registry, &data_root, &job_id, error),
                }
            })
            .map_err(|error| format!("plugin scan job could not start: {error}"))?;
        jobs::to_background_status(status)
    }
}
