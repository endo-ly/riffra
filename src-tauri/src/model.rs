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
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineClip {
    pub id: String,
    pub asset_path: String,
    pub name: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub gain_db: f64,
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
    pub master_db: f64,
    pub emergency_muted: bool,
    pub rack: Vec<RackDevice>,
    #[serde(default)]
    pub snapshots: Vec<SessionSnapshot>,
    #[serde(default)]
    pub timeline: Vec<TimelineClip>,
    #[serde(default)]
    pub sample_pads: Vec<SamplePad>,
    pub note: String,
}

impl ScratchSession {
    pub fn new(now_ms: u64) -> Self {
        Self {
            format_version: CURRENT_SESSION_FORMAT,
            session_id: format!("scratch-{now_ms}"),
            updated_at_ms: now_ms,
            project_name: None,
            workspace: Workspace::Home,
            master_db: -18.0,
            emergency_muted: true,
            rack: vec![
                RackDevice {
                    id: "input".into(),
                    name: "Input 1".into(),
                    kind: DeviceKind::Input,
                    path: None,
                    bypassed: false,
                    gain_db: 0.0,
                },
                RackDevice {
                    id: "safety".into(),
                    name: "Safety Limiter".into(),
                    kind: DeviceKind::Utility,
                    path: None,
                    bypassed: false,
                    gain_db: 0.0,
                },
                RackDevice {
                    id: "output".into(),
                    name: "Main Out".into(),
                    kind: DeviceKind::Output,
                    path: None,
                    bypassed: false,
                    gain_db: -18.0,
                },
            ],
            snapshots: Vec::new(),
            timeline: Vec::new(),
            sample_pads: Vec::new(),
            note: String::new(),
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
        self.note.truncate(16_384);
        if self.rack.len() > 256 {
            return Err("A rack cannot contain more than 256 devices.".into());
        }
        if self.snapshots.len() > 16 {
            return Err("A session cannot contain more than 16 snapshots.".into());
        }
        if self.timeline.len() > 512 {
            return Err("A timeline cannot contain more than 512 clips.".into());
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
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub session: ScratchSession,
    pub recovered_from_generation: bool,
    pub safe_mode: bool,
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
pub struct PluginStatus {
    pub loaded: bool,
    pub bypassed: bool,
    pub path: Option<String>,
    pub name: Option<String>,
    pub sample_rate: Option<u32>,
    pub block_size: Option<u32>,
    pub bypassed_blocks: u64,
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
    pub input_peak: f64,
    pub output_peak: f64,
    pub invalid_samples: u64,
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
