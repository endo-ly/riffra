//! Commands sent to `riffra-audio` and the request sent to `riffra-render`.

use crate::api::output::InstrumentPreviewDefinition;
use crate::execution::{GraphPluginState, OfflineRenderRequest, TimelineSnapshot};
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
    SetChannelMix {
        owner: riffra_core::EffectOwner,
        gain_db: Option<f64>,
        pan: Option<f64>,
    },
    SetSendLevel {
        send_id: String,
        level_db: f64,
    },
    SetDeviceBypassed {
        owner: riffra_core::EffectOwner,
        device_id: String,
        bypassed: bool,
    },
    SetDeviceParameter {
        owner: riffra_core::EffectOwner,
        device_id: String,
        parameter_index: u32,
        value: Option<f32>,
        display: Option<String>,
    },
    GetDeviceStatus {
        owner: riffra_core::EffectOwner,
        device_id: String,
    },
    GetDeviceParameters {
        owner: riffra_core::EffectOwner,
        device_id: String,
    },
    GetDeviceParameterValueForText {
        owner: riffra_core::EffectOwner,
        device_id: String,
        parameter_id: String,
        display: String,
    },
    GetDevicePrograms {
        owner: riffra_core::EffectOwner,
        device_id: String,
    },
    GetPluginState {
        owner: riffra_core::EffectOwner,
        device_id: String,
    },
    SetPluginState {
        owner: riffra_core::EffectOwner,
        device_id: String,
        state: GraphPluginState,
    },
    SetDeviceProgram {
        owner: riffra_core::EffectOwner,
        device_id: String,
        program_index: u32,
    },
    OpenPluginEditor {
        project_id: String,
        owner: riffra_core::EffectOwner,
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
    OpenPluginAudition {
        path: String,
    },
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
    TransportAccepted,
    TimelineAck,
    TimelineIdleAck,
    MidiAck,
    MixAck,
    DeviceAck,
    DeviceStatus,
    DeviceParameters,
    DevicePrograms,
    PluginState,
    DeviceProgramChanged,
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
            | Self::OpenPluginAudition { .. }
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
            | Self::SeekTimeline { .. } => ExpectedResponse::TransportAccepted,
            Self::PrepareTimelineSnapshot { .. }
            | Self::CommitTimelineSnapshot
            | Self::DiscardTimelineSnapshot => ExpectedResponse::TimelineAck,
            Self::WaitForTimelineIdle { .. } => ExpectedResponse::TimelineIdleAck,
            Self::SendTrackMidi { .. } | Self::PanicTrackMidi { .. } => ExpectedResponse::MidiAck,
            Self::SetChannelMix { .. } | Self::SetSendLevel { .. } => ExpectedResponse::MixAck,
            Self::SetDeviceBypassed { .. }
            | Self::SetDeviceParameter { .. }
            | Self::SetPluginState { .. }
            | Self::OpenPluginEditor { .. } => ExpectedResponse::DeviceAck,
            Self::GetDeviceStatus { .. } => ExpectedResponse::DeviceStatus,
            Self::GetDeviceParameters { .. } | Self::GetDeviceParameterValueForText { .. } => {
                ExpectedResponse::DeviceParameters
            }
            Self::GetDevicePrograms { .. } => ExpectedResponse::DevicePrograms,
            Self::GetPluginState { .. } => ExpectedResponse::PluginState,
            Self::SetDeviceProgram { .. } => ExpectedResponse::DeviceProgramChanged,
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
