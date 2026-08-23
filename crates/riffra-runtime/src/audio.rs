use crate::model::{AudioState, AudioStatus};
use crate::{HostEvent, HostEventSink, RuntimeBinaries};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Errors raised while controlling the isolated audio process.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AudioError {
    #[error("audio runtime is unavailable: {0}")]
    Unavailable(String),
    #[error("audio runtime process failed: {0}")]
    Process(String),
    #[error("audio runtime state lock was poisoned")]
    StatePoisoned,
}

struct ChildProcess {
    child: Child,
    stdin: ChildStdin,
}

/// Tauri-independent owner of the native audio process and its generation.
#[derive(Clone)]
pub struct AudioSupervisor {
    status: Arc<Mutex<AudioStatus>>,
    child: Arc<Mutex<Option<ChildProcess>>>,
    generation: Arc<AtomicU64>,
    shutting_down: Arc<AtomicBool>,
    startup_reported: Arc<AtomicBool>,
    safe_mode: bool,
    events: Arc<dyn HostEventSink>,
}

impl AudioSupervisor {
    /// Creates an offline supervisor used by Safe Mode and deterministic tests.
    pub fn offline(message: impl Into<String>, events: Arc<dyn HostEventSink>) -> Self {
        let supervisor = Self {
            status: Arc::new(Mutex::new(AudioStatus {
                state: AudioState::Offline,
                message: message.into(),
                ..AudioStatus::default()
            })),
            child: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            startup_reported: Arc::new(AtomicBool::new(true)),
            safe_mode: true,
            events,
        };
        if let Ok(status) = supervisor.status() {
            supervisor.events.emit(HostEvent::AudioStatus(status));
        }
        supervisor
            .events
            .emit(HostEvent::RuntimeStartupFinished { succeeded: false });
        supervisor
    }

    /// Starts the explicitly configured audio executable.
    ///
    /// A missing or failing native binary leaves the Host usable for canonical
    /// editing while reporting a Faulted audio status.
    pub fn start(
        binaries: &RuntimeBinaries,
        arguments: &[String],
        events: Arc<dyn HostEventSink>,
    ) -> Self {
        let supervisor = Self {
            status: Arc::new(Mutex::new(AudioStatus {
                state: AudioState::Starting,
                message: "native audio runtime is starting in emergency-mute state".into(),
                ..AudioStatus::default()
            })),
            child: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(1)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            startup_reported: Arc::new(AtomicBool::new(false)),
            safe_mode: false,
            events,
        };
        if let Ok(status) = supervisor.status() {
            supervisor.events.emit(HostEvent::AudioStatus(status));
        }
        if let Err(error) = supervisor.spawn(binaries, arguments) {
            supervisor.set_faulted(error.to_string());
        }
        supervisor
    }

    fn spawn(&self, binaries: &RuntimeBinaries, arguments: &[String]) -> Result<(), AudioError> {
        let mut command = Command::new(&binaries.audio);
        command
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| AudioError::Process(error.to_string()))?;
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                terminate_child(&mut child);
                return Err(AudioError::Process(
                    "audio runtime stdin is unavailable".into(),
                ));
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                terminate_child(&mut child);
                return Err(AudioError::Process(
                    "audio runtime stdout is unavailable".into(),
                ));
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                terminate_child(&mut child);
                return Err(AudioError::Process(
                    "audio runtime stderr is unavailable".into(),
                ));
            }
        };
        let status = Arc::clone(&self.status);
        let events = Arc::clone(&self.events);
        let shutting_down = Arc::clone(&self.shutting_down);
        let startup_reported = Arc::clone(&self.startup_reported);
        let generation = self.generation.load(Ordering::Acquire);
        if let Err(error) = std::thread::Builder::new()
            .name("riffra-audio-stdout".into())
            .spawn(move || {
                read_stdout(
                    stdout,
                    status,
                    events,
                    generation,
                    shutting_down,
                    startup_reported,
                )
            })
        {
            self.shutting_down.store(true, Ordering::Release);
            terminate_child(&mut child);
            return Err(AudioError::Process(error.to_string()));
        }
        if let Err(error) = std::thread::Builder::new()
            .name("riffra-audio-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    tracing::debug!(target: "riffra_runtime::audio", line = %line, "audio runtime stderr");
                }
            })
        {
            self.shutting_down.store(true, Ordering::Release);
            terminate_child(&mut child);
            return Err(AudioError::Process(error.to_string()));
        }
        let mut slot = match self.child.lock() {
            Ok(slot) => slot,
            Err(_) => {
                self.shutting_down.store(true, Ordering::Release);
                terminate_child(&mut child);
                return Err(AudioError::StatePoisoned);
            }
        };
        *slot = Some(ChildProcess { child, stdin });
        Ok(())
    }

    /// Returns the current audio status.
    pub fn status(&self) -> Result<AudioStatus, AudioError> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| AudioError::StatePoisoned)
    }

    /// Returns the active native process generation.
    pub fn runtime_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Returns whether this supervisor is intentionally offline.
    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    /// Sends one native command.
    pub fn send(&self, command: Value) -> Result<(), AudioError> {
        if self.safe_mode {
            return Err(AudioError::Unavailable(
                "Safe Mode keeps the audio runtime offline".into(),
            ));
        }
        let mut child = self.child.lock().map_err(|_| AudioError::StatePoisoned)?;
        let process = child
            .as_mut()
            .ok_or_else(|| AudioError::Unavailable("native audio process is not running".into()))?;
        let mut payload =
            serde_json::to_vec(&command).map_err(|error| AudioError::Process(error.to_string()))?;
        payload.push(b'\n');
        process
            .stdin
            .write_all(&payload)
            .and_then(|()| process.stdin.flush())
            .map_err(|error| AudioError::Process(error.to_string()))
    }

    /// Requests transport playback.
    pub fn play_timeline(&self) -> Result<(), AudioError> {
        self.send(serde_json::json!({"type": "playTimeline"}))
    }

    /// Requests transport stop.
    pub fn stop_timeline(&self) -> Result<(), AudioError> {
        self.send(serde_json::json!({"type": "stopTimeline"}))
    }

    /// Requests a timeline seek.
    pub fn seek_timeline(&self, tick: u64) -> Result<(), AudioError> {
        self.send(serde_json::json!({"type": "seekTimeline", "tick": tick}))
    }

    fn set_faulted(&self, message: String) {
        if let Ok(mut status) = self.status.lock() {
            status.state = AudioState::Faulted;
            status.message = message;
            self.events.emit(HostEvent::AudioStatus(status.clone()));
            if self
                .startup_reported
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.events
                    .emit(HostEvent::RuntimeStartupFinished { succeeded: false });
            }
        }
    }

    /// Stops the process and prevents further commands.
    pub fn force_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        if let Ok(mut process) = self.child.lock()
            && let Some(mut process) = process.take()
        {
            terminate_child(&mut process.child);
        }
    }
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_stdout(
    stdout: impl std::io::Read,
    status: Arc<Mutex<AudioStatus>>,
    events: Arc<dyn HostEventSink>,
    generation: u64,
    shutting_down: Arc<AtomicBool>,
    startup_reported: Arc<AtomicBool>,
) {
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(payload) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if payload.get("type").and_then(Value::as_str) == Some("audioStatus") {
            if let Ok(mut current) = status.lock() {
                if let Some(state) = payload.get("state").and_then(Value::as_str) {
                    current.state = match state {
                        "starting" => AudioState::Starting,
                        "ready" => AudioState::Ready,
                        "muted" => AudioState::Muted,
                        "faulted" => AudioState::Faulted,
                        _ => AudioState::Offline,
                    };
                }
                if let Some(driver) = payload.get("driver").and_then(Value::as_str) {
                    current.driver = Some(driver.to_owned());
                }
                if let Some(message) = payload.get("message").and_then(Value::as_str) {
                    current.message = message.to_owned();
                }
                let startup_result = match current.state {
                    AudioState::Ready | AudioState::Muted => Some(true),
                    AudioState::Faulted | AudioState::Offline => Some(false),
                    AudioState::Starting => None,
                };
                events.emit(HostEvent::AudioStatus(current.clone()));
                if let Some(succeeded) = startup_result
                    && startup_reported
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    events.emit(HostEvent::RuntimeStartupFinished { succeeded });
                }
            }
        } else if payload.get("type").and_then(Value::as_str) == Some("audioMeters") {
            events.emit(HostEvent::AudioMeters(payload));
        } else if payload.get("type").and_then(Value::as_str) == Some("transportStatus") {
            events.emit(HostEvent::TransportStatus(payload));
        }
    }
    if !shutting_down.load(Ordering::Acquire) {
        if let Ok(mut current) = status.lock() {
            if !matches!(current.state, AudioState::Faulted | AudioState::Offline) {
                current.state = AudioState::Offline;
                current.message = "native audio runtime stopped".into();
            }
            events.emit(HostEvent::AudioStatus(current.clone()));
        }
        if startup_reported
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            events.emit(HostEvent::RuntimeStartupFinished { succeeded: false });
        }
        events.emit(HostEvent::RuntimeRestarted { generation });
    }
}
