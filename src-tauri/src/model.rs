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
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub session: ScratchSession,
    pub recovered_from_generation: bool,
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
