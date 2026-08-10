use super::AudioSupervisor;
use super::StartupState;
use super::command_bus::{CommandBus, fail_pending_requests, record_command_response};
use super::error::{NativeAudioError, NativeAudioResult};
use super::protocol::{
    NativeEvent, emit_audio_meters, emit_audio_status, handle_native_stdout, set_faulted,
    set_starting,
};
use super::recovery::RecoveryState;
use super::sidecar_process::SidecarProcess;
use crate::audio_preferences::AudioPreferences;
use crate::model::{AudioState, AudioStatus, RecordingStatus};
use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};

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
                plugin: None,
                midi_inputs: Vec::new(),
                midi_outputs: Vec::new(),
                midi_input_active: false,
                midi_messages: 0,
                last_midi_note: None,
                midi_pad_mappings: 0,
                midi_pad_triggers: 0,
                input_peak: 0.0,
                output_peak: 0.0,
                invalid_samples: 0,
                feedback_suspected: false,
                message: message.into(),
            })),
            command_bus: Arc::new(CommandBus::new()),
            process,
            recovery: Arc::new(RecoveryState::new(AudioPreferences::default())),
            startup_state: Arc::new(std::sync::atomic::AtomicU8::new(
                StartupState::Completed as u8,
            )),
            startup_transition_gate,
        }
    }

    pub fn start<R: Runtime>(app: &AppHandle<R>, preferences: AudioPreferences) -> Self {
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
            plugin: None,
            midi_inputs: Vec::new(),
            midi_outputs: Vec::new(),
            midi_input_active: false,
            midi_messages: 0,
            last_midi_note: None,
            midi_pad_mappings: 0,
            midi_pad_triggers: 0,
            input_peak: 0.0,
            output_peak: 0.0,
            invalid_samples: 0,
            feedback_suspected: false,
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
        };
        let generation = supervisor.next_sidecar_generation();
        match supervisor.spawn_sidecar(app, generation) {
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

    pub(crate) fn sidecar_generation(&self) -> u64 {
        self.process.current_generation()
    }

    pub(crate) fn emit_status<R: Runtime>(&self, app: &AppHandle<R>) {
        emit_audio_status(app, &self.status);
    }

    pub(crate) fn wait_until_ready(
        &self,
        generation: u64,
        timeout: Duration,
    ) -> NativeAudioResult<()> {
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

    pub(crate) fn wait_for_next_generation(
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

    pub(crate) fn sidecar_terminated(&self, generation: u64) -> bool {
        self.process.is_terminated(generation)
    }

    fn spawn_sidecar<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        generation: u64,
    ) -> NativeAudioResult<CommandChild> {
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
        let (mut receiver, child) = app
            .shell()
            .sidecar("riffra-audio")
            .and_then(|command| command.args(arguments).spawn())
            .map_err(|error| NativeAudioError::process(error.to_string()))?;

        let event_status = Arc::clone(&self.status);
        let event_responses = Arc::clone(&self.command_bus.responses);
        let event_generation = Arc::clone(&self.process.generation);
        let event_process = Arc::clone(&self.process);
        let event_app = app.clone();
        let event_supervisor = self.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = receiver.recv().await {
                if event_generation.load(Ordering::Acquire) != generation {
                    if matches!(event, CommandEvent::Error(_) | CommandEvent::Terminated(_)) {
                        event_process.mark_terminated(generation);
                        break;
                    }
                    continue;
                }
                match event {
                    CommandEvent::Stdout(bytes) => {
                        let Ok(_event_gate) = event_process.command_gate.lock() else {
                            continue;
                        };
                        if event_generation.load(Ordering::Acquire) != generation {
                            continue;
                        }
                        if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                            match payload.get("type").and_then(serde_json::Value::as_str) {
                                Some("transportStatus") => {
                                    let _ = event_app.emit("transport-status", &payload);
                                }
                                Some("trackPluginStateChanged") => {
                                    let _ = event_app.emit("track-plugin-state-changed", &payload);
                                }
                                Some("trackPluginParameterChanged") => {
                                    let _ =
                                        event_app.emit("track-plugin-parameter-changed", &payload);
                                }
                                _ => {}
                            }
                        }
                        if let Some(response) = handle_native_stdout(&event_status, &bytes) {
                            if let Some(request_id) = response.request_id {
                                record_command_response(
                                    &event_responses,
                                    request_id,
                                    response.result.as_ref().err().cloned(),
                                );
                            }
                            // Transport acknowledgements are already delivered
                            // through the dedicated transport-status event. Meter
                            // frames also use a small payload so they do not
                            // serialize the full plugin/session status 20 times
                            // per second or invalidate the entire React tree.
                            if matches!(response.event, NativeEvent::AudioStatus) {
                                event_process.mark_ready(generation);
                            }
                            match response.event {
                                NativeEvent::AudioStatus => {
                                    emit_audio_status(&event_app, &event_status)
                                }
                                NativeEvent::AudioMeters => {
                                    emit_audio_meters(&event_app, &event_status)
                                }
                                NativeEvent::None => {}
                            }
                        }
                    }
                    CommandEvent::Stderr(bytes) => {
                        let Ok(_event_gate) = event_process.command_gate.lock() else {
                            continue;
                        };
                        if event_generation.load(Ordering::Acquire) != generation {
                            continue;
                        }
                        let detail = String::from_utf8_lossy(&bytes);
                        set_faulted(
                            &event_status,
                            format!(
                                "Native audio diagnostic: {detail}. The engine is isolated and saved data is safe."
                            ),
                        );
                        emit_audio_status(&event_app, &event_status);
                    }
                    CommandEvent::Error(error) => {
                        event_supervisor.handle_sidecar_exit(
                            &event_app,
                            generation,
                            NativeAudioError::transport_lost(format!(
                                "Native audio transport lost: communication failed ({error}). The engine is isolated and saved data is safe."
                            )),
                        );
                    }
                    CommandEvent::Terminated(payload) => {
                        event_supervisor.handle_sidecar_exit(
                            &event_app,
                            generation,
                            NativeAudioError::transport_lost(format!(
                                "Native audio transport lost: process stopped (code {:?}); the UI and saved session remain available.",
                                payload.code
                            )),
                        );
                    }
                    _ => {}
                }
            }
            event_process.mark_terminated(generation);
        });
        Ok(child)
    }

    fn handle_sidecar_exit<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        generation: u64,
        error: NativeAudioError,
    ) {
        self.process.mark_terminated(generation);
        set_faulted(&self.status, error.to_string());
        fail_pending_requests(&self.command_bus.responses, error);
        emit_audio_status(app, &self.status);

        let planned = self.process.take_planned_termination(generation);
        if planned || self.process.shutting_down.load(Ordering::Acquire) {
            return;
        }

        let supervisor = self.clone();
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(error) =
                supervisor.restart_sidecar_for_runtime(&app, generation, Duration::from_secs(20))
            {
                set_faulted(
                    &supervisor.status,
                    format!(
                        "Native audio sidecar could not auto-restart: {error}. Saved data remains safe."
                    ),
                );
            }
        });
    }

    pub(super) fn restart_sidecar<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        starting_message: &str,
        expected_generation: u64,
    ) -> NativeAudioResult<()> {
        self.restart_sidecar_with_timeout(
            app,
            starting_message,
            Duration::from_secs(15),
            Some(expected_generation),
        )
    }

    fn restart_sidecar_with_timeout<R: Runtime>(
        &self,
        app: &AppHandle<R>,
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
            let child = self.spawn_sidecar(app, generation).map_err(|spawn_error| {
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
            let _ = app.emit(
                "runtime-restarted",
                serde_json::json!({ "generation": self.sidecar_generation() }),
            );
        }
        result
    }

    /// Terminates the sidecar at an explicit application lifecycle boundary.
    ///
    /// `AudioSupervisor` is cloned by the event monitor, recovery worker, and
    /// Runtime ports. Shutdown therefore must not be tied to `Drop`: releasing
    /// any one of those clones must not stop a sidecar still owned by the app.
    pub(crate) fn force_shutdown(&self) {
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

    pub(crate) fn restart_sidecar_for_runtime<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        expected_generation: u64,
        timeout: Duration,
    ) -> NativeAudioResult<()> {
        self.restart_sidecar_with_timeout(
            app,
            "The isolated audio runtime exceeded its lifecycle deadline and is restarting.",
            timeout,
            Some(expected_generation),
        )
    }
}
