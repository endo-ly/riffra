use serde::{Deserialize, Serialize};

pub const CURRENT_SESSION_FORMAT: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RackDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub path: Option<String>,
    pub bypassed: bool,
    pub gain_db: f64,
    #[serde(default)]
    pub parameter_values: Vec<f32>,
    #[serde(default)]
    pub state_data: Option<String>,
    #[serde(default)]
    pub disabled_placeholder: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RackMacro {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub value: f32,
    #[serde(default)]
    pub parameter_index: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub description: String,
    pub tag: Option<String>,
    pub parent_id: Option<String>,
    pub master_db: f64,
    pub rack: Vec<RackDevice>,
    #[serde(default)]
    pub macros: Vec<RackMacro>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineClip {
    pub id: String,
    pub asset_path: String,
    pub name: String,
    #[serde(default = "default_track_id")]
    pub track_id: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub source_in_ms: u64,
    #[serde(default)]
    pub source_out_ms: u64,
    #[serde(default)]
    pub loop_enabled: bool,
    pub gain_db: f64,
    #[serde(default)]
    pub fade_in_ms: u64,
    #[serde(default)]
    pub fade_out_ms: u64,
    #[serde(default)]
    pub pan: f64,
    pub muted: bool,
}

fn default_track_id() -> String {
    "main".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineTrack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MidiNote {
    pub id: String,
    pub note: u8,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub velocity: u8,
    pub channel: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MidiClip {
    pub id: String,
    pub name: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub notes: Vec<MidiNote>,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SamplePad {
    pub id: String,
    pub name: String,
    pub asset_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub midi_key: u8,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub loop_enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiChangeSet {
    pub id: String,
    pub created_at_ms: u64,
    pub permission: String,
    pub target: String,
    pub current_gain_db: f64,
    pub proposed_gain_db: f64,
    pub reason: String,
    pub expected_effect: String,
    pub risk: String,
    #[serde(default)]
    pub context: Vec<String>,
    pub applied: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Input,
    Plugin,
    Utility,
    Output,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Workspace {
    Home,
    Play,
    Arrange,
    Sample,
    Analyze,
    Separate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScratchSession {
    pub format_version: u32,
    pub session_id: String,
    pub updated_at_ms: u64,
    pub project_name: Option<String>,
    pub workspace: Workspace,
    #[serde(default)]
    pub audio_driver: Option<String>,
    #[serde(default)]
    pub audio_sample_rate: Option<u32>,
    #[serde(default)]
    pub audio_buffer_size: Option<u32>,
    pub master_db: f64,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default)]
    pub count_in_beats: u8,
    pub emergency_muted: bool,
    pub rack: Vec<RackDevice>,
    #[serde(default)]
    pub snapshots: Vec<SessionSnapshot>,
    #[serde(default = "default_macros")]
    pub macros: Vec<RackMacro>,
    #[serde(default)]
    pub timeline: Vec<TimelineClip>,
    #[serde(default = "default_tracks")]
    pub tracks: Vec<TimelineTrack>,
    #[serde(default)]
    pub midi_clips: Vec<MidiClip>,
    #[serde(default)]
    pub sample_pads: Vec<SamplePad>,
    pub note: String,
    #[serde(default = "default_ai_permission")]
    pub ai_permission: String,
    #[serde(default = "default_ai_context")]
    pub ai_context: Vec<String>,
    #[serde(default)]
    pub ai_history: Vec<AiChangeSet>,
}

fn default_tracks() -> Vec<TimelineTrack> {
    vec![TimelineTrack {
        id: "main".into(),
        name: "Main".into(),
        gain_db: 0.0,
        pan: 0.0,
        muted: false,
        solo: false,
    }]
}

fn default_macros() -> Vec<RackMacro> {
    ["Brightness", "Gain", "Space", "Width"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| RackMacro {
            id: format!("macro:{index}"),
            name: name.into(),
            value: 0.5,
            parameter_index: None,
        })
        .collect()
}

fn default_ai_permission() -> String {
    "Suggest".into()
}

fn default_ai_context() -> Vec<String> {
    vec!["analysis".into(), "selectedClip".into()]
}

const AI_CONTEXT_IDS: &[&str] = &[
    "selectedRack",
    "parameterList",
    "analysis",
    "selectedClip",
    "project",
    "userNote",
    "snapshot",
    "previewAudio",
    "errorLog",
];

impl ScratchSession {
    pub fn new(now_ms: u64) -> Self {
        Self {
            format_version: CURRENT_SESSION_FORMAT,
            session_id: format!("scratch-{now_ms}"),
            updated_at_ms: now_ms,
            project_name: None,
            workspace: Workspace::Home,
            audio_driver: None,
            audio_sample_rate: None,
            audio_buffer_size: None,
            master_db: -18.0,
            loop_enabled: false,
            count_in_beats: 0,
            emergency_muted: true,
            rack: vec![
                RackDevice {
                    id: "input".into(),
                    name: "Input 1".into(),
                    kind: DeviceKind::Input,
                    path: None,
                    bypassed: false,
                    gain_db: 0.0,
                    parameter_values: Vec::new(),
                    state_data: None,
                    disabled_placeholder: false,
                },
                RackDevice {
                    id: "safety".into(),
                    name: "Safety Limiter".into(),
                    kind: DeviceKind::Utility,
                    path: None,
                    bypassed: false,
                    gain_db: 0.0,
                    parameter_values: Vec::new(),
                    state_data: None,
                    disabled_placeholder: false,
                },
                RackDevice {
                    id: "output".into(),
                    name: "Main Out".into(),
                    kind: DeviceKind::Output,
                    path: None,
                    bypassed: false,
                    gain_db: -18.0,
                    parameter_values: Vec::new(),
                    state_data: None,
                    disabled_placeholder: false,
                },
            ],
            snapshots: Vec::new(),
            macros: default_macros(),
            timeline: Vec::new(),
            tracks: default_tracks(),
            midi_clips: Vec::new(),
            sample_pads: Vec::new(),
            note: String::new(),
            ai_permission: default_ai_permission(),
            ai_context: default_ai_context(),
            ai_history: Vec::new(),
        }
    }

    pub fn validate_and_normalize(mut self) -> Result<Self, String> {
        if self.format_version != CURRENT_SESSION_FORMAT {
            return Err(format!(
                "Unsupported session format {} (expected {}).",
                self.format_version, CURRENT_SESSION_FORMAT
            ));
        }
        if self.session_id.trim().is_empty() {
            return Err("Session id must not be empty.".into());
        }
        if !self.master_db.is_finite() {
            return Err("Master gain must be finite.".into());
        }
        self.master_db = self.master_db.clamp(-90.0, 0.0);
        if self.count_in_beats > 8 {
            return Err("Count-in must be between 0 and 8 beats.".into());
        }
        self.audio_driver = self
            .audio_driver
            .map(|value| value.trim().chars().take(128).collect())
            .filter(|value: &String| !value.is_empty());
        if let Some(sample_rate) = self.audio_sample_rate
            && !(8_000..=192_000).contains(&sample_rate)
        {
            return Err("Audio sample rate preference is outside 8-192 kHz.".into());
        }
        if let Some(buffer_size) = self.audio_buffer_size
            && !(16..=8192).contains(&buffer_size)
        {
            return Err("Audio buffer preference is outside 16-8192 samples.".into());
        }
        self.note.truncate(16_384);
        if !matches!(self.ai_permission.as_str(), "Explain" | "Suggest" | "Apply") {
            return Err("AI permission must be Explain, Suggest, or Apply.".into());
        }
        self.ai_context.truncate(16);
        self.ai_context.retain(|item| {
            !item.trim().is_empty() && item.len() <= 64 && AI_CONTEXT_IDS.contains(&item.as_str())
        });
        self.ai_context.dedup();
        if self.ai_history.len() > 128 {
            return Err("AI history cannot contain more than 128 ChangeSets.".into());
        }
        for change_set in &mut self.ai_history {
            if change_set.id.trim().is_empty() || change_set.target.trim().is_empty() {
                return Err("AI ChangeSets require non-empty ids and targets.".into());
            }
            if !matches!(
                change_set.permission.as_str(),
                "Explain" | "Suggest" | "Apply"
            ) {
                return Err("AI ChangeSet permission is invalid.".into());
            }
            if !change_set.current_gain_db.is_finite() || !change_set.proposed_gain_db.is_finite() {
                return Err(format!(
                    "AI ChangeSet '{}' has invalid gain values.",
                    change_set.id
                ));
            }
            change_set.current_gain_db = change_set.current_gain_db.clamp(-90.0, 24.0);
            change_set.proposed_gain_db = change_set.proposed_gain_db.clamp(-90.0, 24.0);
            change_set.reason.truncate(4_096);
            change_set.expected_effect.truncate(4_096);
            change_set.risk.truncate(256);
            change_set.context.truncate(16);
            change_set
                .context
                .retain(|item| AI_CONTEXT_IDS.contains(&item.as_str()));
            change_set.context.dedup();
        }
        if self.rack.len() > 256 {
            return Err("A rack cannot contain more than 256 devices.".into());
        }
        if self.snapshots.len() > 16 {
            return Err("A session cannot contain more than 16 snapshots.".into());
        }
        if self.macros.len() > 64 {
            return Err("A session cannot contain more than 64 rack macros.".into());
        }
        for macro_control in &mut self.macros {
            if macro_control.id.trim().is_empty() || macro_control.name.trim().is_empty() {
                return Err("Rack macros require non-empty ids and names.".into());
            }
            if !macro_control.value.is_finite() {
                return Err(format!(
                    "Rack macro '{}' has an invalid value.",
                    macro_control.name
                ));
            }
            macro_control.value = macro_control.value.clamp(0.0, 1.0);
        }
        if self.timeline.len() > 512 {
            return Err("A timeline cannot contain more than 512 clips.".into());
        }
        if self.tracks.is_empty() {
            self.tracks = default_tracks();
        }
        if self.tracks.len() > 128 {
            return Err("A session cannot contain more than 128 timeline tracks.".into());
        }
        for track in &mut self.tracks {
            if track.id.trim().is_empty() || track.name.trim().is_empty() {
                return Err("Timeline tracks require non-empty ids and names.".into());
            }
            if !track.gain_db.is_finite() || !track.pan.is_finite() {
                return Err(format!(
                    "Timeline track '{}' has invalid mix values.",
                    track.name
                ));
            }
            track.gain_db = track.gain_db.clamp(-90.0, 24.0);
            track.pan = track.pan.clamp(-1.0, 1.0);
        }
        if self.midi_clips.len() > 256 {
            return Err("A session cannot contain more than 256 MIDI clips.".into());
        }
        if self.sample_pads.len() > 128 {
            return Err("A sample instrument cannot contain more than 128 pads.".into());
        }
        for device in &mut self.rack {
            if device.id.trim().is_empty() || device.name.trim().is_empty() {
                return Err("Rack devices require non-empty ids and names.".into());
            }
            if !device.gain_db.is_finite() {
                return Err(format!("Device '{}' has an invalid gain.", device.name));
            }
            device.gain_db = device.gain_db.clamp(-90.0, 24.0);
            if device.parameter_values.len() > 512 {
                return Err(format!(
                    "Device '{}' exposes too many parameter values.",
                    device.name
                ));
            }
            for value in &mut device.parameter_values {
                if !value.is_finite() {
                    return Err(format!(
                        "Device '{}' has an invalid parameter value.",
                        device.name
                    ));
                }
                *value = value.clamp(0.0, 1.0);
            }
            if device
                .state_data
                .as_ref()
                .is_some_and(|state| state.len() > 4_000_000)
            {
                return Err(format!(
                    "Device '{}' has an oversized state blob.",
                    device.name
                ));
            }
        }
        for snapshot in &mut self.snapshots {
            if snapshot.id.trim().is_empty() || snapshot.name.trim().is_empty() {
                return Err("Snapshots require non-empty ids and names.".into());
            }
            if !snapshot.master_db.is_finite() {
                return Err(format!(
                    "Snapshot '{}' has an invalid master gain.",
                    snapshot.name
                ));
            }
            snapshot.master_db = snapshot.master_db.clamp(-90.0, 0.0);
            snapshot.description.truncate(16_384);
            if snapshot.rack.len() > 256 {
                return Err(format!(
                    "Snapshot '{}' contains too many rack devices.",
                    snapshot.name
                ));
            }
        }
        for clip in &mut self.timeline {
            if clip.id.trim().is_empty()
                || clip.asset_path.trim().is_empty()
                || clip.name.trim().is_empty()
                || clip.track_id.trim().is_empty()
            {
                return Err("Timeline clips require ids, source paths and names.".into());
            }
            if !clip.gain_db.is_finite() {
                return Err(format!(
                    "Timeline clip '{}' has an invalid gain.",
                    clip.name
                ));
            }
            clip.gain_db = clip.gain_db.clamp(-90.0, 24.0);
            clip.fade_in_ms = clip.fade_in_ms.min(clip.duration_ms);
            clip.fade_out_ms = clip.fade_out_ms.min(clip.duration_ms);
            if clip.source_out_ms > 0 && clip.source_out_ms <= clip.source_in_ms {
                return Err(format!(
                    "Timeline clip '{}' has an invalid source range.",
                    clip.name
                ));
            }
            if !clip.pan.is_finite() {
                return Err(format!("Timeline clip '{}' has an invalid pan.", clip.name));
            }
            clip.pan = clip.pan.clamp(-1.0, 1.0);
        }
        for clip in &mut self.midi_clips {
            if clip.id.trim().is_empty() || clip.name.trim().is_empty() {
                return Err("MIDI clips require non-empty ids and names.".into());
            }
            if clip.duration_ms == 0 {
                return Err(format!("MIDI clip '{}' must have a duration.", clip.name));
            }
            if clip.notes.len() > 200_000 {
                return Err(format!(
                    "MIDI clip '{}' contains too many notes.",
                    clip.name
                ));
            }
            for note in &clip.notes {
                if note.id.trim().is_empty()
                    || note.note > 127
                    || note.velocity > 127
                    || note.channel == 0
                    || note.channel > 16
                    || note.duration_ms == 0
                {
                    return Err(format!(
                        "MIDI clip '{}' contains an invalid note.",
                        clip.name
                    ));
                }
            }
        }
        for pad in &self.sample_pads {
            if pad.id.trim().is_empty()
                || pad.name.trim().is_empty()
                || pad.asset_path.trim().is_empty()
            {
                return Err("Sample pads require ids, names and source paths.".into());
            }
            if pad.end_ms <= pad.start_ms {
                return Err(format!(
                    "Sample pad '{}' has an invalid slice range.",
                    pad.name
                ));
            }
            if !pad.gain_db.is_finite() {
                return Err(format!("Sample pad '{}' has an invalid gain.", pad.name));
            }
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    pub file_name: String,
    pub updated_at_ms: u64,
    pub session_id: String,
    pub project_name: Option<String>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub session: ScratchSession,
    pub recovered_from_generation: bool,
    pub safe_mode: bool,
    pub native_available: bool,
    pub recovery_candidates: Vec<RecoveryCandidate>,
    pub data_root: String,
    pub vst3_root: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub active: bool,
    pub directory: Option<String>,
    pub sample_rate: Option<u32>,
    pub raw_channels: Option<u32>,
    pub processed_channels: Option<u32>,
    pub samples_written: u64,
    pub dropped_blocks: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginParameter {
    pub index: u32,
    pub name: String,
    pub value: f32,
    pub default_value: f32,
    pub automatable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    pub loaded: bool,
    pub bypassed: bool,
    pub path: Option<String>,
    pub name: Option<String>,
    pub sample_rate: Option<u32>,
    pub block_size: Option<u32>,
    pub bypassed_blocks: u64,
    pub parameters: Vec<PluginParameter>,
    pub state_data: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatus {
    pub state: AudioState,
    pub driver: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
    pub round_trip_ms: Option<f64>,
    pub recording: RecordingStatus,
    pub plugin: Option<PluginStatus>,
    pub midi_inputs: Vec<String>,
    pub midi_outputs: Vec<String>,
    pub midi_input_active: bool,
    pub midi_messages: u64,
    pub last_midi_note: Option<u8>,
    pub midi_pad_mappings: u32,
    pub midi_pad_triggers: u64,
    pub input_peak: f64,
    pub output_peak: f64,
    pub invalid_samples: u64,
    pub feedback_suspected: bool,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiProbe {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub refreshed_at_ms: u64,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDriverInfo {
    pub name: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceProbe {
    pub drivers: Vec<AudioDriverInfo>,
    pub midi_inputs: Vec<String>,
    pub midi_outputs: Vec<String>,
    pub refreshed_at_ms: u64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioState {
    Offline,
    Starting,
    Ready,
    Muted,
    Faulted,
}
