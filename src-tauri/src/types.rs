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
    AudioState, AudioStatus, MidiProbe, PluginParameter, PluginStatus, RecordingStatus,
    RecoveryCandidate,
};
use crate::plugins::{PluginEntry, ScanIssue, ScanReport};
use crate::projects::ProjectExport;
use crate::render::{RenderOptions, RenderResult};
use crate::separation::SeparationResult;
use crate::session::{FrameDuration, FrameRange, Marker};
use ts_rs::{Config, TS};

/// Boundary types whose bindings are regenerated. A type appears here once its
/// `ts-rs` output matches the hand-written TypeScript it replaces; adding a
/// type here makes Rust the single source of truth for it.
#[test]
fn export_types() {
    let cfg = Config::new()
        .with_out_dir("../src/lib/generated")
        .with_large_int("number");
    AssetId::export_all(&cfg).expect("AssetId bindings");
    FrameRange::export_all(&cfg).expect("FrameRange bindings");
    FrameDuration::export_all(&cfg).expect("FrameDuration bindings");
    Marker::export_all(&cfg).expect("Marker bindings");
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
}
