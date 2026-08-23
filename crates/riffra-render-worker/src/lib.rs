//! Process adapter for device-independent Riffra offline rendering.

use riffra_core::{OfflineRenderRequest, RenderRuntime};
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use thiserror::Error;

/// Failure reported while invoking the offline render worker.
#[derive(Debug, Error)]
pub enum RenderWorkerError {
    #[error("current executable could not be resolved: {0}")]
    CurrentExecutable(#[source] std::io::Error),
    #[error("render worker executable path has no parent directory")]
    MissingExecutableDirectory,
    #[error("render worker could not start: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("render request could not be encoded: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("render request could not be sent: {0}")]
    Write(#[source] std::io::Error),
    #[error("render worker could not be awaited: {0}")]
    Wait(#[source] std::io::Error),
    #[error("render worker returned an invalid response: {0}")]
    InvalidResponse(#[source] serde_json::Error),
    #[error("render worker response did not contain a type")]
    MissingResponseType,
    #[error("render worker failed: {0}")]
    Rejected(String),
    #[error("render worker exited without completing the render")]
    Incomplete,
    #[error("render worker was cancelled")]
    Cancelled,
}

/// Launches one `riffra-render` process for each offline render request.
#[derive(Clone)]
pub struct RenderWorker {
    executable: PathBuf,
}

impl RenderWorker {
    /// Creates an adapter for an explicit worker executable.
    pub fn new(executable: PathBuf) -> Self {
        Self { executable }
    }

    /// Resolves a bundled worker located beside the current executable.
    ///
    /// # Errors
    ///
    /// Returns an error when the current executable has no parent directory.
    pub fn bundled() -> Result<Self, RenderWorkerError> {
        let current = std::env::current_exe().map_err(RenderWorkerError::CurrentExecutable)?;
        let directory = current
            .parent()
            .ok_or(RenderWorkerError::MissingExecutableDirectory)?;
        Ok(Self::new(directory.join(format!(
            "riffra-render{}",
            std::env::consts::EXE_SUFFIX
        ))))
    }

    /// Returns the configured worker executable.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    fn render(&self, request: OfflineRenderRequest) -> Result<(), RenderWorkerError> {
        self.render_with_cancellation(request, None)
    }

    fn render_with_cancellation(
        &self,
        request: OfflineRenderRequest,
        cancelled: Option<&AtomicBool>,
    ) -> Result<(), RenderWorkerError> {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(RenderWorkerError::Cancelled);
        }
        let payload = serde_json::json!({
            "type": "renderTimelineOffline",
            "protocolVersion": 1,
            "snapshot": request.snapshot,
            "destination": request.destination,
            "startTick": request.start_tick,
            "endTick": request.end_tick,
            "sampleRate": request.sample_rate,
            "blockSize": request.block_size,
            "masterGainDb": request.master_gain_db,
            "normalize": request.normalize,
        });
        let encoded = serde_json::to_vec(&payload).map_err(RenderWorkerError::Encode)?;
        tracing::info!("starting offline render worker");
        let mut child = Command::new(&self.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(RenderWorkerError::Spawn)?;
        let mut input = child.stdin.take().ok_or_else(|| {
            RenderWorkerError::Write(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "render worker standard input is unavailable",
            ))
        })?;
        input
            .write_all(&encoded)
            .and_then(|()| input.write_all(b"\n"))
            .map_err(RenderWorkerError::Write)?;
        drop(input);
        if cancelled.is_none() {
            return self.handle_output(child.wait_with_output().map_err(RenderWorkerError::Wait)?);
        }
        let mut stdout = child.stdout.take().ok_or_else(|| {
            RenderWorkerError::Wait(std::io::Error::other(
                "render worker standard output is unavailable",
            ))
        })?;
        let mut stderr = child.stderr.take().ok_or_else(|| {
            RenderWorkerError::Wait(std::io::Error::other(
                "render worker standard error is unavailable",
            ))
        })?;
        let stdout_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        });
        let stderr_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        });
        loop {
            if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(RenderWorkerError::Cancelled);
            }
            match child.try_wait().map_err(RenderWorkerError::Wait)? {
                Some(_) => break,
                None => thread::sleep(Duration::from_millis(25)),
            }
        }
        let status = child.wait().map_err(RenderWorkerError::Wait)?;
        let stdout = stdout_reader
            .join()
            .map_err(|_| RenderWorkerError::Wait(std::io::Error::other("stdout reader panicked")))?
            .map_err(RenderWorkerError::Wait)?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| RenderWorkerError::Wait(std::io::Error::other("stderr reader panicked")))?
            .map_err(RenderWorkerError::Wait)?;
        self.handle_output(Output {
            status,
            stdout,
            stderr,
        })
    }

    fn handle_output(&self, output: Output) -> Result<(), RenderWorkerError> {
        let response: Value =
            serde_json::from_slice(&output.stdout).map_err(RenderWorkerError::InvalidResponse)?;
        match response.get("type").and_then(Value::as_str) {
            Some("offlineRenderComplete") if output.status.success() => {
                tracing::info!("offline render worker completed");
                Ok(())
            }
            Some("error") => Err(RenderWorkerError::Rejected(
                response
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown render worker error")
                    .to_owned(),
            )),
            Some(_) => Err(RenderWorkerError::Incomplete),
            None => Err(RenderWorkerError::MissingResponseType),
        }
    }

    /// Runs one offline render while observing a cancellation flag.
    ///
    /// # Errors
    /// Returns a host-provided description when the render cannot be completed
    /// or the flag requests cancellation.
    pub fn render_timeline_offline_cancellable(
        &self,
        request: OfflineRenderRequest,
        cancelled: &AtomicBool,
    ) -> Result<(), String> {
        self.render_with_cancellation(request, Some(cancelled))
            .map_err(|error| error.to_string())
    }
}

impl RenderRuntime for RenderWorker {
    fn render_timeline_offline(&self, request: OfflineRenderRequest) -> Result<(), String> {
        self.render(request).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn explicit_worker_path_is_preserved() {
        // Arrange
        let path =
            PathBuf::from("workers").join(format!("riffra-render{}", std::env::consts::EXE_SUFFIX));

        // Act
        let worker = RenderWorker::new(path.clone());

        // Assert
        assert_eq!(worker.executable(), path);
    }

    #[test]
    fn cancellation_is_observed_before_worker_spawn() {
        let worker = RenderWorker::new(PathBuf::from("missing-render-worker"));
        let cancelled = AtomicBool::new(true);
        let request = OfflineRenderRequest {
            snapshot: serde_json::json!({}),
            destination: PathBuf::from("output.wav"),
            start_tick: 0,
            end_tick: 1,
            sample_rate: 48_000,
            block_size: 512,
            master_gain_db: 0.0,
            normalize: false,
        };

        let error = worker
            .render_timeline_offline_cancellable(request, &cancelled)
            .unwrap_err();

        assert_eq!(error, "render worker was cancelled");
    }
}
