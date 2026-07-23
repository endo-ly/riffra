//! TypeScript boundary-type generation.
//!
//! Every Rust struct or enum that crosses the Tauri IPC boundary derives
//! `ts_rs::TS`. This module regenerates the TypeScript declaration files under
//! `src/lib/generated/` from those types. `scripts/verify.ps1` runs this export
//! before the TypeScript build and fails when the committed bindings differ
//! from the freshly generated output, so the two sides cannot drift.

use crate::analysis::AudioAnalysis;
use crate::asset::AssetId;
use crate::audio_preferences::AudioDriverConfig;
use crate::library::LibraryAsset;
use crate::missing::MissingDependency;
use crate::model::{
    AudioAccessMode, AudioChannelInfo, AudioDevicePairing, AudioDeviceProbe, AudioDriverInfo,
    AudioState, AudioStatus, BootstrapState, MidiProbe, PluginParameter, PluginStatus,
    RecordingStatus, RecoveryCandidate, SessionAudioPair,
};
use crate::plugins::{PluginEntry, ScanIssue, ScanReport};
use crate::projects::ProjectExport;
use crate::rack::{DeviceKind, RackDevice, RackInstance, RackMacro};
use crate::recording::{
    DropoutInformation, RecordingAsset, RecordingCapture, RecordingCaptureStatus,
};
use crate::render::{RenderOptions, RenderResult};
use crate::separation::SeparationResult;
use crate::session::{
    AiChangeSet, AiPermission, Arrangement, AudioClip, AudioClipMove, AudioClipPatch,
    CreativeSession, DesignContext, DesignTool, FrameDuration, FrameRange, Marker, MidiClip,
    MidiNote, MonitoringState, PlayState, ProjectTimebase, SampleInstrumentState, SamplePad,
    SessionSettings, SessionSnapshot, TimelineLoopRange, Track, TrackKind, Workspace,
};
use ts_rs::{Config, TS};

#[test]
fn export_types() {
    let cfg = Config::new()
        .with_out_dir("../src/lib/generated")
        .with_large_int("number");
    AssetId::export_all(&cfg).expect("AssetId bindings");
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
    TrackKind::export_all(&cfg).expect("TrackKind bindings");
    MonitoringState::export_all(&cfg).expect("MonitoringState bindings");
    Track::export_all(&cfg).expect("Track bindings");
    MidiNote::export_all(&cfg).expect("MidiNote bindings");
    MidiClip::export_all(&cfg).expect("MidiClip bindings");
    AudioClip::export_all(&cfg).expect("AudioClip bindings");
    AudioClipPatch::export_all(&cfg).expect("AudioClipPatch bindings");
    AudioClipMove::export_all(&cfg).expect("AudioClipMove bindings");
    Arrangement::export_all(&cfg).expect("Arrangement bindings");
    SamplePad::export_all(&cfg).expect("SamplePad bindings");
    SampleInstrumentState::export_all(&cfg).expect("SampleInstrumentState bindings");
    PlayState::export_all(&cfg).expect("PlayState bindings");
    SessionSnapshot::export_all(&cfg).expect("SessionSnapshot bindings");
    AiChangeSet::export_all(&cfg).expect("AiChangeSet bindings");
    SessionSettings::export_all(&cfg).expect("SessionSettings bindings");
    CreativeSession::export_all(&cfg).expect("CreativeSession bindings");
    AudioState::export_all(&cfg).expect("AudioState bindings");
    AudioAccessMode::export_all(&cfg).expect("AudioAccessMode bindings");
    AudioDevicePairing::export_all(&cfg).expect("AudioDevicePairing bindings");
    AudioChannelInfo::export_all(&cfg).expect("AudioChannelInfo bindings");
    AudioDriverInfo::export_all(&cfg).expect("AudioDriverInfo bindings");
    AudioDeviceProbe::export_all(&cfg).expect("AudioDeviceProbe bindings");
    MidiProbe::export_all(&cfg).expect("MidiProbe bindings");
    PluginParameter::export_all(&cfg).expect("PluginParameter bindings");
    PluginStatus::export_all(&cfg).expect("PluginStatus bindings");
    RecordingStatus::export_all(&cfg).expect("RecordingStatus bindings");
    RecoveryCandidate::export_all(&cfg).expect("RecoveryCandidate bindings");
    AudioStatus::export_all(&cfg).expect("AudioStatus bindings");
    SessionAudioPair::export_all(&cfg).expect("SessionAudioPair bindings");
    BootstrapState::export_all(&cfg).expect("BootstrapState bindings");
    AudioAnalysis::export_all(&cfg).expect("AudioAnalysis bindings");
    AudioDriverConfig::export_all(&cfg).expect("AudioDriverConfig bindings");
    LibraryAsset::export_all(&cfg).expect("LibraryAsset bindings");
    MissingDependency::export_all(&cfg).expect("MissingDependency bindings");
    PluginEntry::export_all(&cfg).expect("PluginEntry bindings");
    ProjectExport::export_all(&cfg).expect("ProjectExport bindings");
    RenderOptions::export_all(&cfg).expect("RenderOptions bindings");
    RenderResult::export_all(&cfg).expect("RenderResult bindings");
    ScanIssue::export_all(&cfg).expect("ScanIssue bindings");
    ScanReport::export_all(&cfg).expect("ScanReport bindings");
    SeparationResult::export_all(&cfg).expect("SeparationResult bindings");
    RecordingCaptureStatus::export_all(&cfg).expect("RecordingCaptureStatus bindings");
    DropoutInformation::export_all(&cfg).expect("DropoutInformation bindings");
    RecordingCapture::export_all(&cfg).expect("RecordingCapture bindings");
    RecordingAsset::export_all(&cfg).expect("RecordingAsset bindings");
}
