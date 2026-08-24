use super::AudioSupervisor;
use super::StartupState;
use super::command_bus::{CommandBus, fail_pending_requests, record_command_response};
use super::error::{NativeAudioError, NativeAudioResult};
use super::protocol::{NativeEvent, handle_native_stdout, set_faulted, set_starting};
use super::recovery::RecoveryState;
use super::sidecar_process::{ChildProcess, SidecarProcess};
use crate::model::{AudioState, AudioStatus, RecordingStatus};
use crate::preferences::AudioPreferences;
use crate::{HostEvent, RuntimeBinaries, SharedHostEventSink};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, atomic::Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn remaining_timeout(
    deadline: Instant,
    maximum: Duration,
) -> NativeAudioResult<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(NativeAudioError::DeadlineExpired)
    } else {
        Ok(remaining.min(maximum))
    }
}

impl AudioSupervisor {
    pub fn offline(message: impl Into<String>) -> Self {
        Self::offline_with_events(message, Arc::new(crate::NoopHostEventSink))
    }

    pub fn offline_with_events(message: impl Into<String>, events: SharedHostEventSink) -> Self {
        let process = Arc::new(SidecarProcess::new(true));
        let startup_transition_gate = Arc::clone(&process.startup_transition_gate);
        Self {
            status: Arc::new(Mutex::new(AudioStatus {
                state: AudioState::Offline,
                driver: None,
                input_device: None,
                input_channel: None,
                input_channels: Vec::new(),
                output_device: None,
                output_channels: Vec::new(),
                sample_rate: None,
                buffer_size: None,
                round_trip_ms: None,
                timeline_tick: None,
                recording: RecordingStatus::default(),
                midi_inputs: Vec::new(),
                midi_outputs: Vec::new(),
                midi_input_active: false,
                midi_messages: 0,
                last_midi_note: None,
                input_peak: 0.0,
                output_peak: 0.0,
                invalid_samples: 0,
                feedback_suspected: false,
                previewing: false,
                message: message.into(),
            })),
            command_bus: Arc::new(CommandBus::new()),
            process,
            recovery: Arc::new(RecoveryState::new(AudioPreferences::default())),
            startup_state: Arc::new(std::sync::atomic::AtomicU8::new(
                StartupState::Completed as u8,
            )),
            startup_transition_gate,
            binaries: Arc::new(RuntimeBinaries::new(
                std::path::PathBuf::new(),
                std::path::PathBuf::new(),
                std::path::PathBuf::new(),
            )),
            events,
        }
    }

    pub fn start(
        binaries: &RuntimeBinaries,
        preferences: AudioPreferences,
        events: SharedHostEventSink,
    ) -> Self {
        let status = Arc::new(Mutex::new(AudioStatus {
            state: AudioState::Starting,
            driver: None,
            input_device: None,
            input_channel: None,
            input_channels: Vec::new(),
            output_device: None,
            output_channels: Vec::new(),
            sample_rate: None,
            buffer_size: None,
            round_trip_ms: None,
            timeline_tick: None,
            recording: RecordingStatus::default(),
            midi_inputs: Vec::new(),
            midi_outputs: Vec::new(),
            midi_input_active: false,
            midi_messages: 0,
            last_midi_note: None,
            input_peak: 0.0,
            output_peak: 0.0,
            invalid_samples: 0,
            feedback_suspected: false,
            previewing: false,
            message: "Native audio sidecar is starting in emergency-mute state.".into(),
        }));
        let process = Arc::new(SidecarProcess::new(false));
        let startup_transition_gate = Arc::clone(&process.startup_transition_gate);
        let supervisor = Self {
            status: Arc::clone(&status),
            command_bus: Arc::new(CommandBus::new()),
            process,
            recovery: Arc::new(RecoveryState::new(preferences)),
            startup_state: Arc::new(std::sync::atomic::AtomicU8::new(
                StartupState::Pending as u8,
            )),
            startup_transition_gate,
            binaries: Arc::new(binaries.clone()),
            events,
        };
        let generation = supervisor.next_sidecar_generation();
        match supervisor.spawn_sidecar(generation) {
            Ok(child) => {
                if let Ok(mut slot) = supervisor.process.child.lock() {
                    *slot = Some(child);
                }
            }
            Err(error) => set_faulted(
                &supervisor.status,
                format!(
                    "Native audio sidecar could not start; the session and saved data remain available: {error}"
                ),
            ),
        }

        supervisor
    }

    fn next_sidecar_generation(&self) -> u64 {
        self.process.next_generation()
    }

    pub fn sidecar_generation(&self) -> u64 {
        self.process.current_generation()
    }

    /// Emits the latest audio status through the Host event sink.
    pub fn emit_status(&self) {
        if let Ok(status) = self.status.lock() {
            self.events.emit(HostEvent::AudioStatus(status.clone()));
        }
    }

    pub fn wait_until_ready(&self, generation: u64, timeout: Duration) -> NativeAudioResult<()> {
        if self.process.shutting_down.load(Ordering::Acquire) {
            return Err(NativeAudioError::ShuttingDown);
        }
        if self.process.wait_for_ready(generation, timeout) {
            return Ok(());
        }
        let actual = self.sidecar_generation();
        if actual != generation {
            return Err(NativeAudioError::GenerationChanged {
                expected: generation,
                actual,
            });
        }
        if self.process.is_terminated(generation) {
            return Err(NativeAudioError::transport_lost(
                "Native audio sidecar terminated before it became ready.",
            ));
        }
        if self.process.shutting_down.load(Ordering::Acquire) {
            return Err(NativeAudioError::ShuttingDown);
        }
        Err(NativeAudioError::Timeout {
            message: format!(
                "Native audio sidecar did not become ready within {} seconds.",
                timeout.as_secs().max(1)
            ),
        })
    }

    pub fn wait_for_next_generation(
        &self,
        generation: u64,
        timeout: Duration,
    ) -> NativeAudioResult<u64> {
        if self.process.shutting_down.load(Ordering::Acquire) {
            return Err(NativeAudioError::ShuttingDown);
        }
        if let Some(next_generation) = self.process.wait_for_next_generation(generation, timeout) {
            return Ok(next_generation);
        }
        if self.process.shutting_down.load(Ordering::Acquire) {
            return Err(NativeAudioError::ShuttingDown);
        }
        Err(NativeAudioError::Timeout {
            message: format!(
                "Native audio sidecar did not start a replacement generation within {} seconds.",
                timeout.as_secs().max(1)
            ),
        })
    }

    pub fn sidecar_terminated(&self, generation: u64) -> bool {
        self.process.is_terminated(generation)
    }

    fn spawn_sidecar(&self, generation: u64) -> NativeAudioResult<ChildProcess> {
        let parent_pid = std::process::id().to_string();
        let preferences = self
            .recovery
            .restart_preferences
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Audio preference",
            })?
            .clone();
        let mut arguments = vec![
            "--serve".to_string(),
            "--parent-pid".to_string(),
            parent_pid,
            "--audio-driver".to_string(),
            preferences.driver,
        ];
        if let Some(input_device) = preferences.input_device {
            arguments.extend(["--input-device".to_string(), input_device]);
        }
        arguments.extend([
            "--input-channel".to_string(),
            preferences.input_channel.to_string(),
        ]);
        if let Some(output_device) = preferences.output_device {
            arguments.extend(["--output-device".to_string(), output_device]);
        }
        if let Some(sample_rate) = preferences.sample_rate {
            arguments.extend(["--sample-rate".to_string(), sample_rate.to_string()]);
        }
        if let Some(buffer_size) = preferences.buffer_size {
            arguments.extend(["--buffer-size".to_string(), buffer_size.to_string()]);
        }
        let mut child = Command::new(&self.binaries.audio)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| NativeAudioError::process(error.to_string()))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            NativeAudioError::process("Native audio process stdin is unavailable")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            NativeAudioError::process("Native audio process stdout is unavailable")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            NativeAudioError::process("Native audio process stderr is unavailable")
        })?;

        let event_status = Arc::clone(&self.status);
        let event_responses = Arc::clone(&self.command_bus.responses);
        let event_generation = Arc::clone(&self.process.generation);
        let event_process = Arc::clone(&self.process);
        let event_supervisor = self.clone();
        let event_events = Arc::clone(&self.events);
        if let Err(error) = thread::Builder::new()
            .name("riffra-audio-stdout".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if event_generation.load(Ordering::Acquire) != generation {
                        break;
                    }
                    let Ok(_event_gate) = event_process.command_gate.lock() else {
                        continue;
                    };
                    if event_generation.load(Ordering::Acquire) != generation {
                        continue;
                    }
                    let bytes = line.as_bytes();
                    if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(bytes) {
                        match payload.get("type").and_then(serde_json::Value::as_str) {
                            Some("transportStatus") => {
                                event_events.emit(HostEvent::TransportStatus(payload));
                            }
                            Some("trackPluginStateChanged") => {
                                event_events.emit(HostEvent::TrackPluginStateChanged(payload));
                            }
                            Some("trackPluginParameterChanged") => {
                                event_events.emit(HostEvent::TrackPluginParameterChanged(payload));
                            }
                            _ => {}
                        }
                    }
                    if let Some(response) = handle_native_stdout(&event_status, bytes) {
                        event_supervisor.synchronize_mute_cause_from_status();
                        if let Some(request_id) = response.request_id {
                            record_command_response(
                                &event_responses,
                                request_id,
                                response.result.as_ref().err().cloned(),
                            );
                        }
                        if matches!(response.event, NativeEvent::AudioStatus) {
                            event_process.mark_ready(generation);
                        }
                        match response.event {
                            NativeEvent::AudioStatus => {
                                if let Ok(status) = event_status.lock() {
                                    event_events.emit(HostEvent::AudioStatus(status.clone()));
                                }
                            }
                            NativeEvent::AudioMeters => {
                                if let Ok(status) = event_status.lock() {
                                    event_events.emit(HostEvent::AudioMeters(serde_json::json!({
                                        "inputPeak": status.input_peak,
                                        "outputPeak": status.output_peak,
                                        "invalidSamples": status.invalid_samples,
                                        "feedbackSuspected": status.feedback_suspected,
                                    })));
                                }
                            }
                            NativeEvent::None => {}
                        }
                    }
                }
                if event_generation.load(Ordering::Acquire) == generation {
                    event_supervisor.handle_sidecar_exit(
                    generation,
                    NativeAudioError::transport_lost(
                        "Native audio transport lost: process stopped; saved data remains safe.",
                    ),
                );
                }
                event_process.mark_terminated(generation);
            })
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NativeAudioError::process(format!(
                "audio stdout reader could not start: {error}"
            )));
        }

        let stderr_status = Arc::clone(&self.status);
        let stderr_events = Arc::clone(&self.events);
        let stderr_generation = Arc::clone(&self.process.generation);
        let stderr_process = Arc::clone(&self.process);
        if let Err(error) = thread::Builder::new()
            .name("riffra-audio-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if stderr_generation.load(Ordering::Acquire) != generation {
                        break;
                    }
                    let Ok(_event_gate) = stderr_process.command_gate.lock() else {
                        continue;
                    };
                    set_faulted(
                        &stderr_status,
                        format!(
                            "Native audio diagnostic: {line}. The engine is isolated and saved data is safe."
                        ),
                    );
                    if let Ok(status) = stderr_status.lock() {
                        stderr_events.emit(HostEvent::AudioStatus(status.clone()));
                    }
                }
            })
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NativeAudioError::process(format!(
                "audio stderr reader could not start: {error}"
            )));
        }

        Ok(ChildProcess { child, stdin })
    }

    fn handle_sidecar_exit(&self, generation: u64, error: NativeAudioError) {
        self.process.mark_terminated(generation);
        set_faulted(&self.status, error.to_string());
        fail_pending_requests(&self.command_bus.responses, error);
        self.emit_status();

        let planned = self.process.take_planned_termination(generation);
        if planned || self.process.shutting_down.load(Ordering::Acquire) {
            return;
        }

        let supervisor = self.clone();
        thread::Builder::new()
            .name("riffra-audio-recovery".into())
            .spawn(move || {
                if let Err(error) = supervisor
                    .restart_sidecar_for_runtime(generation, Duration::from_secs(20))
                {
                    set_faulted(
                        &supervisor.status,
                        format!(
                            "Native audio sidecar could not auto-restart: {error}. Saved data remains safe."
                        ),
                    );
                    supervisor.emit_status();
                }
            })
            .ok();
    }

    pub(super) fn restart_sidecar(
        &self,
        starting_message: &str,
        expected_generation: u64,
    ) -> NativeAudioResult<()> {
        self.restart_sidecar_with_timeout(
            starting_message,
            Duration::from_secs(15),
            Some(expected_generation),
        )
    }

    fn restart_sidecar_with_timeout(
        &self,
        starting_message: &str,
        timeout: Duration,
        expected_generation: Option<u64>,
    ) -> NativeAudioResult<()> {
        let _restart_gate =
            self.recovery
                .restart_gate
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Audio restart gate",
                })?;
        if self.process.shutting_down.load(Ordering::Acquire) {
            return Err(NativeAudioError::ShuttingDown);
        }

        let _command_gate =
            self.process
                .command_gate
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Audio command gate",
                })?;
        let current_generation = self.sidecar_generation();
        if let Some(expected_generation) = expected_generation
            && current_generation != expected_generation
        {
            return match self.completed_restart_outcome(expected_generation) {
                Some(result) => result,
                None => Err(NativeAudioError::GenerationChanged {
                    expected: expected_generation,
                    actual: current_generation,
                }),
            };
        }

        let deadline = Instant::now() + timeout;
        let previous_generation = current_generation;
        let generation = self.next_sidecar_generation();
        let result = (|| {
            fail_pending_requests(
                &self.command_bus.responses,
                NativeAudioError::transport_lost(
                    "Native audio sidecar is restarting; the command will be retried.",
                ),
            );
            let mut had_child = false;
            let mut kill_error = None;
            let mut slot =
                self.process
                    .child
                    .lock()
                    .map_err(|_| NativeAudioError::LockPoisoned {
                        resource: "Audio child",
                    })?;
            if let Some(child) = slot.take() {
                had_child = true;
                self.process.mark_planned_termination(previous_generation);
                kill_error = child.kill().err().map(|error| error.to_string());
            }
            drop(slot);
            if had_child {
                let termination_timeout = remaining_timeout(deadline, Duration::from_millis(1500))
                    .unwrap_or(Duration::from_millis(1));
                if !self
                    .process
                    .wait_for_termination(previous_generation, termination_timeout)
                {
                    let detail = kill_error
                        .map(|error| format!(" Kill error: {error}."))
                        .unwrap_or_default();
                    let message = format!(
                        "Native audio sidecar termination was not confirmed; a replacement was not started.{detail}"
                    );
                    set_faulted(&self.status, message.clone());
                    return Err(NativeAudioError::process(message));
                }
            }
            if self.process.shutting_down.load(Ordering::Acquire) {
                return Err(NativeAudioError::ShuttingDown);
            }
            set_starting(&self.status, starting_message);
            let child = self.spawn_sidecar(generation).map_err(|spawn_error| {
                set_faulted(
                    &self.status,
                    format!(
                        "Native audio sidecar could not restart: {spawn_error}. Saved data remains safe."
                    ),
                );
                NativeAudioError::process(format!(
                    "Native audio sidecar could not restart: {spawn_error}"
                ))
            })?;
            let mut slot =
                self.process
                    .child
                    .lock()
                    .map_err(|_| NativeAudioError::LockPoisoned {
                        resource: "Audio child",
                    })?;
            if self.process.shutting_down.load(Ordering::Acquire) {
                self.process.mark_planned_termination(generation);
                let _ = child.kill();
                return Err(NativeAudioError::ShuttingDown);
            }
            *slot = Some(child);
            drop(slot);
            drop(_command_gate);
            if let Err(error) = self.restore_runtime_controls(deadline) {
                set_faulted(
                    &self.status,
                    format!(
                        "Native audio sidecar restarted but runtime controls could not be restored: {error}"
                    ),
                );
                return Err(error);
            }
            Ok(())
        })();
        self.record_restart_outcome(previous_generation, &result);
        if result.is_ok() && self.startup_completed() {
            if let Some(handler) = self.runtime_restart_handler() {
                handler(self, self.sidecar_generation());
            }
            self.events.emit(HostEvent::RuntimeRestarted {
                generation: self.sidecar_generation(),
            });
        }
        result
    }

    /// Terminates the sidecar at an explicit application lifecycle boundary.
    ///
    /// `AudioSupervisor` is cloned by the event monitor, recovery worker, and
    /// Runtime ports. Shutdown therefore must not be tied to `Drop`: releasing
    /// any one of those clones must not stop a sidecar still owned by the app.
    pub fn force_shutdown(&self) {
        self.process.shutting_down.store(true, Ordering::Release);
        self.process.readiness.1.notify_all();
        fail_pending_requests(&self.command_bus.responses, NativeAudioError::ShuttingDown);
        let _command_gate = self.process.command_gate.lock().ok();
        if let Ok(mut slot) = self.process.child.lock()
            && let Some(child) = slot.take()
        {
            self.process
                .mark_planned_termination(self.sidecar_generation());
            let _ = child.kill();
        }
    }

    pub fn restart_sidecar_for_runtime(
        &self,
        expected_generation: u64,
        timeout: Duration,
    ) -> NativeAudioResult<()> {
        self.restart_sidecar_with_timeout(
            "The isolated audio runtime exceeded its lifecycle deadline and is restarting.",
            timeout,
            Some(expected_generation),
        )
    }
}
