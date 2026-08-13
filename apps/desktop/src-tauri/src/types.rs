//! TypeScript boundary-type generation.
//!
//! Every Rust struct or enum that crosses the Tauri IPC boundary derives
//! `ts_rs::TS`. This module regenerates the TypeScript declaration files under
//! `src/lib/generated/` from those types. `scripts/verify.mjs` runs this export
//! before the TypeScript build and fails when the committed bindings differ
//! from the freshly generated output, so the two sides cannot drift.

use crate::analysis::AudioAnalysis;
use crate::audio_preferences::AudioDriverConfig;
use crate::jobs::{BackgroundJobStatus, JobKind, JobState};
use crate::library::LibraryAsset;
use crate::missing::MissingDependency;
use crate::model::{
    AudioAccessMode, AudioChannelInfo, AudioDevicePairing, AudioDeviceProbe, AudioDriverInfo,
    AudioState, AudioStatus, BootstrapState, DeviceChannels, MidiDeviceInfo, MidiProbe,
    RecordingStatus, RecoveryCandidate, RuntimeProjectionState, RuntimeProjectionStatus,
    SessionAudioPair,
};
use crate::plugins::{PluginEntry, PluginFormat, PluginScanState, ScanIssue, ScanReport};
use crate::presentation::{DesignContext, DesignTool, DesktopViewState, Workspace};
use crate::projects::ProjectExport;
use crate::recording::{
    DropoutInformation, RecordingAsset, RecordingCapture, RecordingCaptureStatus,
};
use crate::render::{RenderOptions, RenderRange, RenderResult};
use crate::separation::SeparationResult;
use riffra_core::{
    AiChangeSet, AiPermission, Arrangement, AssetId, AudioClip, AudioClipMove, AudioClipPatch,
    AudioInputRoute, AudioTakeVariant, AutomationLane, AutomationParameter, AutomationPoint,
    CreativeSession, DeviceKind, FrameDuration, FrameRange, HistoryState, Marker, MidiClip,
    MidiClipMove, MidiClipPatch, MidiInputRoute, MidiNote, MonitoringState, PlayState,
    ProjectTimebase, RackDevice, RackInstance, RackMacro, RecordingPassRecord,
    RecordingSessionRecord, RecordingSessionTrackSlot, RecordingTakeRecord, SampleInstrumentState,
    SamplePad, SessionSettings, TimelineLoopRange, Track, TrackKind,
};
use ts_rs::{Config, TS};

#[test]
fn export_types() {
    let cfg = Config::new()
        .with_out_dir("../src/lib/generated")
        .with_large_int("number");
    AssetId::export_all(&cfg).expect("AssetId bindings");
    BackgroundJobStatus::export_all(&cfg).expect("BackgroundJobStatus bindings");
    JobKind::export_all(&cfg).expect("JobKind bindings");
    JobState::export_all(&cfg).expect("JobState bindings");
    FrameRange::export_all(&cfg).expect("FrameRange bindings");
    FrameDuration::export_all(&cfg).expect("FrameDuration bindings");
    Marker::export_all(&cfg).expect("Marker bindings");
    AiPermission::export_all(&cfg).expect("AiPermission bindings");
    DeviceKind::export_all(&cfg).expect("DeviceKind bindings");
    RackDevice::export_all(&cfg).expect("RackDevice bindings");
    RackMacro::export_all(&cfg).expect("RackMacro bindings");
    RackInstance::export_all(&cfg).expect("RackInstance bindings");
    DesignTool::export_all(&cfg).expect("DesignTool bindings");
    Workspace::export_all(&cfg).expect("Workspace bindings");
    ProjectTimebase::export_all(&cfg).expect("ProjectTimebase bindings");
    TimelineLoopRange::export_all(&cfg).expect("TimelineLoopRange bindings");
    DesignContext::export_all(&cfg).expect("DesignContext bindings");
    DesktopViewState::export_all(&cfg).expect("DesktopViewState bindings");
    TrackKind::export_all(&cfg).expect("TrackKind bindings");
    MonitoringState::export_all(&cfg).expect("MonitoringState bindings");
    AudioInputRoute::export_all(&cfg).expect("AudioInputRoute bindings");
    MidiInputRoute::export_all(&cfg).expect("MidiInputRoute bindings");
    Track::export_all(&cfg).expect("Track bindings");
    AutomationParameter::export_all(&cfg).expect("AutomationParameter bindings");
    AutomationPoint::export_all(&cfg).expect("AutomationPoint bindings");
    AutomationLane::export_all(&cfg).expect("AutomationLane bindings");
    MidiNote::export_all(&cfg).expect("MidiNote bindings");
    MidiClip::export_all(&cfg).expect("MidiClip bindings");
    AudioTakeVariant::export_all(&cfg).expect("AudioTakeVariant bindings");
    RecordingSessionTrackSlot::export_all(&cfg).expect("RecordingSessionTrackSlot bindings");
    RecordingSessionRecord::export_all(&cfg).expect("RecordingSessionRecord bindings");
    RecordingPassRecord::export_all(&cfg).expect("RecordingPassRecord bindings");
    RecordingTakeRecord::export_all(&cfg).expect("RecordingTakeRecord bindings");
    AudioClip::export_all(&cfg).expect("AudioClip bindings");
    AudioClipPatch::export_all(&cfg).expect("AudioClipPatch bindings");
    AudioClipMove::export_all(&cfg).expect("AudioClipMove bindings");
    MidiClipMove::export_all(&cfg).expect("MidiClipMove bindings");
    MidiClipPatch::export_all(&cfg).expect("MidiClipPatch bindings");
    Arrangement::export_all(&cfg).expect("Arrangement bindings");
    SamplePad::export_all(&cfg).expect("SamplePad bindings");
    SampleInstrumentState::export_all(&cfg).expect("SampleInstrumentState bindings");
    PlayState::export_all(&cfg).expect("PlayState bindings");
    AiChangeSet::export_all(&cfg).expect("AiChangeSet bindings");
    SessionSettings::export_all(&cfg).expect("SessionSettings bindings");
    CreativeSession::export_all(&cfg).expect("CreativeSession bindings");
    HistoryState::export_all(&cfg).expect("HistoryState bindings");
    AudioState::export_all(&cfg).expect("AudioState bindings");
    AudioAccessMode::export_all(&cfg).expect("AudioAccessMode bindings");
    AudioDevicePairing::export_all(&cfg).expect("AudioDevicePairing bindings");
    AudioChannelInfo::export_all(&cfg).expect("AudioChannelInfo bindings");
    AudioDriverInfo::export_all(&cfg).expect("AudioDriverInfo bindings");
    AudioDeviceProbe::export_all(&cfg).expect("AudioDeviceProbe bindings");
    DeviceChannels::export_all(&cfg).expect("DeviceChannels bindings");
    MidiProbe::export_all(&cfg).expect("MidiProbe bindings");
    MidiDeviceInfo::export_all(&cfg).expect("MidiDeviceInfo bindings");
    RecordingStatus::export_all(&cfg).expect("RecordingStatus bindings");
    RecoveryCandidate::export_all(&cfg).expect("RecoveryCandidate bindings");
    AudioStatus::export_all(&cfg).expect("AudioStatus bindings");
    RuntimeProjectionState::export_all(&cfg).expect("RuntimeProjectionState bindings");
    RuntimeProjectionStatus::export_all(&cfg).expect("RuntimeProjectionStatus bindings");
    SessionAudioPair::export_all(&cfg).expect("SessionAudioPair bindings");
    BootstrapState::export_all(&cfg).expect("BootstrapState bindings");
    AudioAnalysis::export_all(&cfg).expect("AudioAnalysis bindings");
    AudioDriverConfig::export_all(&cfg).expect("AudioDriverConfig bindings");
    LibraryAsset::export_all(&cfg).expect("LibraryAsset bindings");
    MissingDependency::export_all(&cfg).expect("MissingDependency bindings");
    PluginEntry::export_all(&cfg).expect("PluginEntry bindings");
    PluginFormat::export_all(&cfg).expect("PluginFormat bindings");
    PluginScanState::export_all(&cfg).expect("PluginScanState bindings");
    ProjectExport::export_all(&cfg).expect("ProjectExport bindings");
    RenderOptions::export_all(&cfg).expect("RenderOptions bindings");
    RenderRange::export_all(&cfg).expect("RenderRange bindings");
    RenderResult::export_all(&cfg).expect("RenderResult bindings");
    ScanIssue::export_all(&cfg).expect("ScanIssue bindings");
    ScanReport::export_all(&cfg).expect("ScanReport bindings");
    SeparationResult::export_all(&cfg).expect("SeparationResult bindings");
    RecordingCaptureStatus::export_all(&cfg).expect("RecordingCaptureStatus bindings");
    DropoutInformation::export_all(&cfg).expect("DropoutInformation bindings");
    RecordingCapture::export_all(&cfg).expect("RecordingCapture bindings");
    RecordingAsset::export_all(&cfg).expect("RecordingAsset bindings");
}
