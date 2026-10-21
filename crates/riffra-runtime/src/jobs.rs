use crate::api::output::{
    AnalysisMeasureResult, BackgroundJobStatus, JobKind, JobState, RenderResult, RenderStemsResult,
    ScanReport,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

impl JobKind {
    fn label(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Render => "render",
            Self::RenderStems => "render-stems",
            Self::AnalysisMeasure => "analysis-measure",
        }
    }
}

/// Internal record held by [`JobRegistry`]. The result is kept as an opaque
/// JSON value because the registry is generic over job kinds; the typed
/// [`BackgroundJobStatus`] is produced when a status crosses the IPC boundary.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub id: String,
    pub kind: JobKind,
    pub state: JobState,
    pub progress: Option<f32>,
    pub message: String,
    pub result: Option<Value>,
}

/// Promotes an opaque [`JobStatus`] to a typed [`BackgroundJobStatus`]. The
/// result value is decoded into the type fixed by `kind`; a mismatched payload
/// is a worker encoding error and is surfaced as such.
///
/// # Errors
/// Returns a description when the stored result cannot be decoded into the type
/// the job kind demands.
pub fn to_background_status(status: JobStatus) -> Result<BackgroundJobStatus, String> {
    let JobStatus {
        id,
        kind,
        state,
        progress,
        message,
        result,
    } = status;
    Ok(match kind {
        JobKind::Scan => BackgroundJobStatus::Scan {
            id,
            state,
            progress,
            message,
            result: result
                .map(serde_json::from_value::<ScanReport>)
                .transpose()
                .map_err(|error| format!("scan result could not be decoded: {error}"))?,
        },
        JobKind::Render => BackgroundJobStatus::Render {
            id,
            state,
            progress,
            message,
            result: result
                .map(serde_json::from_value::<RenderResult>)
                .transpose()
                .map_err(|error| format!("render result could not be decoded: {error}"))?,
        },
        JobKind::RenderStems => BackgroundJobStatus::RenderStems {
            id,
            state,
            progress,
            message,
            result: result
                .map(serde_json::from_value::<RenderStemsResult>)
                .transpose()
                .map_err(|error| format!("render stems result could not be decoded: {error}"))?,
        },
        JobKind::AnalysisMeasure => BackgroundJobStatus::AnalysisMeasure {
            id,
            state,
            progress,
            message,
            result: result
                .map(serde_json::from_value::<AnalysisMeasureResult>)
                .transpose()
                .map_err(|error| {
                    format!("analysis measure result could not be decoded: {error}")
                })?,
        },
    })
}

struct JobRecord {
    status: Mutex<JobStatus>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Default)]
pub struct JobRegistry {
    records: Arc<Mutex<HashMap<String, Arc<JobRecord>>>>,
    workers: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    sequence: Arc<AtomicU64>,
}

impl JobRegistry {
    pub fn start(&self, kind: JobKind) -> (String, JobStatus) {
        let label = kind.label();
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let id = format!("job:{label}:{sequence}");
        let record = Arc::new(JobRecord {
            status: Mutex::new(JobStatus {
                id: id.clone(),
                kind,
                state: JobState::Queued,
                progress: None,
                message: format!("{label} job queued."),
                result: None,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        self.records
            .lock()
            .expect("job registry lock should not be poisoned")
            .insert(id.clone(), Arc::clone(&record));
        let status = record
            .status
            .lock()
            .expect("job status lock should not be poisoned")
            .clone();
        (id, status)
    }

    pub fn is_cancelled(&self, id: &str) -> bool {
        self.records
            .lock()
            .ok()
            .and_then(|records| records.get(id).cloned())
            .is_some_and(|record| record.cancelled.load(Ordering::Acquire))
    }

    pub fn cancellation_flag(&self, id: &str) -> Option<Arc<AtomicBool>> {
        self.records
            .lock()
            .ok()?
            .get(id)
            .map(|record| Arc::clone(&record.cancelled))
    }

    pub fn set_running(&self, id: &str, message: impl Into<String>) {
        self.update(id, |status, _| {
            if status.state != JobState::Queued {
                return;
            }
            status.state = JobState::Running;
            status.message = message.into();
        });
    }

    /// Updates running work without reviving a cancelled or terminal job.
    pub(crate) fn set_progress(&self, id: &str, progress: f32, message: impl Into<String>) {
        self.update(id, |status, _| {
            if status.state == JobState::Running {
                status.progress =
                    Some(status.progress.unwrap_or(0.0).max(progress.clamp(0.0, 1.0)));
                status.message = message.into();
            }
        });
    }

    pub fn complete(&self, id: &str, result: Value, message: impl Into<String>) {
        self.complete_if_not_cancelled(id, result, message, || {});
    }

    /// Completes the job and promotes its output while cancellation is excluded.
    ///
    /// # Returns
    /// `true` when the job completed and `promote` ran; `false` when cancellation
    /// or a terminal state prevented promotion.
    pub(crate) fn complete_if_not_cancelled(
        &self,
        id: &str,
        result: Value,
        message: impl Into<String>,
        promote: impl FnOnce(),
    ) -> bool {
        let Some(record) = self
            .records
            .lock()
            .ok()
            .and_then(|records| records.get(id).cloned())
        else {
            return false;
        };
        let Ok(mut status) = record.status.lock() else {
            return false;
        };
        if record.cancelled.load(Ordering::Acquire) || status.state == JobState::Cancelling {
            status.state = JobState::Cancelled;
            status.message = "Job cancelled; no partial result was promoted.".into();
            status.result = None;
            return false;
        }
        if matches!(
            status.state,
            JobState::Cancelled | JobState::Completed | JobState::Failed
        ) {
            return false;
        }
        promote();
        status.state = JobState::Completed;
        status.progress = Some(1.0);
        status.message = message.into();
        status.result = Some(result);
        true
    }

    pub fn fail(&self, id: &str, message: impl Into<String>) {
        self.update(id, |status, record| {
            if record.cancelled.load(Ordering::Acquire) {
                status.state = JobState::Cancelled;
                status.message = "Job cancelled; no partial result was promoted.".into();
            } else {
                status.state = JobState::Failed;
                status.message = message.into();
            }
        });
    }

    pub fn status(&self, id: &str) -> Option<JobStatus> {
        let record = self.records.lock().ok()?.get(id).cloned()?;
        record.status.lock().ok().map(|status| status.clone())
    }

    pub(crate) fn active_render_job_ids(&self) -> Result<Vec<String>, String> {
        let records = self
            .records
            .lock()
            .map_err(|error| format!("render jobs could not be inspected: {error}"))?;
        let mut active = Vec::new();
        for (id, record) in records.iter() {
            let status = record
                .status
                .lock()
                .map_err(|error| format!("render job {id} could not be inspected: {error}"))?;
            if matches!(status.kind, JobKind::Render | JobKind::RenderStems)
                && matches!(
                    status.state,
                    JobState::Queued | JobState::Running | JobState::Cancelling
                )
            {
                active.push(id.clone());
            }
        }
        Ok(active)
    }

    pub fn cancel(&self, id: &str) -> Option<JobStatus> {
        let record = self.records.lock().ok()?.get(id).cloned()?;
        if let Ok(mut status) = record.status.lock() {
            if matches!(
                status.state,
                JobState::Queued | JobState::Running | JobState::Cancelling
            ) {
                record.cancelled.store(true, Ordering::Release);
                status.state = JobState::Cancelling;
                status.message =
                    "Cancellation requested; the worker is finishing its current block.".into();
            }
            return Some(status.clone());
        }
        None
    }

    /// Requests cancellation for every non-terminal job owned by the Host.
    pub fn cancel_all(&self) {
        let records = self
            .records
            .lock()
            .map(|records| records.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for record in records {
            if let Ok(mut status) = record.status.lock()
                && matches!(
                    status.state,
                    JobState::Queued | JobState::Running | JobState::Cancelling
                )
            {
                record.cancelled.store(true, Ordering::Release);
                status.state = JobState::Cancelling;
                status.message = "Cancellation requested because the Host is shutting down.".into();
            }
        }
    }

    pub(crate) fn spawn_worker<F>(&self, id: &str, name: &str, worker: F) -> Result<(), String>
    where
        F: FnOnce() + Send + 'static,
    {
        let (start_sender, start_receiver) = std::sync::mpsc::sync_channel(0);
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                if start_receiver.recv().is_ok() {
                    worker();
                }
            })
            .map_err(|error| format!("job worker could not start: {error}"))?;
        self.workers
            .lock()
            .map_err(|_| "job worker registry was poisoned".to_owned())?
            .insert(id.to_owned(), handle);
        start_sender
            .send(())
            .map_err(|_| "job worker could not be released".to_owned())
    }

    pub(crate) fn cancel_all_and_wait(&self) {
        self.cancel_all();
        let workers: Vec<JoinHandle<()>> = self
            .workers
            .lock()
            .map(|mut workers| workers.drain().map(|(_, worker)| worker).collect())
            .unwrap_or_default();
        for worker in workers {
            if worker.join().is_err() {
                tracing::warn!("background job worker panicked during Host shutdown");
            }
        }
    }

    pub fn mark_cancelled(&self, id: &str) {
        self.update(id, |status, _| {
            status.state = JobState::Cancelled;
            status.message = "Job cancelled; no partial result was promoted.".into();
            status.result = None;
        });
    }

    fn update(&self, id: &str, update: impl FnOnce(&mut JobStatus, &JobRecord)) {
        let Some(record) = self
            .records
            .lock()
            .ok()
            .and_then(|records| records.get(id).cloned())
        else {
            return;
        };
        if let Ok(mut status) = record.status.lock()
            && !matches!(
                status.state,
                JobState::Completed | JobState::Failed | JobState::Cancelled
            )
        {
            update(&mut status, &record);
        }
    }
}

/// Shared by background-job features: marks the job failed (or cancelled, if
/// the user asked to cancel before the result landed) and records the failure
/// to the diagnostics log so it is not silently swallowed.
pub fn fail(registry: &JobRegistry, data_root: &Path, id: &str, message: String) {
    if registry.is_cancelled(id) {
        registry.mark_cancelled(id);
    } else {
        tracing::warn!(data_root = %data_root.display(), error = %message, "background job failed");
        registry.fail(id, message);
    }
}

/// Shared by background-job features: encodes a strongly typed job result into
/// the [`Value`] shape the registry stores, surfacing encoding failures
/// through the same `fail` path as worker errors.
pub fn serialize_result<T: Serialize>(result: &T) -> Result<Value, String> {
    serde_json::to_value(result)
        .map_err(|error| format!("Job result could not be encoded: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_prevents_completion_from_promoting_a_result() {
        let registry = JobRegistry::default();
        let (id, status) = registry.start(JobKind::Scan);
        assert_eq!(status.state, JobState::Queued);
        registry.set_running(&id, "running");
        registry.set_progress(&id, 0.5, "half done");
        registry.set_progress(&id, 0.25, "still measuring");
        assert_eq!(registry.status(&id).unwrap().progress, Some(0.5));
        registry.cancel(&id).expect("job should exist");
        registry.set_running(&id, "late running");
        registry.set_progress(&id, 0.75, "late progress");
        assert_eq!(registry.status(&id).unwrap().state, JobState::Cancelling);
        assert_eq!(registry.status(&id).unwrap().progress, Some(0.5));
        let promoted = Arc::new(AtomicBool::new(false));
        let promotion = Arc::clone(&promoted);
        assert!(!registry.complete_if_not_cancelled(
            &id,
            serde_json::json!({"path":"output.wav"}),
            "done",
            move || promotion.store(true, Ordering::Release),
        ));
        let status = registry.status(&id).unwrap();
        assert_eq!(status.state, JobState::Cancelled);
        assert!(status.result.is_none());
        assert!(!promoted.load(Ordering::Acquire));
        registry.set_running(&id, "late running");
        registry.set_progress(&id, 1.0, "late progress");
        registry.fail(&id, "late failure");
        assert_eq!(registry.status(&id).unwrap().state, JobState::Cancelled);
    }

    #[test]
    fn an_uncancelled_stem_job_promotes_its_result_before_completion() {
        let registry = JobRegistry::default();
        let (id, _) = registry.start(JobKind::RenderStems);
        registry.set_running(&id, "rendering");
        let promoted = Arc::new(AtomicBool::new(false));
        let promotion = Arc::clone(&promoted);
        let result = RenderStemsResult {
            directory: "renders/render-1".into(),
            exported_directory: None,
            project_id: "project-1".into(),
            created_at_ms: None,
            source_sequence: 4,
            sample_rate: 48_000,
            start_tick: 0,
            end_tick: 480,
            frames: 1_024,
            stems: Vec::new(),
            skipped_track_ids: Vec::new(),
        };

        assert!(registry.complete_if_not_cancelled(
            &id,
            serialize_result(&result).unwrap(),
            "done",
            move || promotion.store(true, Ordering::Release),
        ));

        let BackgroundJobStatus::RenderStems {
            state,
            result: Some(actual),
            ..
        } = to_background_status(registry.status(&id).unwrap()).unwrap()
        else {
            panic!("render stems jobs preserve their typed result");
        };
        assert_eq!(state, JobState::Completed);
        assert_eq!(actual.project_id, result.project_id);
        assert_eq!(actual.sample_rate, result.sample_rate);
        assert_eq!(actual.frames, result.frames);
        assert!(promoted.load(Ordering::Acquire));
        registry.set_running(&id, "late running");
        registry.set_progress(&id, 0.5, "late progress");
        registry.fail(&id, "late failure");
        assert_eq!(registry.status(&id).unwrap().state, JobState::Completed);
    }

    #[test]
    fn shutdown_waits_for_registered_workers() {
        let registry = JobRegistry::default();
        let (id, _) = registry.start(JobKind::Render);
        let finished = Arc::new(AtomicBool::new(false));
        let worker_finished = Arc::clone(&finished);
        registry
            .spawn_worker(&id, "test-job-worker", move || {
                thread::sleep(std::time::Duration::from_millis(10));
                worker_finished.store(true, Ordering::Release);
            })
            .unwrap();

        registry.cancel_all_and_wait();

        assert!(finished.load(Ordering::Acquire));
    }
}
