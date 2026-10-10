//! Typed results of Control Commands.
//!
//! On the wire a result is `{"type": ..., "value": ...}`; the variant name in
//! camelCase is the `type`.

mod audio;
mod device;
mod host;
mod job;
mod library;
mod project;
mod recording;
mod session;

pub use audio::*;
pub use device::*;
pub use host::*;
pub use job::*;
pub use library::*;
pub use project::*;
pub use recording::*;
pub use session::*;

use super::params::AudioDriverConfig;
use riffra_control::CommandResult;
use riffra_core::application::{
    MusicalHarmonyEventView, MusicalMidiNoteView, MusicalNoteListView, MusicalRegionView,
    SessionInspection,
};
use riffra_core::{AssetId, AudioClip, CreativeSession, HarmonyChord, HistoryState, MidiClip};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The result of one Control Command.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ControlOutput {
    Session(CreativeSession),
    Timebase(riffra_core::ProjectTimebase),
    Mixdown(riffra_core::MixdownSettings),
    InstrumentEvents(Vec<riffra_core::InstrumentControlEvent>),
    SessionInspection(SessionInspection),
    History(HistoryState),
    ArrangementMutation(ArrangementMutationResult),
    BatchMutation(BatchMutationResult),
    Tracks(Vec<TrackSummary>),
    AudioClips(Vec<AudioClip>),
    MidiClips(Vec<MidiClip>),
    MusicNotes(MusicalNoteListView),
    MusicNote(MusicalMidiNoteView),
    HarmonyChord(HarmonyChord),
    HarmonyEvents(Vec<MusicalHarmonyEventView>),
    PhrasePreview(PhrasePreview),
    Regions(Vec<MusicalRegionView>),
    AssetId(AssetId),
    ProjectState(ProjectState),
    ProjectActivation(ProjectActivationResult),
    ProjectExport(ProjectExport),
    InstrumentLibrary(Vec<InstrumentLibraryItem>),
    InstrumentLibraryItem(InstrumentLibraryItem),
    InstrumentCategories(Vec<String>),
    InstrumentExport(InstrumentExport),
    InstrumentCollections(Vec<InstrumentCollection>),
    InstrumentCollection(InstrumentCollection),
    DeviceInspection(DeviceInspection),
    DeviceParameters(Vec<DeviceParameterInfo>),
    DeviceParameter(DeviceParameterInfo),
    PluginState(PluginStateSnapshot),
    PluginPresets(Vec<PluginPresetInfo>),
    PluginPreset(PluginPresetInfo),
    Plugins(Vec<PluginEntry>),
    PluginScan(ScanReport),
    Missing(Vec<MissingDependency>),
    HostStatus(HostStatus),
    HostInfo(HostInfo),
    HostBootstrap(Box<HostBootstrap>),
    AudioStatus(Box<AudioStatus>),
    AudioDiagnostics(AudioDiagnosticsReport),
    AudioProbe(AudioDeviceProbe),
    DeviceChannels(DeviceChannels),
    AudioDriver(AudioDriverConfig),
    RuntimeProjection(RuntimeProjectionStatus),
    RecordingStop(Box<RecordingStopResult>),
    Recordings(Vec<RecordingAsset>),
    RecordingId(String),
    RecordingDuplicates(Vec<Vec<String>>),
    Job(Option<BackgroundJobStatus>),
    Library(Vec<LibraryAsset>),
    LibraryAsset(LibraryAsset),
    Analysis(AudioAnalysis),
    Ok(()),
}

impl From<ControlOutput> for CommandResult {
    fn from(output: ControlOutput) -> Self {
        let value = serde_json::to_value(output).expect("control outputs serialize");
        serde_json::from_value(value).expect("control outputs are tagged command results")
    }
}

impl TryFrom<CommandResult> for ControlOutput {
    type Error = serde_json::Error;

    fn try_from(result: CommandResult) -> Result<Self, Self::Error> {
        serde_json::from_value(serde_json::to_value(result)?)
    }
}
