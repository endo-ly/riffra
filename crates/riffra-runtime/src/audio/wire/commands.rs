//! Commands sent to `riffra-audio` and the request sent to `riffra-render`.

use crate::execution::{GraphPluginState, OfflineRenderRequest, TimelineSnapshot};
use crate::instrument::InstrumentPreviewDefinition;
use serde::Serialize;

/// One command accepted by the realtime sidecar.
///
/// `Option` fields serialize as `null`; the sidecar requires every key.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SidecarCommand {
    Status,
    SetEmergencyMute {
        muted: bool,
    },
    SetFeedbackProtection {
        active: bool,
    },
    SetEngineTransitionMute {
        active: bool,
    },
    PreviewMasterGainDb {
        gain_db: f64,
    },
    PrepareTimelineSnapshot {
        snapshot: TimelineSnapshot,
    },
    CommitTimelineSnapshot,
    DiscardTimelineSnapshot,
    WaitForTimelineIdle {
        timeout_ms: u64,
    },
    PlayTimeline,
    SetTransportStarting,
    StopTimeline,
    SeekTimeline {
        tick: u64,
    },
    EnableMidiListening,
    DisableMidiListening,
    SetLiveMidiTarget {
        track_id: Option<String>,
    },
    SendTrackMidi {
        track_id: String,
        bytes: Vec<u8>,
    },
    PanicTrackMidi {
        track_id: String,
    },
    SetTrackMix {
        track_id: String,
        gain_db: Option<f64>,
        pan: Option<f64>,
    },
    SetTrackDeviceBypassed {
        track_id: String,
        device_id: String,
        bypassed: bool,
    },
    SetTrackDeviceParameter {
        track_id: String,
        device_id: String,
        parameter_index: u32,
        value: f32,
    },
    GetTrackDeviceStatus {
        track_id: String,
        device_id: String,
    },
    GetTrackDeviceParameters {
        track_id: String,
        device_id: String,
    },
    GetTrackDevicePrograms {
        track_id: String,
        device_id: String,
    },
    GetTrackPluginState {
        track_id: String,
        device_id: String,
    },
    SetTrackPluginState {
        track_id: String,
        device_id: String,
        state: GraphPluginState,
    },
    SetTrackDeviceProgram {
        track_id: String,
        device_id: String,
        program_index: u32,
    },
    OpenTrackPluginEditor {
        project_id: String,
        track_id: String,
        device_id: String,
    },
    PreviewSample {
        path: String,
        start_ms: u64,
        end_ms: Option<u64>,
        gain: f32,
        #[serde(rename = "loop")]
        looped: bool,
    },
    PreviewInstrument {
        definition_json: String,
        definition_base_dir: String,
        preview: InstrumentPreviewDefinition,
    },
    StopPreview,
    StopInstrumentPreview,
    StartTakeComparison {
        raw_path: String,
        processed_path: String,
        raw_start_frame: u64,
        raw_end_frame: u64,
        processed_start_frame: u64,
        processed_end_frame: u64,
    },
    SwitchTakeComparisonVariant {
        variant: TakeComparisonVariant,
    },
    StopTakeComparison,
    RecoverAudioDevice,
    SetAudioDriver {
        driver: String,
        input_device: Option<String>,
        input_channel: u32,
        output_device: Option<String>,
        sample_rate: Option<u32>,
        buffer_size: Option<u32>,
    },
    StartArrangeRecording {
        directory: String,
        count_in_beats: u8,
    },
    StopArrangeRecording,
}

/// Audition source selected during take comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TakeComparisonVariant {
    Raw,
    Processed,
}

/// Response type that completes one command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExpectedResponse {
    AudioStatus,
    TransportStatus,
    Ack,
    TrackDeviceStatus,
    TrackDeviceParameters,
    TrackDevicePrograms,
    TrackPluginState,
}

impl SidecarCommand {
    /// Returns the only response type that may complete this command.
    pub(crate) fn expected_response(&self) -> ExpectedResponse {
        match self {
            Self::Status
            | Self::SetEmergencyMute { .. }
            | Self::SetFeedbackProtection { .. }
            | Self::SetEngineTransitionMute { .. }
            | Self::PreviewMasterGainDb { .. }
            | Self::EnableMidiListening
            | Self::DisableMidiListening
            | Self::SetLiveMidiTarget { .. }
            | Self::PreviewSample { .. }
            | Self::PreviewInstrument { .. }
            | Self::StopPreview
            | Self::StopInstrumentPreview
            | Self::StartTakeComparison { .. }
            | Self::SwitchTakeComparisonVariant { .. }
            | Self::StopTakeComparison
            | Self::RecoverAudioDevice
            | Self::SetAudioDriver { .. }
            | Self::StartArrangeRecording { .. }
            | Self::StopArrangeRecording => ExpectedResponse::AudioStatus,
            Self::PlayTimeline
            | Self::SetTransportStarting
            | Self::StopTimeline
            | Self::SeekTimeline { .. } => ExpectedResponse::TransportStatus,
            Self::PrepareTimelineSnapshot { .. }
            | Self::CommitTimelineSnapshot
            | Self::DiscardTimelineSnapshot
            | Self::WaitForTimelineIdle { .. }
            | Self::SendTrackMidi { .. }
            | Self::PanicTrackMidi { .. }
            | Self::SetTrackMix { .. }
            | Self::SetTrackDeviceBypassed { .. }
            | Self::SetTrackDeviceParameter { .. }
            | Self::SetTrackPluginState { .. }
            | Self::OpenTrackPluginEditor { .. } => ExpectedResponse::Ack,
            Self::GetTrackDeviceStatus { .. } => ExpectedResponse::TrackDeviceStatus,
            Self::GetTrackDeviceParameters { .. } => ExpectedResponse::TrackDeviceParameters,
            Self::GetTrackDevicePrograms { .. } => ExpectedResponse::TrackDevicePrograms,
            Self::GetTrackPluginState { .. } | Self::SetTrackDeviceProgram { .. } => {
                ExpectedResponse::TrackPluginState
            }
        }
    }
}

/// The one request line accepted by `riffra-render`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum OfflineRenderEnvelope {
    RenderTimelineOffline {
        protocol_version: u32,
        request: OfflineRenderRequest,
    },
}
