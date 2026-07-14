use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub progress: f32,
    pub message: String,
    pub result: Option<Value>,
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
    pub fn start(&self, kind: &str) -> (String, JobStatus) {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let id = format!("job:{kind}:{sequence}");
        let record = Arc::new(JobRecord {
            status: Mutex::new(JobStatus {
                id: id.clone(),
                kind: kind.into(),
                state: "queued".into(),
                progress: 0.0,
                message: format!("{kind} job queued."),
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
            status.state = "running".into();
            status.message = message.into();
        });
    }

    pub fn complete(&self, id: &str, result: Value, message: impl Into<String>) {
        self.update(id, |status, record| {
            if record.cancelled.load(Ordering::Acquire) {
                status.state = "cancelled".into();
                status.message = "Job cancelled; no partial result was promoted.".into();
                status.result = None;
            } else {
                status.state = "completed".into();
                status.progress = 1.0;
                status.message = message.into();
                status.result = Some(result);
            }
        });
    }

    pub fn fail(&self, id: &str, message: impl Into<String>) {
        self.update(id, |status, record| {
            if record.cancelled.load(Ordering::Acquire) {
                status.state = "cancelled".into();
                status.message = "Job cancelled; no partial result was promoted.".into();
            } else {
                status.state = "failed".into();
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
            if matches!(status.state.as_str(), "queued" | "running" | "cancelling") {
                record.cancelled.store(true, Ordering::Release);
                status.state = "cancelling".into();
                status.message =
                    "Cancellation requested; the worker is finishing its current block.".into();
            }
            return Some(status.clone());
        }
        None
    }

    pub fn mark_cancelled(&self, id: &str) {
        self.update(id, |status, _| {
            status.state = "cancelled".into();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_prevents_completion_from_promoting_a_result() {
        let registry = JobRegistry::default();
        let (id, status) = registry.start("render");
        assert_eq!(status.state, "queued");
        registry.set_running(&id, "running");
        registry.cancel(&id).expect("job should exist");
        registry.complete(&id, serde_json::json!({"path":"output.wav"}), "done");
        let status = registry.status(&id).unwrap();
        assert_eq!(status.state, "cancelled");
        assert!(status.result.is_none());
    }
}
