//! Process adapter for device-independent Riffra offline rendering.

use crate::audio::wire::{OfflineRenderEnvelope, RenderMessage, SIDECAR_PROTOCOL_VERSION};
use crate::execution::OfflineRenderRequest as OfflineRenderRequestSpec;
use crate::render::OfflineRenderRequest;
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use thiserror::Error;

/// Failure reported while invoking the offline render worker.
#[derive(Debug, Error)]
enum RenderWorkerError {
    #[error("render worker could not start: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("render request could not be encoded: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("render request could not be sent: {0}")]
    Write(#[source] std::io::Error),
    #[error("render worker could not be awaited: {0}")]
    Wait(#[source] std::io::Error),
    #[error(
        "render worker returned an invalid response: {error}; stdout: {stdout}; stderr: {stderr}"
    )]
    InvalidResponse {
        #[source]
        error: serde_json::Error,
        stdout: String,
        stderr: String,
    },
    #[error("render worker failed: {0}")]
    Rejected(String),
    #[error("render worker exited without completing the render: {status}; stderr: {stderr}")]
    Incomplete { status: String, stderr: String },
    #[error("render worker was cancelled")]
    Cancelled,
}

/// Bounded single-line view of the tail of a worker byte stream for error
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

/// Launches one `riffra-render` process for each offline render request.
#[derive(Clone)]
pub(crate) struct RenderWorker {
    executable: PathBuf,
}

impl RenderWorker {
    /// Creates an adapter for an explicit worker executable.
    pub(crate) fn new(executable: PathBuf) -> Self {
        Self { executable }
    }

    fn render_with_cancellation(
        &self,
        request: OfflineRenderRequest,
        cancelled: Option<&AtomicBool>,
    ) -> Result<(), RenderWorkerError> {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(RenderWorkerError::Cancelled);
        }
        let payload = OfflineRenderEnvelope::RenderTimelineOffline {
            protocol_version: SIDECAR_PROTOCOL_VERSION,
            request: OfflineRenderRequestSpec {
                graph: request.graph,
                destination: request.destination.to_string_lossy().into_owned(),
                start_tick: request.start_tick,
                end_tick: request.end_tick,
                sample_rate: request.sample_rate,
                block_size: request.block_size,
                normalize: request.normalize,
            },
        };
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
        let response: RenderMessage = serde_json::from_slice(&output.stdout).map_err(|source| {
            RenderWorkerError::InvalidResponse {
                error: source,
                stdout: excerpt(&output.stdout),
                stderr: excerpt(&output.stderr),
            }
        })?;
        match response {
            RenderMessage::OfflineRenderComplete {
                frames,
                sample_rate,
            } if output.status.success() => {
                tracing::info!(frames, sample_rate, "offline render worker completed");
                Ok(())
            }
            RenderMessage::OfflineRenderComplete { .. } => Err(RenderWorkerError::Incomplete {
                status: output.status.to_string(),
                stderr: excerpt(&output.stderr),
            }),
            RenderMessage::Error(error) => Err(RenderWorkerError::Rejected(error.message)),
        }
    }

    /// Runs one offline render while observing a cancellation flag.
    ///
    /// # Errors
    /// Returns a host-provided description when the render cannot be completed
    /// or the flag requests cancellation.
    pub(super) fn render_timeline_offline_cancellable(
        &self,
        request: OfflineRenderRequest,
        cancelled: &AtomicBool,
    ) -> Result<(), String> {
        self.render_with_cancellation(request, Some(cancelled))
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{ExecutionGraph, GraphLoopRange, GraphTimebase};
    use std::sync::atomic::AtomicBool;

    fn empty_graph() -> ExecutionGraph {
        ExecutionGraph {
            timebase: GraphTimebase {
                ppq: 960,
                bpm: 120.0,
                time_signature_numerator: 4,
                time_signature_denominator: 4,
            },
            loop_range: GraphLoopRange {
                enabled: false,
                start_tick: 0,
                end_tick: 0,
            },
            punch_range: None,
            metronome_enabled: false,
            master_gain_db: 0.0,
            tracks: Vec::new(),
        }
    }

    fn offline_request(destination: PathBuf) -> OfflineRenderRequest {
        OfflineRenderRequest {
            graph: empty_graph(),
            destination,
            start_tick: 0,
            end_tick: 960,
            sample_rate: 48_000,
            block_size: 512,
            normalize: false,
        }
    }

    #[test]
    fn explicit_worker_path_is_preserved() {
        // Arrange
        let path =
            PathBuf::from("workers").join(format!("riffra-render{}", std::env::consts::EXE_SUFFIX));

        // Act
        let worker = RenderWorker::new(path.clone());

        // Assert
        assert_eq!(worker.executable, path);
    }

    #[test]
    fn cancellation_is_observed_before_worker_spawn() {
        let worker = RenderWorker::new(PathBuf::from("missing-render-worker"));
        let cancelled = AtomicBool::new(true);
        let request = offline_request(PathBuf::from("output.wav"));

        let error = worker
            .render_timeline_offline_cancellable(request, &cancelled)
            .unwrap_err();

        assert_eq!(error, "render worker was cancelled");
    }

    #[test]
    #[ignore = "requires a built riffra-render executable"]
    fn renders_wave_without_an_audio_device() {
        // Arrange
        let executable =
            std::env::var_os("RIFFRA_RENDER_WORKER").expect("RIFFRA_RENDER_WORKER must be set");
        let destination = std::env::temp_dir().join(format!(
            "riffra-native-worker-render-{}.wav",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&destination);
        let worker = RenderWorker::new(executable.into());
        let request = offline_request(destination.clone());

        // Act
        worker
            .render_timeline_offline_cancellable(request, &AtomicBool::new(false))
            .expect("offline render should succeed");

        // Assert
        let wave = std::fs::read(&destination).expect("rendered WAV");
        assert!(wave.len() > 44);
        assert_eq!(&wave[..4], b"RIFF");
        assert_eq!(&wave[8..12], b"WAVE");
        std::fs::remove_file(destination).expect("rendered WAV cleanup");
    }
}
