use crate::audio_preferences::AudioPreferences;
use crate::model::{
    AudioChannelInfo, AudioState, AudioStatus, PluginParameter, PluginStatus, RecordingStatus,
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Runtime};
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};

pub struct AudioSupervisor {
    status: Arc<Mutex<AudioStatus>>,
    responses: Arc<(Mutex<CommandResponse>, Condvar)>,
    next_request_id: AtomicU64,
    sidecar_generation: Arc<AtomicU64>,
    pending_request_id: Arc<AtomicU64>,
    child: Mutex<Option<CommandChild>>,
    restart_preferences: Mutex<AudioPreferences>,
}

#[derive(Default)]
struct CommandResponse {
    request_id: Option<u64>,
    error: Option<String>,
}

struct NativeReply {
    request_id: Option<u64>,
    result: Result<(), String>,
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
    recording: Option<NativeRecordingStatus>,
    plugin: Option<NativePluginStatus>,
    midi_inputs: Option<Vec<String>>,
    midi_outputs: Option<Vec<String>>,
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
    directory: Option<String>,
    sample_rate: Option<f64>,
    raw_channels: Option<u32>,
    processed_channels: Option<u32>,
    samples_written: Option<u64>,
    dropped_blocks: Option<u64>,
    missing_samples: Option<u64>,
    dropout_start_sample: Option<u64>,
    dropout_end_sample: Option<u64>,
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
            responses: Arc::new((Mutex::new(CommandResponse::default()), Condvar::new())),
            next_request_id: AtomicU64::new(1),
            sidecar_generation: Arc::new(AtomicU64::new(0)),
            pending_request_id: Arc::new(AtomicU64::new(0)),
            child: Mutex::new(None),
            restart_preferences: Mutex::new(AudioPreferences::default()),
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
            status,
            responses,
            next_request_id: AtomicU64::new(1),
            sidecar_generation: Arc::new(AtomicU64::new(0)),
            pending_request_id: Arc::new(AtomicU64::new(0)),
            child: Mutex::new(None),
            restart_preferences: Mutex::new(preferences),
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
        let event_pending_request_id = Arc::clone(&self.pending_request_id);
        tauri::async_runtime::spawn(async move {
            while let Some(event) = receiver.recv().await {
                if event_generation.load(Ordering::Acquire) != generation {
                    break;
                }
                match event {
                    CommandEvent::Stdout(bytes) => {
                        if let Some(response) = handle_native_stdout(&event_status, &bytes)
                            && let Some(request_id) = response.request_id
                        {
                            record_command_response(
                                &event_responses,
                                request_id,
                                response.result.err(),
                            );
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
                    }
                    CommandEvent::Error(error) => {
                        let message = format!(
                            "Native audio communication failed: {error}. The engine is isolated and saved data is safe."
                        );
                        set_faulted(&event_status, message.clone());
                        fail_pending_request(&event_responses, &event_pending_request_id, message);
                    }
                    CommandEvent::Terminated(payload) => {
                        let message = format!(
                            "Native audio process stopped (code {:?}); the UI and saved session remain available.",
                            payload.code
                        );
                        set_faulted(&event_status, message.clone());
                        fail_pending_request(&event_responses, &event_pending_request_id, message);
                    }
                    _ => {}
                }
            }
        });
        Ok(child)
    }

    fn restart_sidecar<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        starting_message: &str,
    ) -> Result<(), String> {
        let generation = self.next_sidecar_generation();
        let mut slot = self
            .child
            .lock()
            .map_err(|error| format!("Audio child lock was poisoned: {error}"))?;
        if let Some(child) = slot.take() {
            let _ = child.kill();
        }
        drop(slot);
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
        *self
            .child
            .lock()
            .map_err(|error| format!("Audio child lock was poisoned: {error}"))? = Some(child);
        Ok(())
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

    fn send_command(
        &self,
        command: serde_json::Value,
        message: &str,
    ) -> Result<AudioStatus, String> {
        self.send_command_with_timeout(command, message, Duration::from_secs(3))
    }

    fn send_command_with_timeout(
        &self,
        mut command: serde_json::Value,
        message: &str,
        timeout: Duration,
    ) -> Result<AudioStatus, String> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        command["requestId"] = serde_json::json!(request_id);
        let payload = serde_json::to_string(&command)
            .map_err(|error| format!("Audio command could not be encoded: {error}"))?;
        let mut child_slot = self
            .child
            .lock()
            .map_err(|error| format!("Audio child lock was poisoned: {error}"))?;
        let child = child_slot.as_mut().ok_or_else(|| {
            "Native audio is unavailable; the requested audio command was not sent.".to_string()
        })?;
        let (response_lock, response_ready) = &*self.responses;
        let response = response_lock
            .lock()
            .map_err(|error| format!("Audio response lock was poisoned: {error}"))?;
        self.pending_request_id.store(request_id, Ordering::Release);
        if let Err(error) = child.write(format!("{payload}\n").as_bytes()) {
            let _ = self.pending_request_id.compare_exchange(
                request_id,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return Err(format!(
                "Audio command could not reach the isolated audio process: {error}"
            ));
        }
        let wait = response_ready.wait_timeout_while(response, timeout, |current| {
            current.request_id != Some(request_id)
        });
        let _ = self.pending_request_id.compare_exchange(
            request_id,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        let (response, wait_result) =
            wait.map_err(|error| format!("Audio response wait failed: {error}"))?;
        if wait_result.timed_out() && response.request_id != Some(request_id) {
            return Err(format!(
                "Native audio did not acknowledge the command within {} seconds.",
                timeout.as_secs()
            ));
        }
        if let Some(error) = response.error.clone() {
            return Err(error);
        }
        let mut status = self
            .status
            .lock()
            .map_err(|error| format!("Audio status lock was poisoned: {error}"))?;
        if !message.is_empty() {
            status.message = message.into();
        }
        Ok(status.clone())
    }

    pub fn load_plugin(&self, path: &Path) -> Result<AudioStatus, String> {
        self.send_command_with_timeout(
            serde_json::json!({"type": "loadPlugin", "path": path.to_string_lossy()}),
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

    pub fn start_recording(&self, directory: &Path) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "startRecording", "directory": directory.to_string_lossy()}),
            "Recording started; Raw and Processed files are being flushed safely.",
        )
    }

    pub fn stop_recording(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "stopRecording"}),
            "Recording stopped; Raw and Processed files are finalized.",
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

    pub fn set_plugin_state(&self, state_data: &str) -> Result<AudioStatus, String> {
        if state_data.len() > 4_000_000 {
            return Err("VST3 state data exceeds the safe 4 MiB limit.".into());
        }
        self.send_command(
            serde_json::json!({"type": "setPluginState", "stateData": state_data}),
            "VST3 state restored through the isolated rack.",
        )
    }

    pub fn plugin_parameter_status(&self) -> Result<AudioStatus, String> {
        self.send_command(serde_json::json!({"type": "pluginParameterStatus"}), "")
    }

    pub fn set_master_gain_db(&self, gain_db: f64) -> Result<AudioStatus, String> {
        let safe_gain = gain_db.clamp(-90.0, 0.0);
        self.send_command(
            serde_json::json!({"type": "setMasterGainDb", "gainDb": safe_gain}),
            "Master gain updated through the safety limiter.",
        )
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

    pub fn enable_midi_listening(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "enableMidiListening"}),
            "MIDI listening enabled; all detected inputs are routed to the rack.",
        )
    }

    pub fn disable_midi_listening(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "disableMidiListening"}),
            "MIDI listening disabled; no external MIDI device is being consumed.",
        )
    }

    pub fn send_midi(&self, bytes: &[u8]) -> Result<AudioStatus, String> {
        if bytes.is_empty() {
            return Err("MIDI bytes must contain at least one status byte.".into());
        }
        if bytes.len() > 3 {
            return Err(
                "MIDI bytes must contain at most three bytes (status, data1, data2).".into(),
            );
        }
        let payload_bytes: Vec<u64> = bytes.iter().map(|byte| *byte as u64).collect();
        self.send_command(
            serde_json::json!({"type": "sendMidi", "bytes": payload_bytes}),
            "MIDI message enqueued for the loaded plugin.",
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
        match self.send_command(
            command.clone(),
            "Audio device recovery requested; output remains muted until the device is ready.",
        ) {
            Ok(status) => Ok(status),
            Err(error) if sidecar_restart_required(&error) => {
                self.restart_sidecar(
                    app,
                    "Native audio sidecar is restarting in emergency-mute state.",
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
        driver: &str,
        input_device: Option<&str>,
        input_channel: u32,
        output_device: Option<&str>,
        sample_rate: Option<u32>,
        buffer_size: Option<u32>,
    ) -> Result<AudioStatus, String> {
        let mut command = serde_json::json!({"type": "setAudioDriver", "driver": driver});
        if let Some(input_device) = input_device {
            command["inputDevice"] = serde_json::json!(input_device);
        }
        command["inputChannel"] = serde_json::json!(input_channel);
        if let Some(output_device) = output_device {
            command["outputDevice"] = serde_json::json!(output_device);
        }
        if let Some(sample_rate) = sample_rate {
            command["sampleRate"] = serde_json::json!(sample_rate);
        }
        if let Some(buffer_size) = buffer_size {
            command["bufferSize"] = serde_json::json!(buffer_size);
        }
        match self.send_command(
            command,
            "Audio driver switch requested; output remains muted until the new device is ready.",
        ) {
            Ok(status) => Ok(status),
            Err(error) if sidecar_restart_required(&error) => {
                self.restart_sidecar(
                    app,
                    "The audio driver switch stalled; the isolated engine is restarting with the previous device.",
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
        self.send_command(
            serde_json::json!({"type": "setEmergencyMute", "muted": muted}),
            if muted {
                "Emergency mute is engaged; saved and recorded data is unaffected."
            } else {
                "Audio faded in from silence through the safety limiter."
            },
        )
    }
}

impl Drop for AudioSupervisor {
    fn drop(&mut self) {
        if let Ok(slot) = self.child.get_mut()
            && let Some(mut child) = slot.take()
        {
            let _ = child.write(b"{\"type\":\"shutdown\"}\n");
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
        recording: native
            .recording
            .map(|recording| RecordingStatus {
                active: recording.active,
                directory: recording.directory,
                sample_rate: recording.sample_rate.and_then(normalize_sample_rate),
                raw_channels: recording.raw_channels,
                processed_channels: recording.processed_channels,
                samples_written: recording.samples_written.unwrap_or_default(),
                dropped_blocks: recording.dropped_blocks.unwrap_or_default(),
                missing_samples: recording.missing_samples.unwrap_or_default(),
                dropout_start_sample: recording.dropout_start_sample,
                dropout_end_sample: recording.dropout_end_sample,
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
            })
        }
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
        response.request_id = Some(request_id);
        response.error = error;
        response_ready.notify_all();
    }
}

fn fail_pending_request(
    responses: &Arc<(Mutex<CommandResponse>, Condvar)>,
    pending_request_id: &AtomicU64,
    error: String,
) {
    let request_id = pending_request_id.swap(0, Ordering::AcqRel);
    if request_id != 0 {
        record_command_response(responses, request_id, Some(error));
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

fn sidecar_restart_required(error: &str) -> bool {
    error.contains("could not reach the isolated audio process")
        || error.contains("Native audio is unavailable")
        || error.contains("did not acknowledge the command")
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
            ParsedNativeLine::Meters { .. } | ParsedNativeLine::Error { .. } => {
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
    fn sidecar_termination_completes_the_pending_command() {
        let responses = Arc::new((Mutex::new(CommandResponse::default()), Condvar::new()));
        let pending = AtomicU64::new(42);

        fail_pending_request(&responses, &pending, "plugin process stopped".into());

        let response = responses.0.lock().unwrap();
        assert_eq!(pending.load(Ordering::Acquire), 0);
        assert_eq!(response.request_id, Some(42));
        assert_eq!(response.error.as_deref(), Some("plugin process stopped"));
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
            ParsedNativeLine::Status { .. } | ParsedNativeLine::Meters { .. } => {
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
