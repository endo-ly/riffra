use crate::audio_preferences::AudioPreferences;
use crate::model::{
    AudioChannelInfo, AudioState, AudioStatus, PluginParameter, PluginStatus, RecordingStatus,
};
use crate::session::AudioTakeVariant;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};

/// Stable classification prefix for failures that mean the Rust supervisor no
/// longer has a usable transport to the isolated audio process. Recovery code
/// must classify this boundary, not depend on the human-readable tail of an
/// error message.
pub(crate) const NATIVE_AUDIO_TRANSPORT_LOST: &str = "Native audio transport lost";

#[derive(Clone)]
pub struct AudioSupervisor {
    status: Arc<Mutex<AudioStatus>>,
    runtime_controls: Arc<Mutex<RuntimeControlState>>,
    responses: Arc<(Mutex<CommandResponse>, Condvar)>,
    next_request_id: Arc<AtomicU64>,
    sidecar_generation: Arc<AtomicU64>,
    child: Arc<Mutex<Option<CommandChild>>>,
    terminated_generations: Arc<(Mutex<HashSet<u64>>, Condvar)>,
    restart_preferences: Arc<Mutex<AudioPreferences>>,
    planned_terminations: Arc<Mutex<HashSet<u64>>>,
    restart_gate: Arc<Mutex<()>>,
    restart_outcomes: Arc<Mutex<HashMap<u64, Result<(), String>>>>,
    shutting_down: Arc<AtomicBool>,
}

#[derive(Default)]
struct CommandResponse {
    results: HashMap<u64, Option<Result<(), String>>>,
}

#[derive(Clone, Debug)]
struct RuntimeControlState {
    processing_mode: String,
    processing_mode_sent: Option<String>,
    master_gain_db: f64,
    midi_listening: bool,
    emergency_muted: bool,
}

impl Default for RuntimeControlState {
    fn default() -> Self {
        Self {
            processing_mode: "passive".into(),
            processing_mode_sent: None,
            master_gain_db: -18.0,
            midi_listening: false,
            emergency_muted: true,
        }
    }
}

#[derive(Clone, Copy)]
enum NativeEvent {
    AudioStatus,
    AudioMeters,
    None,
}

struct NativeReply {
    request_id: Option<u64>,
    result: Result<(), String>,
    event: NativeEvent,
}

/// Sample-pad payload exchanged with the native audio sidecar. The sidecar
/// consumes resolved filesystem paths, not Asset ids, so this is a distinct
/// type from the domain [`crate::session::SamplePad`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSamplePad {
    pub id: String,
    pub name: String,
    pub asset_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub midi_key: u8,
    pub gain_db: f64,
    pub loop_enabled: bool,
}

/// JSON message body for the audio sidecar IPC.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeStatus {
    state: String,
    driver: Option<String>,
    input_device: Option<String>,
    input_channel: Option<u32>,
    input_channels: Option<Vec<NativeAudioChannelInfo>>,
    output_device: Option<String>,
    output_channels: Option<Vec<NativeAudioChannelInfo>>,
    sample_rate: Option<f64>,
    buffer_size: Option<u32>,
    round_trip_ms: Option<f64>,
    timeline_tick: Option<u64>,
    recording: Option<NativeRecordingStatus>,
    plugin: Option<NativePluginStatus>,
    midi_inputs: Option<Vec<crate::model::MidiDeviceInfo>>,
    midi_outputs: Option<Vec<crate::model::MidiDeviceInfo>>,
    midi_input_active: Option<bool>,
    midi_messages: Option<u64>,
    last_midi_note: Option<i32>,
    midi_pad_mappings: Option<u32>,
    midi_pad_triggers: Option<u64>,
    input_peak: Option<f64>,
    output_peak: Option<f64>,
    invalid_samples: Option<u64>,
    feedback_suspected: Option<bool>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeMeters {
    input_peak: Option<f64>,
    output_peak: Option<f64>,
    invalid_samples: Option<u64>,
    feedback_suspected: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeRecordingStatus {
    active: bool,
    #[serde(default)]
    cancelled: bool,
    directory: Option<String>,
    sample_rate: Option<f64>,
    raw_channels: Option<u32>,
    processed_channels: Option<u32>,
    samples_written: Option<u64>,
    dropped_blocks: Option<u64>,
    missing_samples: Option<u64>,
    dropout_start_sample: Option<u64>,
    dropout_end_sample: Option<u64>,
    raw_attempted_samples: Option<u64>,
    processed_attempted_samples: Option<u64>,
    raw_dropped_blocks: Option<u64>,
    processed_dropped_blocks: Option<u64>,
    raw_missing_samples: Option<u64>,
    processed_missing_samples: Option<u64>,
    raw_dropout_start_sample: Option<u64>,
    raw_dropout_end_sample: Option<u64>,
    processed_dropout_start_sample: Option<u64>,
    processed_dropout_end_sample: Option<u64>,
    recovery_status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativePluginStatus {
    loaded: bool,
    bypassed: bool,
    path: Option<String>,
    name: Option<String>,
    sample_rate: Option<f64>,
    block_size: Option<u32>,
    input_channels: Option<u32>,
    output_channels: Option<u32>,
    bypassed_blocks: Option<u64>,
    processed_blocks: Option<u64>,
    contention_blocks: Option<u64>,
    transition_blocks: Option<u64>,
    parameters: Option<Vec<NativePluginParameter>>,
    state_data: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeAudioChannelInfo {
    index: u32,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativePluginParameter {
    index: u32,
    name: String,
    value: f32,
    default_value: f32,
    automatable: bool,
}

fn normalize_sample_rate(rate: f64) -> Option<u32> {
    if !rate.is_finite() || rate <= 0.0 || rate > f64::from(u32::MAX) {
        return None;
    }
    let rounded = rate.round();
    if !(1.0..=f64::from(u32::MAX)).contains(&rounded) {
        return None;
    }
    Some(rounded as u32)
}

fn remaining_timeout(deadline: Instant, maximum: Duration) -> Result<Duration, String> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err("Audio Runtime recovery deadline expired before the next control step.".into())
    } else {
        Ok(remaining.min(maximum))
    }
}

impl AudioSupervisor {
    pub fn offline(message: impl Into<String>) -> Self {
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
            runtime_controls: Arc::new(Mutex::new(RuntimeControlState::default())),
            responses: Arc::new((Mutex::new(CommandResponse::default()), Condvar::new())),
            next_request_id: Arc::new(AtomicU64::new(1)),
            sidecar_generation: Arc::new(AtomicU64::new(0)),
            child: Arc::new(Mutex::new(None)),
            terminated_generations: Arc::new((Mutex::new(HashSet::new()), Condvar::new())),
            restart_preferences: Arc::new(Mutex::new(AudioPreferences::default())),
            planned_terminations: Arc::new(Mutex::new(HashSet::new())),
            restart_gate: Arc::new(Mutex::new(())),
            restart_outcomes: Arc::new(Mutex::new(HashMap::new())),
            shutting_down: Arc::new(AtomicBool::new(true)),
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
        let responses = Arc::new((Mutex::new(CommandResponse::default()), Condvar::new()));

        let supervisor = Self {
            status: Arc::clone(&status),
            runtime_controls: Arc::new(Mutex::new(RuntimeControlState::default())),
            responses,
            next_request_id: Arc::new(AtomicU64::new(1)),
            sidecar_generation: Arc::new(AtomicU64::new(0)),
            child: Arc::new(Mutex::new(None)),
            terminated_generations: Arc::new((Mutex::new(HashSet::new()), Condvar::new())),
            restart_preferences: Arc::new(Mutex::new(preferences)),
            planned_terminations: Arc::new(Mutex::new(HashSet::new())),
            restart_gate: Arc::new(Mutex::new(())),
            restart_outcomes: Arc::new(Mutex::new(HashMap::new())),
            shutting_down: Arc::new(AtomicBool::new(false)),
        };
        let generation = supervisor.next_sidecar_generation();
        match supervisor.spawn_sidecar(app, generation) {
            Ok(child) => {
                if let Ok(mut slot) = supervisor.child.lock() {
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
        self.sidecar_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(crate) fn sidecar_generation(&self) -> u64 {
        self.sidecar_generation.load(Ordering::Acquire)
    }

    fn spawn_sidecar<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        generation: u64,
    ) -> Result<CommandChild, String> {
        let parent_pid = std::process::id().to_string();
        let preferences = self
            .restart_preferences
            .lock()
            .map_err(|error| format!("Audio preference lock was poisoned: {error}"))?
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
            .map_err(|error| error.to_string())?;

        let event_status = Arc::clone(&self.status);
        let event_responses = Arc::clone(&self.responses);
        let event_generation = Arc::clone(&self.sidecar_generation);
        let event_terminated_generations = Arc::clone(&self.terminated_generations);
        let event_app = app.clone();
        let event_supervisor = self.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = receiver.recv().await {
                if event_generation.load(Ordering::Acquire) != generation {
                    if matches!(event, CommandEvent::Error(_) | CommandEvent::Terminated(_)) {
                        mark_sidecar_terminated(&event_terminated_generations, generation);
                        break;
                    }
                    continue;
                }
                match event {
                    CommandEvent::Stdout(bytes) => {
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
                        mark_sidecar_terminated(&event_terminated_generations, generation);
                        let message = format!(
                            "{NATIVE_AUDIO_TRANSPORT_LOST}: communication failed ({error}). The engine is isolated and saved data is safe."
                        );
                        set_faulted(&event_status, message.clone());
                        fail_pending_requests(&event_responses, message);
                        emit_audio_status(&event_app, &event_status);
                        let planned = event_supervisor.take_planned_termination(generation);
                        if !planned && !event_supervisor.shutting_down.load(Ordering::Acquire) {
                            let supervisor = event_supervisor.clone();
                            let app = event_app.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                if let Err(error) = supervisor.restart_sidecar_for_runtime(
                                    &app,
                                    generation,
                                    Duration::from_secs(20),
                                ) {
                                    set_faulted(
                                        &supervisor.status,
                                        format!(
                                            "Native audio sidecar could not auto-restart: {error}. Saved data remains safe."
                                        ),
                                    );
                                }
                            });
                        }
                    }
                    CommandEvent::Terminated(payload) => {
                        mark_sidecar_terminated(&event_terminated_generations, generation);
                        let message = format!(
                            "{NATIVE_AUDIO_TRANSPORT_LOST}: process stopped (code {:?}); the UI and saved session remain available.",
                            payload.code
                        );
                        set_faulted(&event_status, message.clone());
                        fail_pending_requests(&event_responses, message);
                        emit_audio_status(&event_app, &event_status);
                        let planned = event_supervisor.take_planned_termination(generation);
                        if !planned && !event_supervisor.shutting_down.load(Ordering::Acquire) {
                            let supervisor = event_supervisor.clone();
                            let app = event_app.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                if let Err(error) = supervisor.restart_sidecar_for_runtime(
                                    &app,
                                    generation,
                                    Duration::from_secs(20),
                                ) {
                                    set_faulted(
                                        &supervisor.status,
                                        format!(
                                            "Native audio sidecar could not auto-restart: {error}. Saved data remains safe."
                                        ),
                                    );
                                }
                            });
                        }
                    }
                    _ => {}
                }
            }
            mark_sidecar_terminated(&event_terminated_generations, generation);
        });
        Ok(child)
    }

    fn restart_sidecar<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        starting_message: &str,
        expected_generation: u64,
    ) -> Result<(), String> {
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
    ) -> Result<(), String> {
        let _restart_gate = self
            .restart_gate
            .lock()
            .map_err(|error| format!("Audio restart gate was poisoned: {error}"))?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(
                "Native audio sidecar restart was skipped because the app is shutting down.".into(),
            );
        }

        let current_generation = self.sidecar_generation();
        if let Some(expected_generation) = expected_generation
            && current_generation != expected_generation
        {
            return match self.completed_restart_outcome(expected_generation) {
                Some(result) => result,
                None => Err(format!(
                    "Native audio sidecar generation changed from {expected_generation} to {current_generation}, but its restart result is unavailable."
                )),
            };
        }

        let deadline = Instant::now() + timeout;
        let previous_generation = current_generation;
        let generation = self.next_sidecar_generation();
        let result = (|| {
            fail_pending_requests(
                &self.responses,
                "Native audio sidecar is restarting; the command will be retried.".into(),
            );
            let mut had_child = false;
            let mut kill_error = None;
            let mut slot = self
                .child
                .lock()
                .map_err(|error| format!("Audio child lock was poisoned: {error}"))?;
            if let Some(child) = slot.take() {
                had_child = true;
                self.mark_planned_termination(previous_generation);
                kill_error = child.kill().err().map(|error| error.to_string());
            }
            drop(slot);
            if had_child {
                let termination_timeout = remaining_timeout(deadline, Duration::from_millis(1500))
                    .unwrap_or(Duration::from_millis(1));
                if !self.wait_for_sidecar_termination(previous_generation, termination_timeout) {
                    let detail = kill_error
                        .map(|error| format!(" Kill error: {error}."))
                        .unwrap_or_default();
                    let message = format!(
                        "Native audio sidecar termination was not confirmed; a replacement was not started.{detail}"
                    );
                    set_faulted(&self.status, message.clone());
                    return Err(message);
                }
            }
            if self.shutting_down.load(Ordering::Acquire) {
                return Err(
                    "Native audio sidecar restart was cancelled because the app is shutting down."
                        .into(),
                );
            }
            set_starting(&self.status, starting_message);
            let child = self.spawn_sidecar(app, generation).map_err(|spawn_error| {
                set_faulted(
                    &self.status,
                    format!(
                        "Native audio sidecar could not restart: {spawn_error}. Saved data remains safe."
                    ),
                );
                format!("Native audio sidecar could not restart: {spawn_error}")
            })?;
            let mut slot = self
                .child
                .lock()
                .map_err(|error| format!("Audio child lock was poisoned: {error}"))?;
            if self.shutting_down.load(Ordering::Acquire) {
                self.mark_planned_termination(generation);
                let _ = child.kill();
                return Err(
                    "Native audio sidecar restart was cancelled because the app is shutting down."
                        .into(),
                );
            }
            *slot = Some(child);
            drop(slot);
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
        result
    }

    fn completed_restart_outcome(&self, previous_generation: u64) -> Option<Result<(), String>> {
        self.restart_outcomes
            .lock()
            .ok()?
            .get(&previous_generation)
            .cloned()
    }

    fn record_restart_outcome(&self, previous_generation: u64, result: &Result<(), String>) {
        let Ok(mut outcomes) = self.restart_outcomes.lock() else {
            return;
        };
        if outcomes.len() >= 32
            && let Some(oldest_generation) = outcomes.keys().min().copied()
        {
            outcomes.remove(&oldest_generation);
        }
        outcomes.insert(previous_generation, result.clone());
    }

    fn mark_planned_termination(&self, generation: u64) {
        if let Ok(mut planned) = self.planned_terminations.lock() {
            planned.insert(generation);
        }
    }

    fn take_planned_termination(&self, generation: u64) -> bool {
        self.planned_terminations
            .lock()
            .map(|mut planned| planned.remove(&generation))
            .unwrap_or(false)
    }

    fn restore_runtime_controls(&self, deadline: Instant) -> Result<(), String> {
        let controls = self
            .runtime_controls
            .lock()
            .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?
            .clone();
        self.wait_for_command(
            serde_json::json!({
                "type": "setProcessingMode",
                "mode": controls.processing_mode,
            }),
            remaining_timeout(deadline, Duration::from_secs(3))?,
        )?;
        self.wait_for_command(
            serde_json::json!({
                "type": "setMasterGainDb",
                "gainDb": controls.master_gain_db,
            }),
            remaining_timeout(deadline, Duration::from_secs(3))?,
        )?;
        self.wait_for_command(
            serde_json::json!({
                "type": if controls.midi_listening {
                    "enableMidiListening"
                } else {
                    "disableMidiListening"
                },
            }),
            remaining_timeout(deadline, Duration::from_secs(3))?,
        )?;
        // A recovered process must remain muted until the user explicitly
        // confirms that audio should fade back in. The other control values are
        // restored exactly, but safety mute is deliberately not auto-cleared.
        self.wait_for_command(
            serde_json::json!({"type": "setEmergencyMute", "muted": true}),
            remaining_timeout(deadline, Duration::from_secs(3))?,
        )?;
        if let Ok(mut current) = self.runtime_controls.lock() {
            current.processing_mode_sent = Some(current.processing_mode.clone());
            current.emergency_muted = true;
        }
        Ok(())
    }

    pub(crate) fn force_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        fail_pending_requests(
            &self.responses,
            "Native audio sidecar is shutting down.".into(),
        );
        if let Ok(mut slot) = self.child.lock()
            && let Some(child) = slot.take()
        {
            self.mark_planned_termination(self.sidecar_generation());
            let _ = child.kill();
        }
    }

    fn wait_for_sidecar_termination(&self, generation: u64, timeout: Duration) -> bool {
        let (terminated, changed) = &*self.terminated_generations;
        let guard = match terminated.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        let (mut guard, _) = match changed.wait_timeout_while(guard, timeout, |generations| {
            !generations.contains(&generation)
        }) {
            Ok(result) => result,
            Err(_) => return false,
        };
        guard.remove(&generation)
    }

    pub(crate) fn restart_sidecar_for_runtime<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        expected_generation: u64,
        timeout: Duration,
    ) -> Result<(), String> {
        self.restart_sidecar_with_timeout(
            app,
            "The isolated audio runtime exceeded its lifecycle deadline and is restarting.",
            timeout,
            Some(expected_generation),
        )
    }

    pub fn refresh_status(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "status"}),
            "Native audio status refreshed.",
        )
    }

    pub fn refresh_meters(&self) -> Result<AudioStatus, String> {
        self.send_command(serde_json::json!({"type": "meterStatus"}), "")
    }

    pub fn prepare_timeline_snapshot(
        &self,
        snapshot: serde_json::Value,
        timeout: Duration,
    ) -> Result<(), String> {
        self.send_command_ack(
            serde_json::json!({
                "type": "prepareTimelineSnapshot",
                "protocolVersion": 1,
                "snapshot": snapshot,
            }),
            "",
            timeout.min(Duration::from_secs(15)),
        )
    }

    pub fn commit_timeline_snapshot(&self, timeout: Duration) -> Result<(), String> {
        self.send_command_ack(
            serde_json::json!({"type": "commitTimelineSnapshot"}),
            "",
            timeout.min(Duration::from_secs(3)),
        )
    }

    pub fn discard_timeline_snapshot(&self, timeout: Duration) -> Result<(), String> {
        self.send_command_ack(
            serde_json::json!({"type": "discardTimelineSnapshot"}),
            "",
            timeout.min(Duration::from_secs(3)),
        )
    }

    pub fn play_timeline(&self) -> Result<(), String> {
        self.send_command(serde_json::json!({"type": "playTimeline"}), "")?;
        Ok(())
    }

    pub fn stop_timeline(&self) -> Result<(), String> {
        self.send_command(serde_json::json!({"type": "stopTimeline"}), "")?;
        Ok(())
    }

    pub fn stop_timeline_nonblocking(&self) -> Result<(), String> {
        self.send_command_without_wait(serde_json::json!({
            "type": "stopTimeline",
            "reportStatus": false,
        }))
    }

    pub fn seek_timeline(&self, tick: u64) -> Result<(), String> {
        self.send_command(
            serde_json::json!({"type": "seekTimeline", "tick": tick}),
            "",
        )?;
        Ok(())
    }

    pub fn set_processing_mode(&self, mode: &str) -> Result<AudioStatus, String> {
        if !matches!(mode, "play" | "arrange" | "passive") {
            return Err("Audio processing mode is invalid.".into());
        }
        // Keep the desired control ahead of the acknowledgement. If the
        // sidecar disappears while this request is in flight, a replacement
        // process must restore the user's latest mode rather than the last
        // mode that happened to acknowledge successfully.
        self.runtime_controls
            .lock()
            .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?
            .processing_mode = mode.into();
        let status = self.send_command(
            serde_json::json!({"type": "setProcessingMode", "mode": mode}),
            "",
        )?;
        self.runtime_controls
            .lock()
            .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?
            .processing_mode_sent = Some(mode.into());
        Ok(status)
    }

    /// Updates the desired processing mode and sends it without waiting for a
    /// status acknowledgement. Workspace navigation uses this path because a
    /// third-party VST must never hold the navigation/persistence boundary.
    /// Recovery restores the same desired value if the write races with a
    /// sidecar restart.
    pub fn set_processing_mode_nonblocking(&self, mode: &str) -> Result<(), String> {
        if !matches!(mode, "play" | "arrange" | "passive") {
            return Err("Audio processing mode is invalid.".into());
        }
        let changed = {
            let mut controls = self
                .runtime_controls
                .lock()
                .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?;
            if controls.processing_mode == mode
                && controls.processing_mode_sent.as_deref() == Some(mode)
            {
                false
            } else {
                controls.processing_mode = mode.into();
                true
            }
        };
        if !changed {
            return Ok(());
        }
        let result = self.send_command_without_wait(serde_json::json!({
            "type": "setProcessingMode",
            "mode": mode,
            "reportStatus": false,
        }));
        if result.is_ok()
            && let Ok(mut controls) = self.runtime_controls.lock()
        {
            controls.processing_mode_sent = Some(mode.into());
        }
        result
    }

    pub fn set_track_device_bypassed(
        &self,
        track_id: &str,
        device_id: &str,
        bypassed: bool,
    ) -> Result<(), String> {
        self.send_command(
            serde_json::json!({
                "type": "setTrackDeviceBypassed",
                "trackId": track_id,
                "deviceId": device_id,
                "bypassed": bypassed,
            }),
            "",
        )?;
        Ok(())
    }

    pub fn set_track_device_parameter(
        &self,
        track_id: &str,
        device_id: &str,
        parameter_index: u32,
        value: f32,
    ) -> Result<(), String> {
        if !value.is_finite() {
            return Err("Track Device parameter value must be finite.".into());
        }
        self.send_command(
            serde_json::json!({
                "type": "setTrackDeviceParameter",
                "trackId": track_id,
                "deviceId": device_id,
                "parameterIndex": parameter_index,
                "value": value.clamp(0.0, 1.0),
            }),
            "",
        )?;
        Ok(())
    }

    pub fn open_track_plugin_editor(&self, track_id: &str, device_id: &str) -> Result<(), String> {
        self.send_command_with_timeout(
            serde_json::json!({
                "type": "openTrackPluginEditor",
                "trackId": track_id,
                "deviceId": device_id,
            }),
            "",
            Duration::from_secs(10),
        )?;
        Ok(())
    }

    fn send_command(
        &self,
        command: serde_json::Value,
        message: &str,
    ) -> Result<AudioStatus, String> {
        self.send_command_with_timeout(command, message, Duration::from_secs(3))
    }

    fn send_command_with_timeout(
        &self,
        command: serde_json::Value,
        message: &str,
        timeout: Duration,
    ) -> Result<AudioStatus, String> {
        self.wait_for_command(command, timeout)?;
        let mut status = self
            .status
            .lock()
            .map_err(|error| format!("Audio status lock was poisoned: {error}"))?;
        if !message.is_empty() {
            status.message = message.into();
        }
        Ok(status.clone())
    }

    /// Waits for a sidecar acknowledgement without cloning the full
    /// [`AudioStatus`]. High-rate realtime commands such as MIDI only need an
    /// acknowledgement; cloning plugin state data for every note can otherwise
    /// turn a performance path into a large allocation/serialization path.
    fn send_command_ack(
        &self,
        command: serde_json::Value,
        message: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        self.wait_for_command(command, timeout)?;
        if !message.is_empty() {
            let mut status = self
                .status
                .lock()
                .map_err(|error| format!("Audio status lock was poisoned: {error}"))?;
            status.message = message.into();
        }
        Ok(())
    }

    fn send_command_without_wait(&self, command: serde_json::Value) -> Result<(), String> {
        let payload = serde_json::to_string(&command)
            .map_err(|error| format!("Audio command could not be encoded: {error}"))?;
        let mut child_slot = self
            .child
            .lock()
            .map_err(|error| format!("Audio child lock was poisoned: {error}"))?;
        let child = child_slot.as_mut().ok_or_else(|| {
            format!("{NATIVE_AUDIO_TRANSPORT_LOST}: the requested audio command was not sent.")
        })?;
        child
            .write(format!("{payload}\n").as_bytes())
            .map_err(|error| format!("{NATIVE_AUDIO_TRANSPORT_LOST}: {error}"))
    }

    fn wait_for_command(
        &self,
        mut command: serde_json::Value,
        timeout: Duration,
    ) -> Result<(), String> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        command["requestId"] = serde_json::json!(request_id);
        let payload = serde_json::to_string(&command)
            .map_err(|error| format!("Audio command could not be encoded: {error}"))?;
        let (response_lock, response_ready) = &*self.responses;
        {
            let mut response = response_lock
                .lock()
                .map_err(|error| format!("Audio response lock was poisoned: {error}"))?;
            response.results.insert(request_id, None);
        }

        let write_result = {
            let mut child_slot = self
                .child
                .lock()
                .map_err(|error| format!("Audio child lock was poisoned: {error}"))?;
            let child = child_slot.as_mut().ok_or_else(|| {
                format!("{NATIVE_AUDIO_TRANSPORT_LOST}: the requested audio command was not sent.")
            });
            child.and_then(|child| {
                child
                    .write(format!("{payload}\n").as_bytes())
                    .map_err(|error| error.to_string())
            })
        };
        if let Err(error) = write_result {
            if let Ok(mut response) = response_lock.lock() {
                response.results.remove(&request_id);
            }
            return Err(format!(
                "{NATIVE_AUDIO_TRANSPORT_LOST}: command could not reach the isolated audio process: {error}"
            ));
        }

        let response = response_lock
            .lock()
            .map_err(|error| format!("Audio response lock was poisoned: {error}"))?;
        let wait = response_ready.wait_timeout_while(response, timeout, |current| {
            current.results.get(&request_id).is_none_or(Option::is_none)
        });
        let (mut response, wait_result) =
            wait.map_err(|error| format!("Audio response wait failed: {error}"))?;
        if wait_result.timed_out()
            && response
                .results
                .get(&request_id)
                .is_none_or(Option::is_none)
        {
            response.results.remove(&request_id);
            return Err(format!(
                "{NATIVE_AUDIO_TRANSPORT_LOST}: command was not acknowledged within {} seconds.",
                timeout.as_secs()
            ));
        }

        let result = response
            .results
            .remove(&request_id)
            .flatten()
            .unwrap_or_else(|| Err("Native audio returned no command result.".into()));
        result?;
        Ok(())
    }

    pub fn load_plugin(
        &self,
        path: &Path,
        parameter_values: &[f32],
        bypassed: bool,
        state_data: Option<&str>,
    ) -> Result<AudioStatus, String> {
        if parameter_values.iter().any(|value| !value.is_finite()) {
            return Err("VST3 parameter values must be finite.".into());
        }
        if state_data.is_some_and(|value| value.len() > 4_000_000) {
            return Err("VST3 state data exceeds the safe 4 MiB limit.".into());
        }
        let has_state = state_data.is_some_and(|value| !value.is_empty());
        // A non-empty opaque state is authoritative. Do not also replay the
        // parameter array: some plugins expose thousands of parameters and
        // applying both would duplicate work while allowing the stale array
        // to overwrite values restored by the plugin state blob.
        let values = if has_state { &[] } else { parameter_values };
        self.send_command_with_timeout(
            serde_json::json!({
                "type": "loadPlugin",
                "path": path.to_string_lossy(),
                "persistedState": {
                    "parameterValues": values,
                    "stateData": state_data.unwrap_or_default(),
                    "bypassed": bypassed,
                },
            }),
            "VST3 loaded into the isolated rack; audio remains under the safety limiter.",
            Duration::from_secs(30),
        )
    }

    pub fn clear_plugin(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "clearPlugin"}),
            "VST3 removed from the isolated rack; the safety path remains active.",
        )
    }

    pub fn open_plugin_editor(&self) -> Result<AudioStatus, String> {
        self.send_command_with_timeout(
            serde_json::json!({"type": "openPluginEditor"}),
            "VST3 editor opened for the active rack plugin.",
            Duration::from_secs(10),
        )
    }

    pub fn start_arrange_recording(
        &self,
        directory: &Path,
        allow_no_input: bool,
        count_in_beats: u8,
    ) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({
                "type": "startArrangeRecording",
                "directory": directory.to_string_lossy(),
                "allowNoInput": allow_no_input,
                "countInBeats": count_in_beats,
            }),
            "Arrange recording scheduled on the Native Audio Clock.",
        )
    }

    pub fn stop_arrange_recording(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "stopArrangeRecording"}),
            "Arrange recording stopped on the Native Audio Clock.",
        )
    }

    pub fn set_plugin_bypassed(&self, bypassed: bool) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "setPluginBypassed", "bypassed": bypassed}),
            if bypassed {
                "VST3 bypassed; the safety copy path remains active."
            } else {
                "VST3 processing resumed through the safety limiter."
            },
        )
    }

    pub fn set_plugin_parameter(&self, index: u32, value: f32) -> Result<AudioStatus, String> {
        if !value.is_finite() {
            return Err("Plugin parameter value must be finite.".into());
        }
        self.send_command(
            serde_json::json!({"type": "setPluginParameter", "index": index, "value": value.clamp(0.0, 1.0)}),
            "VST3 parameter updated through the isolated rack.",
        )
    }

    pub fn plugin_parameter_status(&self) -> Result<AudioStatus, String> {
        self.send_command(serde_json::json!({"type": "pluginParameterStatus"}), "")
    }

    pub fn set_master_gain_db(&self, gain_db: f64) -> Result<AudioStatus, String> {
        let safe_gain = gain_db.clamp(-90.0, 0.0);
        let status = self.send_command(
            serde_json::json!({"type": "setMasterGainDb", "gainDb": safe_gain}),
            "Master gain updated through the safety limiter.",
        )?;
        self.runtime_controls
            .lock()
            .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?
            .master_gain_db = safe_gain;
        Ok(status)
    }

    pub fn preview_master_gain_db(&self, gain_db: f64) -> Result<(), String> {
        let safe_gain = gain_db.clamp(-90.0, 0.0);
        self.send_command_ack(
            serde_json::json!({"type": "setMasterGainDb", "gainDb": safe_gain}),
            "",
            Duration::from_secs(3),
        )?;
        self.runtime_controls
            .lock()
            .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?
            .master_gain_db = safe_gain;
        Ok(())
    }

    pub fn preview_sample(
        &self,
        path: &Path,
        start_ms: u64,
        end_ms: Option<u64>,
        looped: bool,
        gain: f32,
        voice_key: Option<i32>,
    ) -> Result<AudioStatus, String> {
        let mut command = serde_json::json!({
            "type": "previewSample",
            "path": path.to_string_lossy(),
            "startMs": start_ms,
            "gain": gain.clamp(0.0, 2.0),
            "loop": looped,
        });
        if let Some(end_ms) = end_ms {
            command["endMs"] = serde_json::json!(end_ms);
        }
        if let Some(voice_key) = voice_key {
            command["voiceKey"] = serde_json::json!(voice_key);
        }
        self.send_command(
            command,
            "Sample preview queued through the safety limiter; output remains muted until unmuted.",
        )
    }

    pub fn stop_preview(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "stopPreview"}),
            "Sample preview stopped; the source file remains unchanged.",
        )
    }

    pub fn stop_preview_for_key(&self, voice_key: i32) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "stopPreviewForKey", "voiceKey": voice_key}),
            "Mapped preview voice stopped; other preview voices remain available.",
        )
    }

    pub fn start_take_comparison(
        &self,
        raw_path: &Path,
        processed_path: &Path,
        raw_start_frame: u64,
        raw_end_frame: u64,
        processed_start_frame: u64,
        processed_end_frame: u64,
    ) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({
                "type": "startTakeComparison",
                "rawPath": raw_path.to_string_lossy(),
                "processedPath": processed_path.to_string_lossy(),
                "rawStartFrame": raw_start_frame,
                "rawEndFrame": raw_end_frame,
                "processedStartFrame": processed_start_frame,
                "processedEndFrame": processed_end_frame,
            }),
            "Take comparison started with one synchronized audition voice.",
        )
    }

    pub fn switch_take_comparison_variant(
        &self,
        variant: AudioTakeVariant,
    ) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({
                "type": "switchTakeComparisonVariant",
                "variant": match variant {
                    AudioTakeVariant::Raw => "raw",
                    AudioTakeVariant::Processed => "processed",
                },
            }),
            "Take comparison variant switched without moving its audition cursor.",
        )
    }

    pub fn stop_take_comparison(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "stopTakeComparison"}),
            "Take comparison stopped.",
        )
    }

    pub fn enable_midi_listening(&self) -> Result<AudioStatus, String> {
        let status = self.send_command(
            serde_json::json!({"type": "enableMidiListening"}),
            "MIDI listening enabled; all detected inputs are routed to the rack.",
        )?;
        self.runtime_controls
            .lock()
            .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?
            .midi_listening = true;
        Ok(status)
    }

    pub fn disable_midi_listening(&self) -> Result<AudioStatus, String> {
        let status = self.send_command(
            serde_json::json!({"type": "disableMidiListening"}),
            "MIDI listening disabled; no external MIDI device is being consumed.",
        )?;
        self.runtime_controls
            .lock()
            .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?
            .midi_listening = false;
        Ok(status)
    }

    pub fn send_midi(&self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            return Err("MIDI bytes must contain at least one status byte.".into());
        }
        if bytes.len() > 3 {
            return Err(
                "MIDI bytes must contain at most three bytes (status, data1, data2).".into(),
            );
        }
        let payload_bytes: Vec<u64> = bytes.iter().map(|byte| *byte as u64).collect();
        self.send_command_ack(
            serde_json::json!({"type": "sendMidi", "bytes": payload_bytes}),
            "MIDI message enqueued for the loaded plugin.",
            Duration::from_secs(3),
        )
    }

    pub fn configure_sample_pads(&self, pads: &[NativeSamplePad]) -> Result<AudioStatus, String> {
        let pads = serde_json::to_value(pads)
            .map_err(|error| format!("Sample pad mapping could not be encoded: {error}"))?;
        self.send_command(
            serde_json::json!({"type": "configureSamplePads", "pads": pads}),
            "Sample pad mappings were prepared for MIDI-triggered audition.",
        )
    }

    pub fn recover_audio_device<R: Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> Result<AudioStatus, String> {
        let command = serde_json::json!({"type": "recoverAudioDevice"});
        let expected_generation = self.sidecar_generation();
        match self.send_command(
            command.clone(),
            "Audio device recovery requested; output remains muted until the device is ready.",
        ) {
            Ok(status) => Ok(status),
            Err(error) if sidecar_restart_required(&error) => {
                self.restart_sidecar(
                    app,
                    "Native audio sidecar is restarting in emergency-mute state.",
                    expected_generation,
                )?;
                self.send_command(
                    command,
                    "Audio sidecar restarted and device recovery was requested; output remains muted until the device is ready.",
                )
            }
            Err(error) => Err(error),
        }
    }

    pub fn set_audio_driver<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        config: &crate::audio_preferences::AudioDriverConfig,
    ) -> Result<AudioStatus, String> {
        let mut command = serde_json::json!({"type": "setAudioDriver", "driver": config.driver});
        if let Some(input_device) = config.input_device.as_deref() {
            command["inputDevice"] = serde_json::json!(input_device);
        }
        command["inputChannel"] = serde_json::json!(config.input_channel);
        if let Some(output_device) = config.output_device.as_deref() {
            command["outputDevice"] = serde_json::json!(output_device);
        }
        if let Some(sample_rate) = config.sample_rate {
            command["sampleRate"] = serde_json::json!(sample_rate);
        }
        if let Some(buffer_size) = config.buffer_size {
            command["bufferSize"] = serde_json::json!(buffer_size);
        }
        let expected_generation = self.sidecar_generation();
        match self.send_command(
            command,
            "Audio driver switch requested; output remains muted until the new device is ready.",
        ) {
            Ok(status) => Ok(status),
            Err(error) if sidecar_restart_required(&error) => {
                self.restart_sidecar(
                    app,
                    "The audio driver switch stalled; the isolated engine is restarting with the previous device.",
                    expected_generation,
                )?;
                let mut status = self.refresh_status()?;
                status.message = format!(
                    "The requested audio driver did not respond, so the previous device was restored: {error}"
                );
                if let Ok(mut current) = self.status.lock() {
                    current.message = status.message.clone();
                }
                Ok(status)
            }
            Err(error) => Err(error),
        }
    }

    pub fn set_restart_preferences(&self, preferences: AudioPreferences) -> Result<(), String> {
        *self
            .restart_preferences
            .lock()
            .map_err(|error| format!("Audio preference lock was poisoned: {error}"))? = preferences;
        Ok(())
    }

    pub fn set_emergency_mute(&self, muted: bool) -> Result<AudioStatus, String> {
        let status = self.send_command(
            serde_json::json!({"type": "setEmergencyMute", "muted": muted}),
            if muted {
                "Emergency mute is engaged; saved and recorded data is unaffected."
            } else {
                "Audio faded in from silence through the safety limiter."
            },
        )?;
        self.runtime_controls
            .lock()
            .map_err(|error| format!("Runtime control lock was poisoned: {error}"))?
            .emergency_muted = muted;
        Ok(status)
    }
}

impl Drop for AudioSupervisor {
    fn drop(&mut self) {
        if Arc::strong_count(&self.child) != 1 {
            return;
        }
        self.shutting_down.store(true, Ordering::Release);
        if let Ok(mut slot) = self.child.lock()
            && let Some(child) = slot.take()
        {
            self.mark_planned_termination(self.sidecar_generation());
            // Drop is a hard safety boundary. A graceful shutdown write can
            // block behind the same VST that caused the app to close, so kill
            // the isolated process directly and let the OS reclaim it.
            let _ = child.kill();
        }
    }
}

/// Maps a deserialized native status line onto the in-app `AudioStatus` without
/// touching any shared state, so the field mapping and safety clamping are
/// unit-testable in isolation.
fn native_status_to_audio_status(native: NativeStatus) -> AudioStatus {
    let state = match native.state.as_str() {
        "ready" => AudioState::Ready,
        "muted" => AudioState::Muted,
        "starting" => AudioState::Starting,
        "faulted" => AudioState::Faulted,
        _ => AudioState::Offline,
    };
    let fallback_message = match state {
        AudioState::Ready => "Native audio is ready through the safety chain.".into(),
        AudioState::Muted => "Native audio is connected and emergency-muted.".into(),
        AudioState::Starting => "Native audio is starting safely.".into(),
        AudioState::Faulted => "Native audio reported a fault; saved data is safe.".into(),
        AudioState::Offline => "Native audio is offline; saved data is safe.".into(),
    };
    let message = native
        .message
        .filter(|m| !m.is_empty())
        .unwrap_or(fallback_message);
    AudioStatus {
        state,
        driver: native.driver,
        input_device: native.input_device,
        input_channel: native.input_channel,
        input_channels: native
            .input_channels
            .unwrap_or_default()
            .into_iter()
            .map(|channel| AudioChannelInfo {
                index: channel.index,
                name: channel.name,
            })
            .collect(),
        output_device: native.output_device,
        output_channels: native
            .output_channels
            .unwrap_or_default()
            .into_iter()
            .map(|channel| AudioChannelInfo {
                index: channel.index,
                name: channel.name,
            })
            .collect(),
        sample_rate: native.sample_rate.and_then(normalize_sample_rate),
        buffer_size: native.buffer_size,
        round_trip_ms: native.round_trip_ms,
        timeline_tick: native.timeline_tick,
        recording: native
            .recording
            .map(|recording| RecordingStatus {
                active: recording.active,
                cancelled: recording.cancelled,
                directory: recording.directory,
                sample_rate: recording.sample_rate.and_then(normalize_sample_rate),
                raw_channels: recording.raw_channels,
                processed_channels: recording.processed_channels,
                samples_written: recording.samples_written.unwrap_or_default(),
                dropped_blocks: recording.dropped_blocks.unwrap_or_default(),
                missing_samples: recording.missing_samples.unwrap_or_default(),
                dropout_start_sample: recording.dropout_start_sample,
                dropout_end_sample: recording.dropout_end_sample,
                raw_attempted_samples: recording.raw_attempted_samples.unwrap_or_default(),
                processed_attempted_samples: recording
                    .processed_attempted_samples
                    .unwrap_or_default(),
                raw_dropped_blocks: recording.raw_dropped_blocks.unwrap_or_default(),
                processed_dropped_blocks: recording.processed_dropped_blocks.unwrap_or_default(),
                raw_missing_samples: recording.raw_missing_samples.unwrap_or_default(),
                processed_missing_samples: recording.processed_missing_samples.unwrap_or_default(),
                raw_dropout_start_sample: recording.raw_dropout_start_sample,
                raw_dropout_end_sample: recording.raw_dropout_end_sample,
                processed_dropout_start_sample: recording.processed_dropout_start_sample,
                processed_dropout_end_sample: recording.processed_dropout_end_sample,
                recovery_status: recording.recovery_status.unwrap_or_else(|| {
                    if recording.dropped_blocks.unwrap_or_default() == 0 {
                        "clean".into()
                    } else {
                        "partial".into()
                    }
                }),
            })
            .unwrap_or_default(),
        plugin: native.plugin.map(|plugin| PluginStatus {
            loaded: plugin.loaded,
            bypassed: plugin.bypassed,
            path: plugin.path.filter(|path| !path.is_empty()),
            name: plugin.name.filter(|name| !name.is_empty()),
            sample_rate: plugin.sample_rate.and_then(normalize_sample_rate),
            block_size: plugin.block_size,
            input_channels: plugin.input_channels.unwrap_or_default(),
            output_channels: plugin.output_channels.unwrap_or_default(),
            bypassed_blocks: plugin.bypassed_blocks.unwrap_or_default(),
            processed_blocks: plugin.processed_blocks.unwrap_or_default(),
            contention_blocks: plugin.contention_blocks.unwrap_or_default(),
            transition_blocks: plugin.transition_blocks.unwrap_or_default(),
            parameters: plugin
                .parameters
                .unwrap_or_default()
                .into_iter()
                .map(|parameter| PluginParameter {
                    index: parameter.index,
                    name: parameter.name,
                    value: if parameter.value.is_finite() {
                        parameter.value.clamp(0.0, 1.0)
                    } else {
                        0.0
                    },
                    default_value: if parameter.default_value.is_finite() {
                        parameter.default_value.clamp(0.0, 1.0)
                    } else {
                        0.0
                    },
                    automatable: parameter.automatable,
                })
                .collect(),
            state_data: plugin.state_data.filter(|state| state.len() <= 4_000_000),
        }),
        midi_inputs: native.midi_inputs.unwrap_or_default(),
        midi_outputs: native.midi_outputs.unwrap_or_default(),
        midi_input_active: native.midi_input_active.unwrap_or(false),
        midi_messages: native.midi_messages.unwrap_or_default(),
        last_midi_note: native
            .last_midi_note
            .and_then(|note| u8::try_from(note).ok()),
        midi_pad_mappings: native.midi_pad_mappings.unwrap_or_default(),
        midi_pad_triggers: native.midi_pad_triggers.unwrap_or_default(),
        input_peak: native.input_peak.unwrap_or_default().clamp(0.0, 1.0),
        output_peak: native.output_peak.unwrap_or_default().clamp(0.0, 1.0),
        invalid_samples: native.invalid_samples.unwrap_or_default(),
        feedback_suspected: native.feedback_suspected.unwrap_or(false),
        message,
    }
}

/// One parsed sidecar line: a status update, or an error classified by scope.
/// Parsing is pure; applying the effect to shared state happens in
/// `handle_native_stdout`, so the protocol is reproducible without a live child.
#[allow(clippy::large_enum_variant)]
enum ParsedNativeLine {
    Status {
        request_id: Option<u64>,
        status: NativeStatus,
    },
    Meters {
        request_id: Option<u64>,
        meters: NativeMeters,
    },
    Acknowledgement {
        request_id: Option<u64>,
    },
    Error {
        request_id: Option<u64>,
        fault: bool,
        detail: String,
    },
}

/// Classifies a native error into a user-facing message and whether it should
/// fault the audio engine (device errors) or only report a command failure.
fn render_native_error(scope: &str, message: &str) -> (bool, String) {
    if scope == "audioDevice" {
        let detail = format!("Native audio device error: {message}. Saved data remains safe.");
        (true, detail)
    } else {
        let detail =
            format!("Native {scope} command failed: {message}. Audio and saved data remain safe.");
        (false, detail)
    }
}

/// Parses one JSON line from the sidecar into a typed reply. Returns `None` for
/// non-JSON or unrecognized message types so the caller can ignore them.
fn parse_native_line(bytes: &[u8]) -> Option<ParsedNativeLine> {
    let payload = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    let request_id = payload.get("requestId").and_then(serde_json::Value::as_u64);
    match payload.get("type").and_then(serde_json::Value::as_str) {
        Some("audioStatus") => {
            let status = serde_json::from_value::<NativeStatus>(payload).ok()?;
            Some(ParsedNativeLine::Status { request_id, status })
        }
        Some("audioMeters") => {
            let meters = serde_json::from_value::<NativeMeters>(payload).ok()?;
            Some(ParsedNativeLine::Meters { request_id, meters })
        }
        Some("transportStatus" | "timelineAck") => {
            Some(ParsedNativeLine::Acknowledgement { request_id })
        }
        Some("error") => {
            let scope = payload
                .get("scope")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("protocol");
            let message = payload
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Unknown native error.");
            let (fault, detail) = render_native_error(scope, message);
            Some(ParsedNativeLine::Error {
                request_id,
                fault,
                detail,
            })
        }
        _ => None,
    }
}

fn handle_native_stdout(status: &Arc<Mutex<AudioStatus>>, bytes: &[u8]) -> Option<NativeReply> {
    let parsed = parse_native_line(bytes)?;
    match parsed {
        ParsedNativeLine::Status {
            request_id,
            status: native_status,
        } => {
            if let Ok(mut current) = status.lock() {
                *current = native_status_to_audio_status(native_status);
            }
            Some(NativeReply {
                request_id,
                result: Ok(()),
                event: NativeEvent::AudioStatus,
            })
        }
        ParsedNativeLine::Meters { request_id, meters } => {
            if let Ok(mut current) = status.lock() {
                current.input_peak = meters.input_peak.unwrap_or_default().clamp(0.0, 1.0);
                current.output_peak = meters.output_peak.unwrap_or_default().clamp(0.0, 1.0);
                current.invalid_samples = meters.invalid_samples.unwrap_or_default();
                current.feedback_suspected = meters.feedback_suspected.unwrap_or(false);
            }
            Some(NativeReply {
                request_id,
                result: Ok(()),
                event: NativeEvent::AudioMeters,
            })
        }
        ParsedNativeLine::Acknowledgement { request_id } => Some(NativeReply {
            request_id,
            result: Ok(()),
            event: NativeEvent::None,
        }),
        ParsedNativeLine::Error {
            request_id,
            fault,
            detail,
        } => {
            if fault {
                set_faulted(status, detail.clone());
            } else {
                set_command_error(status, detail.clone());
            }
            Some(NativeReply {
                request_id,
                result: Err(detail),
                event: NativeEvent::AudioStatus,
            })
        }
    }
}

fn record_command_response(
    responses: &Arc<(Mutex<CommandResponse>, Condvar)>,
    request_id: u64,
    error: Option<String>,
) {
    let (response_lock, response_ready) = &**responses;
    if let Ok(mut response) = response_lock.lock() {
        if let Some(result) = response.results.get_mut(&request_id) {
            *result = Some(match error {
                Some(error) => Err(error),
                None => Ok(()),
            });
        }
        response_ready.notify_all();
    }
}

fn mark_sidecar_terminated(
    terminated_generations: &Arc<(Mutex<HashSet<u64>>, Condvar)>,
    generation: u64,
) {
    let (terminated, changed) = &**terminated_generations;
    if let Ok(mut generations) = terminated.lock() {
        generations.insert(generation);
        changed.notify_all();
    }
}

fn fail_pending_requests(responses: &Arc<(Mutex<CommandResponse>, Condvar)>, error: String) {
    let (response_lock, response_ready) = &**responses;
    if let Ok(mut response) = response_lock.lock() {
        for result in response.results.values_mut() {
            if result.is_none() {
                *result = Some(Err(error.clone()));
            }
        }
        response_ready.notify_all();
    }
}

fn set_command_error(status: &Arc<Mutex<AudioStatus>>, message: String) {
    if let Ok(mut current) = status.lock() {
        current.message = message;
    }
}

fn set_starting(status: &Arc<Mutex<AudioStatus>>, message: &str) {
    if let Ok(mut current) = status.lock() {
        current.state = AudioState::Starting;
        current.message = message.into();
    }
}

fn set_faulted(status: &Arc<Mutex<AudioStatus>>, message: String) {
    if let Ok(mut current) = status.lock() {
        current.state = AudioState::Faulted;
        current.message = message;
    }
}

pub(crate) fn is_transport_loss_error(error: &str) -> bool {
    error.contains(NATIVE_AUDIO_TRANSPORT_LOST)
        || error.contains("could not reach the isolated audio process")
        || error.contains("Native audio is unavailable")
        || error.contains("did not acknowledge the command")
        || error.contains("did not acknowledge the graph publish")
}

fn sidecar_restart_required(error: &str) -> bool {
    is_transport_loss_error(error)
}

/// Emits the current AudioStatus to the frontend via the Tauri event bus so
/// React receives status changes as they happen instead of polling. The emit
/// is best-effort: if the frontend listener is gone (app shutting down) the
/// error is silently ignored because there is nothing to recover into.
fn emit_audio_status<R: Runtime>(app: &AppHandle<R>, status: &Arc<Mutex<AudioStatus>>) {
    if let Ok(current) = status.lock() {
        let _ = app.emit("audio-status", &*current);
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioMeters {
    input_peak: f64,
    output_peak: f64,
    invalid_samples: u64,
    feedback_suspected: bool,
}

fn emit_audio_meters<R: Runtime>(app: &AppHandle<R>, status: &Arc<Mutex<AudioStatus>>) {
    if let Ok(current) = status.lock() {
        let meters = AudioMeters {
            input_peak: current.input_peak,
            output_peak: current.output_peak,
            invalid_samples: current.invalid_samples,
            feedback_suspected: current.feedback_suspected,
        };
        let _ = app.emit("audio-meters", meters);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_status() -> Arc<Mutex<AudioStatus>> {
        Arc::new(Mutex::new(AudioStatus {
            state: AudioState::Ready,
            driver: Some("Test".into()),
            input_device: Some("Input".into()),
            input_channel: Some(0),
            input_channels: vec![AudioChannelInfo {
                index: 0,
                name: "Input 1".into(),
            }],
            output_device: Some("Output".into()),
            output_channels: vec![AudioChannelInfo {
                index: 0,
                name: "Output 1".into(),
            }],
            sample_rate: Some(44_100),
            buffer_size: Some(441),
            round_trip_ms: Some(20.0),
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
            message: "ready".into(),
        }))
    }

    #[test]
    fn plugin_error_preserves_audio_state() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"error","scope":"plugin","message":"load failed","dataSafe":true}"#,
        );
        let current = status.lock().unwrap();
        assert!(matches!(current.state, AudioState::Ready));
        assert!(current.message.contains("plugin"));
    }

    #[test]
    fn audio_device_error_faults_audio_state() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"error","scope":"audioDevice","message":"device missing","dataSafe":true}"#,
        );
        let current = status.lock().unwrap();
        assert!(matches!(current.state, AudioState::Faulted));
        assert!(current.message.contains("device missing"));
    }

    #[test]
    fn midi_status_updates_without_affecting_audio_state() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"audioStatus","state":"ready","midiInputActive":true,"midiMessages":12,"lastMidiNote":60,"inputPeak":0.2,"outputPeak":0.3}"#,
        );
        let current = status.lock().unwrap();
        assert!(matches!(current.state, AudioState::Ready));
        assert!(current.midi_input_active);
        assert_eq!(current.midi_messages, 12);
        assert_eq!(current.last_midi_note, Some(60));
        assert_eq!(current.output_peak, 0.3);
    }

    #[test]
    fn normalizes_native_floating_sample_rates_safely() {
        assert_eq!(normalize_sample_rate(44_100.0), Some(44_100));
        assert_eq!(normalize_sample_rate(f64::NAN), None);
        assert_eq!(normalize_sample_rate(f64::INFINITY), None);
    }

    #[test]
    fn preserves_request_ids_for_command_acknowledgements() {
        let status = test_status();
        let success = handle_native_stdout(
            &status,
            br#"{"type":"audioStatus","requestId":42,"state":"ready"}"#,
        )
        .expect("status reply");
        assert_eq!(success.request_id, Some(42));
        assert!(success.result.is_ok());

        let failure = handle_native_stdout(
            &status,
            br#"{"type":"error","requestId":43,"scope":"recording","message":"no input"}"#,
        )
        .expect("error reply");
        assert_eq!(failure.request_id, Some(43));
        assert!(failure.result.is_err());
    }

    #[test]
    fn parses_status_reply_with_request_id() {
        let parsed = parse_native_line(
            br#"{"type":"audioStatus","requestId":7,"state":"ready","midiInputActive":true}"#,
        )
        .expect("status line");
        match parsed {
            ParsedNativeLine::Status { request_id, status } => {
                assert_eq!(request_id, Some(7));
                assert_eq!(status.state, "ready");
                assert_eq!(status.midi_input_active, Some(true));
            }
            ParsedNativeLine::Meters { .. }
            | ParsedNativeLine::Acknowledgement { .. }
            | ParsedNativeLine::Error { .. } => {
                panic!("expected a status line")
            }
        }
    }

    #[test]
    fn classifies_audio_device_errors_as_faults() {
        let (fault, detail) = render_native_error("audioDevice", "device missing");
        assert!(fault);
        assert!(detail.contains("device missing"));
    }

    #[test]
    fn classifies_other_errors_as_command_failures() {
        let (fault, detail) = render_native_error("plugin", "load failed");
        assert!(!fault);
        assert!(detail.contains("plugin"));
    }

    #[test]
    fn identifies_lost_sidecar_transport_for_recovery() {
        assert!(sidecar_restart_required(
            "Native audio transport lost: process stopped (code Some(0))."
        ));
        assert!(sidecar_restart_required(
            "Audio command could not reach the isolated audio process: pipe closed."
        ));
        assert!(sidecar_restart_required(
            "Native audio did not acknowledge the command within 3 seconds."
        ));
        assert!(!sidecar_restart_required(
            "Native audio device error: device missing."
        ));
    }

    #[test]
    fn restart_coordinator_reuses_the_result_for_a_stale_generation() {
        let supervisor = AudioSupervisor::offline("test");
        let result = Err("restart failed".to_string());
        supervisor.record_restart_outcome(7, &result);

        assert_eq!(supervisor.completed_restart_outcome(7), Some(result));
        assert!(supervisor.completed_restart_outcome(8).is_none());
    }

    #[test]
    fn planned_termination_is_consumed_once() {
        let supervisor = AudioSupervisor::offline("test");
        supervisor.mark_planned_termination(3);

        assert!(supervisor.take_planned_termination(3));
        assert!(!supervisor.take_planned_termination(3));
    }

    #[test]
    fn sidecar_termination_completes_the_pending_command() {
        let responses = Arc::new((Mutex::new(CommandResponse::default()), Condvar::new()));
        responses.0.lock().unwrap().results.insert(42, None);

        fail_pending_requests(&responses, "plugin process stopped".into());

        let response = responses.0.lock().unwrap();
        assert!(matches!(
            response.results.get(&42),
            Some(Some(Err(message))) if message == "plugin process stopped"
        ));
    }

    #[test]
    fn sidecar_termination_completes_all_pending_commands() {
        let responses = Arc::new((Mutex::new(CommandResponse::default()), Condvar::new()));
        responses
            .0
            .lock()
            .unwrap()
            .results
            .extend([(7, None), (8, None)]);

        fail_pending_requests(&responses, "plugin process stopped".into());

        let response = responses.0.lock().unwrap();
        assert!(matches!(
            response.results.get(&7),
            Some(Some(Err(message))) if message == "plugin process stopped"
        ));
        assert!(matches!(
            response.results.get(&8),
            Some(Some(Err(message))) if message == "plugin process stopped"
        ));
    }

    #[test]
    fn error_reply_without_scope_defaults_to_protocol() {
        let parsed = parse_native_line(br#"{"type":"error","requestId":9,"message":"no input"}"#)
            .expect("error line");
        match parsed {
            ParsedNativeLine::Error {
                request_id, fault, ..
            } => {
                assert_eq!(request_id, Some(9));
                assert!(!fault);
            }
            ParsedNativeLine::Status { .. }
            | ParsedNativeLine::Meters { .. }
            | ParsedNativeLine::Acknowledgement { .. } => {
                panic!("expected an error line")
            }
        }
    }

    #[test]
    fn meter_reply_updates_only_meter_fields() {
        let status = test_status();
        let reply = handle_native_stdout(
            &status,
            br#"{"type":"audioMeters","requestId":12,"inputPeak":0.7,"outputPeak":0.4,"invalidSamples":3,"feedbackSuspected":true}"#,
        )
        .expect("meter reply");
        let current = status.lock().unwrap();
        assert_eq!(reply.request_id, Some(12));
        assert!(matches!(reply.event, NativeEvent::AudioMeters));
        assert_eq!(current.driver.as_deref(), Some("Test"));
        assert_eq!(current.input_peak, 0.7);
        assert_eq!(current.output_peak, 0.4);
        assert_eq!(current.invalid_samples, 3);
        assert!(current.feedback_suspected);
    }

    #[test]
    fn ignores_non_json_and_unrecognized_lines() {
        assert!(parse_native_line(b"not json").is_none());
        assert!(parse_native_line(br#"{"type":"keepAlive"}"#).is_none());
    }

    #[test]
    fn maps_unknown_state_to_offline_and_clamps_peaks() {
        let native: NativeStatus = serde_json::from_value(serde_json::json!({
            "state": "bogus",
            "inputPeak": 5.0,
            "outputPeak": -1.0,
        }))
        .expect("native status");
        let status = native_status_to_audio_status(native);
        assert!(matches!(status.state, AudioState::Offline));
        assert_eq!(status.input_peak, 1.0);
        assert_eq!(status.output_peak, 0.0);
        assert!(status.message.contains("offline"));
    }

    #[test]
    fn device_disconnect_status_reports_faulted_state() {
        let native: NativeStatus = serde_json::from_value(serde_json::json!({
            "state": "faulted",
            "message": "Audio device disconnected; output is muted and any captured take is preserved."
        }))
        .expect("native status");
        let status = native_status_to_audio_status(native);
        assert!(matches!(status.state, AudioState::Faulted));
        assert!(status.message.contains("device disconnected"));
    }

    #[test]
    fn maps_audio_status_onto_pure_audio_status() {
        let native: NativeStatus = serde_json::from_value(serde_json::json!({
            "state": "muted",
            "driver": "ASIO",
            "inputChannel": 1,
            "inputChannels": [
                { "index": 0, "name": "Analogue 1" },
                { "index": 1, "name": "Analogue 2" }
            ],
            "outputChannels": [
                { "index": 0, "name": "Monitor 1" },
                { "index": 1, "name": "Monitor 2" }
            ],
            "sampleRate": 48000.0,
            "bufferSize": 256,
            "recording": { "active": true, "directory": "/tmp", "samplesWritten": 10 },
            "plugin": { "loaded": true, "bypassed": false, "path": "v.st3", "name": "V", "contentionBlocks": 3, "parameters": [] }
        }))
        .expect("native status");
        let status = native_status_to_audio_status(native);
        assert!(matches!(status.state, AudioState::Muted));
        assert_eq!(status.driver.as_deref(), Some("ASIO"));
        assert_eq!(status.sample_rate, Some(48_000));
        assert_eq!(status.input_channel, Some(1));
        assert_eq!(status.input_channels[1].name, "Analogue 2");
        assert_eq!(status.output_channels.len(), 2);
        assert!(status.recording.active);
        assert_eq!(status.recording.samples_written, 10);
        assert_eq!(
            status.plugin.as_ref().unwrap().path.as_deref(),
            Some("v.st3")
        );
        assert_eq!(status.plugin.as_ref().unwrap().contention_blocks, 3);
        assert!(status.message.contains("emergency-muted"));
        assert!(!status.feedback_suspected);
    }
}
