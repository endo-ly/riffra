use crate::model::{
    AudioState, AudioStatus, PluginParameter, PluginStatus, RecordingStatus, SamplePad,
};
use serde::Deserialize;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Runtime};
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};

pub struct AudioSupervisor {
    status: Arc<Mutex<AudioStatus>>,
    child: Mutex<Option<CommandChild>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeStatus {
    state: String,
    driver: Option<String>,
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
    bypassed_blocks: Option<u64>,
    parameters: Option<Vec<NativePluginParameter>>,
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

impl AudioSupervisor {
    pub fn offline(message: impl Into<String>) -> Self {
        Self {
            status: Arc::new(Mutex::new(AudioStatus {
                state: AudioState::Offline,
                driver: None,
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
                message: message.into(),
            })),
            child: Mutex::new(None),
        }
    }

    pub fn start<R: Runtime>(app: &AppHandle<R>) -> Self {
        let status = Arc::new(Mutex::new(AudioStatus {
            state: AudioState::Starting,
            driver: None,
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
            message: "Native audio sidecar is starting in emergency-mute state.".into(),
        }));

        let spawn_result = app
            .shell()
            .sidecar("riffra-audio")
            .and_then(|command| command.args(["--serve"]).spawn());

        let (mut receiver, child) = match spawn_result {
            Ok(pair) => pair,
            Err(error) => {
                if let Ok(mut current) = status.lock() {
                    current.state = AudioState::Faulted;
                    current.message = format!(
                        "Native audio sidecar could not start; the session and saved data remain available: {error}"
                    );
                }
                return Self {
                    status,
                    child: Mutex::new(None),
                };
            }
        };

        let event_status = Arc::clone(&status);
        tauri::async_runtime::spawn(async move {
            while let Some(event) = receiver.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) => {
                        handle_native_stdout(&event_status, &bytes);
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
                    CommandEvent::Error(error) => set_faulted(
                        &event_status,
                        format!(
                            "Native audio communication failed: {error}. The engine is isolated and saved data is safe."
                        ),
                    ),
                    CommandEvent::Terminated(payload) => set_faulted(
                        &event_status,
                        format!(
                            "Native audio process stopped (code {:?}); the UI and saved session remain available.",
                            payload.code
                        ),
                    ),
                    _ => {}
                }
            }
        });

        Self {
            status,
            child: Mutex::new(Some(child)),
        }
    }

    pub fn status(&self) -> Result<AudioStatus, String> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|error| format!("Audio status lock was poisoned: {error}"))
    }

    fn send_command(
        &self,
        command: serde_json::Value,
        message: &str,
    ) -> Result<AudioStatus, String> {
        let payload = serde_json::to_string(&command)
            .map_err(|error| format!("Audio command could not be encoded: {error}"))?;
        let mut child_slot = self
            .child
            .lock()
            .map_err(|error| format!("Audio child lock was poisoned: {error}"))?;
        let child = child_slot.as_mut().ok_or_else(|| {
            "Native audio is unavailable; the requested audio command was not sent.".to_string()
        })?;
        child
            .write(format!("{payload}\n").as_bytes())
            .map_err(|error| {
                format!(
                    "Audio recording command could not reach the isolated audio process: {error}"
                )
            })?;
        let mut status = self
            .status
            .lock()
            .map_err(|error| format!("Audio status lock was poisoned: {error}"))?;
        status.message = message.into();
        Ok(status.clone())
    }

    pub fn load_plugin(&self, path: &Path) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "loadPlugin", "path": path.to_string_lossy()}),
            "VST3 loaded into the isolated rack; audio remains under the safety limiter.",
        )
    }

    pub fn clear_plugin(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "clearPlugin"}),
            "VST3 removed from the isolated rack; the safety path remains active.",
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
    ) -> Result<AudioStatus, String> {
        let mut command = serde_json::json!({
            "type": "previewSample",
            "path": path.to_string_lossy(),
            "startMs": start_ms,
            "gain": 1.0,
            "loop": looped,
        });
        if let Some(end_ms) = end_ms {
            command["endMs"] = serde_json::json!(end_ms);
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

    pub fn open_midi_input(&self, name: &str) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "openMidiInput", "name": name}),
            "MIDI input opening requested; incoming note activity will remain isolated from audio safety state.",
        )
    }

    pub fn configure_sample_pads(&self, pads: &[SamplePad]) -> Result<AudioStatus, String> {
        let pads = serde_json::to_value(pads)
            .map_err(|error| format!("Sample pad mapping could not be encoded: {error}"))?;
        self.send_command(
            serde_json::json!({"type": "configureSamplePads", "pads": pads}),
            "Sample pad mappings were prepared for MIDI-triggered audition.",
        )
    }

    pub fn close_midi_input(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "closeMidiInput"}),
            "MIDI input closed; no notes are being consumed.",
        )
    }

    pub fn recover_audio_device(&self) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "recoverAudioDevice"}),
            "Audio device recovery requested; output remains muted until the device is ready.",
        )
    }

    pub fn set_audio_driver(&self, driver: &str) -> Result<AudioStatus, String> {
        self.send_command(
            serde_json::json!({"type": "setAudioDriver", "driver": driver}),
            "Audio driver switch requested; output remains muted until the new device is ready.",
        )
    }

    pub fn set_emergency_mute(&self, muted: bool) -> Result<AudioStatus, String> {
        let command = format!("{{\"type\":\"setEmergencyMute\",\"muted\":{muted}}}\n");
        let mut child_slot = self
            .child
            .lock()
            .map_err(|error| format!("Audio child lock was poisoned: {error}"))?;
        let child = child_slot.as_mut().ok_or_else(|| {
            "Native audio is unavailable; the requested mute state was not sent.".to_string()
        })?;
        child.write(command.as_bytes()).map_err(|error| {
            format!("Emergency mute command could not reach the isolated audio process: {error}")
        })?;

        let mut status = self
            .status
            .lock()
            .map_err(|error| format!("Audio status lock was poisoned: {error}"))?;
        status.state = if muted {
            AudioState::Muted
        } else {
            AudioState::Starting
        };
        status.message = if muted {
            "Emergency mute is engaged; saved and recorded data is unaffected.".into()
        } else {
            "Audio is fading in from silence through the safety limiter.".into()
        };
        Ok(status.clone())
    }
}

impl Drop for AudioSupervisor {
    fn drop(&mut self) {
        if let Ok(slot) = self.child.get_mut() {
            if let Some(mut child) = slot.take() {
                let _ = child.write(b"{\"type\":\"shutdown\"}\n");
                let _ = child.kill();
            }
        }
    }
}

fn update_from_native(status: &Arc<Mutex<AudioStatus>>, native: NativeStatus) {
    if let Ok(mut current) = status.lock() {
        current.state = match native.state.as_str() {
            "ready" => AudioState::Ready,
            "muted" => AudioState::Muted,
            "starting" => AudioState::Starting,
            "faulted" => AudioState::Faulted,
            _ => AudioState::Offline,
        };
        current.driver = native.driver;
        current.sample_rate = native
            .sample_rate
            .and_then(|rate| u32::try_from(rate.round() as i64).ok());
        current.buffer_size = native.buffer_size;
        current.round_trip_ms = native.round_trip_ms;
        current.recording = native
            .recording
            .map(|recording| RecordingStatus {
                active: recording.active,
                directory: recording.directory,
                sample_rate: recording
                    .sample_rate
                    .and_then(|rate| u32::try_from(rate.round() as i64).ok()),
                raw_channels: recording.raw_channels,
                processed_channels: recording.processed_channels,
                samples_written: recording.samples_written.unwrap_or_default(),
                dropped_blocks: recording.dropped_blocks.unwrap_or_default(),
            })
            .unwrap_or_default();
        current.plugin = native.plugin.map(|plugin| PluginStatus {
            loaded: plugin.loaded,
            bypassed: plugin.bypassed,
            path: plugin.path.filter(|path| !path.is_empty()),
            name: plugin.name.filter(|name| !name.is_empty()),
            sample_rate: plugin
                .sample_rate
                .and_then(|rate| u32::try_from(rate.round() as i64).ok()),
            block_size: plugin.block_size,
            bypassed_blocks: plugin.bypassed_blocks.unwrap_or_default(),
            parameters: plugin
                .parameters
                .unwrap_or_default()
                .into_iter()
                .map(|parameter| PluginParameter {
                    index: parameter.index,
                    name: parameter.name,
                    value: parameter.value.clamp(0.0, 1.0),
                    default_value: parameter.default_value.clamp(0.0, 1.0),
                    automatable: parameter.automatable,
                })
                .collect(),
        });
        current.midi_inputs = native.midi_inputs.unwrap_or_default();
        current.midi_outputs = native.midi_outputs.unwrap_or_default();
        current.midi_input_active = native.midi_input_active.unwrap_or(false);
        current.midi_messages = native.midi_messages.unwrap_or_default();
        current.last_midi_note = native
            .last_midi_note
            .and_then(|note| u8::try_from(note).ok());
        current.midi_pad_mappings = native.midi_pad_mappings.unwrap_or_default();
        current.midi_pad_triggers = native.midi_pad_triggers.unwrap_or_default();
        current.input_peak = native.input_peak.unwrap_or_default().clamp(0.0, 1.0);
        current.output_peak = native.output_peak.unwrap_or_default().clamp(0.0, 1.0);
        current.invalid_samples = native.invalid_samples.unwrap_or_default();
        current.message = match current.state {
            AudioState::Ready => "Native audio is ready through the safety chain.".into(),
            AudioState::Muted => "Native audio is connected and emergency-muted.".into(),
            AudioState::Starting => "Native audio is starting safely.".into(),
            AudioState::Faulted => "Native audio reported a fault; saved data is safe.".into(),
            AudioState::Offline => "Native audio is offline; saved data is safe.".into(),
        };
    }
}

fn handle_native_stdout(status: &Arc<Mutex<AudioStatus>>, bytes: &[u8]) {
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return;
    };
    match payload.get("type").and_then(serde_json::Value::as_str) {
        Some("audioStatus") => {
            if let Ok(native) = serde_json::from_value::<NativeStatus>(payload) {
                update_from_native(status, native);
            }
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
            if scope == "audioDevice" {
                set_faulted(
                    status,
                    format!("Native audio device error: {message}. Saved data remains safe."),
                );
            } else {
                set_command_error(
                    status,
                    format!(
                        "Native {scope} command failed: {message}. Audio and saved data remain safe."
                    ),
                );
            }
        }
        _ => {}
    }
}

fn set_command_error(status: &Arc<Mutex<AudioStatus>>, message: String) {
    if let Ok(mut current) = status.lock() {
        current.message = message;
    }
}

fn set_faulted(status: &Arc<Mutex<AudioStatus>>, message: String) {
    if let Ok(mut current) = status.lock() {
        current.state = AudioState::Faulted;
        current.message = message;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_status() -> Arc<Mutex<AudioStatus>> {
        Arc::new(Mutex::new(AudioStatus {
            state: AudioState::Ready,
            driver: Some("Test".into()),
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
}
