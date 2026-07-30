use crate::analysis::AudioAnalysis;
use crate::plugins::ScanReport;
use crate::separation::SeparationResult;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use ts_rs::TS;

/// Lifecycle state of a background job. Terminal states (`Cancelled`,
/// `Completed`, `Failed`) cannot return to `Running`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

/// Background job kind. Acts as the `kind` discriminator of
/// [`BackgroundJobStatus`] and fixes the type of the result payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Analysis,
    Separation,
    Scan,
}

impl JobKind {
    fn label(self) -> &'static str {
        match self {
            Self::Analysis => "analysis",
            Self::Separation => "separation",
            Self::Scan => "scan",
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
    pub progress: f32,
    pub message: String,
    pub result: Option<Value>,
}

/// Typed view of a background job, produced from [`JobStatus`] at the IPC
/// boundary. `kind` is the discriminator and fixes the shape of `result`.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackgroundJobStatus {
    Analysis {
        id: String,
        state: JobState,
        progress: f32,
        message: String,
        result: Option<AudioAnalysis>,
    },
    Separation {
        id: String,
        state: JobState,
        progress: f32,
        message: String,
        result: Option<SeparationResult>,
    },
    Scan {
        id: String,
        state: JobState,
        progress: f32,
        message: String,
        result: Option<ScanReport>,
    },
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
        JobKind::Analysis => BackgroundJobStatus::Analysis {
            id,
            state,
            progress,
            message,
            result: result
                .map(serde_json::from_value::<AudioAnalysis>)
                .transpose()
                .map_err(|error| format!("analysis result could not be decoded: {error}"))?,
        },
        JobKind::Separation => BackgroundJobStatus::Separation {
            id,
            state,
            progress,
            message,
            result: result
                .map(serde_json::from_value::<SeparationResult>)
                .transpose()
                .map_err(|error| format!("separation result could not be decoded: {error}"))?,
        },
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
    })
}

struct JobRecord {
    status: Mutex<JobStatus>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Default)]
pub struct JobRegistry {
    records: Arc<Mutex<HashMap<String, Arc<JobRecord>>>>,
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
                progress: 0.0,
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
            status.state = JobState::Running;
            status.message = message.into();
        });
    }

    pub fn complete(&self, id: &str, result: Value, message: impl Into<String>) {
        self.update(id, |status, record| {
            if record.cancelled.load(Ordering::Acquire) {
                status.state = JobState::Cancelled;
                status.message = "Job cancelled; no partial result was promoted.".into();
                status.result = None;
            } else {
                status.state = JobState::Completed;
                status.progress = 1.0;
                status.message = message.into();
                status.result = Some(result);
            }
        });
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
        if let Ok(mut status) = record.status.lock() {
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
        let _ = crate::diagnostics::record(data_root, "job", &message);
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
        let (id, status) = registry.start(JobKind::Analysis);
        assert_eq!(status.state, JobState::Queued);
        registry.set_running(&id, "running");
        registry.cancel(&id).expect("job should exist");
        registry.complete(&id, serde_json::json!({"path":"output.wav"}), "done");
        let status = registry.status(&id).unwrap();
        assert_eq!(status.state, JobState::Cancelled);
        assert!(status.result.is_none());
    }
}
