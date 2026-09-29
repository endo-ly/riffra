//! The table of every Control Command: wire name, params, result, and policy.
//!
//! This table is the only place where commands are named. Executors match
//! the generated enums exhaustively, so a command added here without an
//! implementation fails to compile.

use super::decode::{CommandDecodeError, decode_params};
use super::params::*;
use super::policy::{CanonicalAccess, CommandExecutor, CommandPolicy, CommandScope};
use riffra_control::ControlRequest;
use riffra_core::application::{SessionInspectionQuery, SessionSettingsPatch};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

macro_rules! control_commands {
    (
        canonical {
            $(
                $canonical:ident = $canonical_name:literal ($canonical_params:ty)
                    -> $canonical_output:ident,
                    $access:ident $(($batch:ident))?,
                    $canonical_scope:ident $(($canonical_long:ident))?;
            )*
        }
        project {
            $(
                $project:ident = $project_name:literal ($project_params:ty)
                    -> $project_output:ident,
                    $project_scope:ident $(($project_long:ident))?;
            )*
        }
        runtime {
            $(
                $runtime:ident = $runtime_name:literal ($runtime_params:ty)
                    -> $runtime_output:ident,
                    $runtime_scope:ident $(($runtime_long:ident))?;
            )*
        }
    ) => {
        /// A command that reads or edits canonical state.
        #[derive(Clone, Debug, Serialize, TS)]
        #[serde(tag = "command", content = "params")]
        pub enum CanonicalCommand {
            $( #[serde(rename = $canonical_name)] $canonical($canonical_params), )*
        }

        /// A command that selects or exchanges Project containers.
        #[derive(Clone, Debug, Serialize, TS)]
        #[serde(tag = "command", content = "params")]
        pub enum ProjectCommand {
            $( #[serde(rename = $project_name)] $project($project_params), )*
        }

        /// A command that requires the runtime services of a live Host.
        #[derive(Clone, Debug, Serialize, TS)]
        #[serde(tag = "command", content = "params")]
        pub enum RuntimeCommand {
            $( #[serde(rename = $runtime_name)] $runtime($runtime_params), )*
        }

        impl ControlCommand {
            /// Decodes wire params for the command named `name`.
            ///
            /// # Errors
            ///
            /// Returns [`CommandDecodeError::UnknownCommand`] for an unknown
            /// name and [`CommandDecodeError::InvalidParams`] when the params
            /// do not match the command.
            pub fn decode(name: &str, params: Value) -> Result<Self, CommandDecodeError> {
                match name {
                    $( $canonical_name => Ok(CanonicalCommand::$canonical(decode_params(params)?).into()), )*
                    $( $project_name => Ok(ProjectCommand::$project(decode_params(params)?).into()), )*
                    $( $runtime_name => Ok(RuntimeCommand::$runtime(decode_params(params)?).into()), )*
                    _ => Err(CommandDecodeError::UnknownCommand(name.to_owned())),
                }
            }

            /// Returns the wire name of this command.
            pub fn name(&self) -> &'static str {
                match self {
                    Self::Canonical(command) => match command {
                        $( CanonicalCommand::$canonical(_) => $canonical_name, )*
                    },
                    Self::Project(command) => match command {
                        $( ProjectCommand::$project(_) => $project_name, )*
                    },
                    Self::Runtime(command) => match command {
                        $( RuntimeCommand::$runtime(_) => $runtime_name, )*
                    },
                }
            }

            /// Returns how this command is gated and executed.
            pub fn policy(&self) -> CommandPolicy {
                match self {
                    Self::Canonical(command) => match command {
                        $(
                            CanonicalCommand::$canonical(_) => CommandPolicy {
                                scope: control_commands!(@scope $canonical_scope $(($canonical_long))?),
                                executor: CommandExecutor::Canonical {
                                    access: control_commands!(@access $access $(($batch))?),
                                },
                            },
                        )*
                    },
                    Self::Project(command) => match command {
                        $(
                            ProjectCommand::$project(_) => CommandPolicy {
                                scope: control_commands!(@scope $project_scope $(($project_long))?),
                                executor: CommandExecutor::Project,
                            },
                        )*
                    },
                    Self::Runtime(command) => match command {
                        $(
                            RuntimeCommand::$runtime(_) => CommandPolicy {
                                scope: control_commands!(@scope $runtime_scope $(($runtime_long))?),
                                executor: CommandExecutor::Runtime,
                            },
                        )*
                    },
                }
            }
        }

        /// Generates the TypeScript `ControlCommandResults` map from each
        /// command name to the value type of its [`ControlOutput`](super::output::ControlOutput).
        pub fn typescript_result_map() -> String {
            let rows: &[(&str, &str)] = &[
                $( ($canonical_name, stringify!($canonical_output)), )*
                $( ($project_name, stringify!($project_output)), )*
                $( ($runtime_name, stringify!($runtime_output)), )*
            ];
            render_result_map(rows)
        }
    };
    (@access read) => { CanonicalAccess::Read };
    (@access mutation) => { CanonicalAccess::Mutation { batchable: false } };
    (@access mutation (batch)) => { CanonicalAccess::Mutation { batchable: true } };
    (@scope host) => { CommandScope::Host };
    (@scope project) => { CommandScope::Project { long_running: false } };
    (@scope project (long)) => { CommandScope::Project { long_running: true } };
}

/// Any Control Command.
///
/// On the wire every command is `{"command": ..., "params": ...}`; decoding is
/// done only by [`ControlCommand::decode`].
#[derive(Clone, Debug, Serialize, TS)]
#[serde(untagged)]
pub enum ControlCommand {
    Canonical(CanonicalCommand),
    Project(ProjectCommand),
    Runtime(RuntimeCommand),
}

impl ControlCommand {
    /// Wraps this command in a request envelope.
    pub fn into_request(
        self,
        request_id: impl Into<String>,
        expected_sequence: Option<u64>,
    ) -> ControlRequest {
        let name = self.name();
        let mut wire = serde_json::to_value(self).expect("control commands serialize");
        ControlRequest::new(request_id, name, wire["params"].take(), expected_sequence)
    }
}

impl From<CanonicalCommand> for ControlCommand {
    fn from(command: CanonicalCommand) -> Self {
        Self::Canonical(command)
    }
}

impl From<ProjectCommand> for ControlCommand {
    fn from(command: ProjectCommand) -> Self {
        Self::Project(command)
    }
}

impl From<RuntimeCommand> for ControlCommand {
    fn from(command: RuntimeCommand) -> Self {
        Self::Runtime(command)
    }
}

fn render_result_map(rows: &[(&str, &str)]) -> String {
    let mut map = String::from(
        "// This file was generated by `typescript_result_map`. Do not edit this file manually.\n\
         import type { ControlOutput } from './ControlOutput';\n\n\
         type OutputValue<T extends ControlOutput['type']> = Extract<ControlOutput, { type: T }>['value'];\n\n\
         export type ControlCommandResults = {\n",
    );
    for (name, output) in rows {
        let mut variant = output.chars();
        let tag = variant
            .next()
            .map(|first| first.to_ascii_lowercase().to_string() + variant.as_str())
            .unwrap_or_default();
        map.push_str(&format!("  '{name}': OutputValue<'{tag}'>;\n"));
    }
    map.push_str("};\n");
    map
}

control_commands! {
    canonical {
        SessionGet = "session.get" (EmptyParams) -> Session, read, project;
        SessionInspect = "session.inspect" (SessionInspectionQuery) -> SessionInspection, read, project;
        SessionApply = "session.apply" (SessionApplyParams) -> BatchMutation, mutation, project;
        SessionSettingsUpdate = "session.settings.update" (SessionSettingsPatch) -> ArrangementMutation, mutation(batch), project;
        HistoryGet = "history.get" (EmptyParams) -> History, read, project;
        Undo = "undo" (EmptyParams) -> ArrangementMutation, mutation, project;
        Redo = "redo" (EmptyParams) -> ArrangementMutation, mutation, project;
        MasterGainSet = "audio.master-gain.set" (MasterGainParams) -> ArrangementMutation, mutation, project;

        TrackList = "track.list" (EmptyParams) -> Tracks, read, project;
        TrackAdd = "track.add" (TrackAddParams) -> ArrangementMutation, mutation(batch), project;
        TrackUpdate = "track.update" (TrackUpdateParams) -> ArrangementMutation, mutation(batch), project;
        TrackRemove = "track.remove" (TrackIdParams) -> ArrangementMutation, mutation(batch), project;
        TrackDuplicate = "track.duplicate" (TrackIdParams) -> ArrangementMutation, mutation(batch), project;
        TrackReorder = "track.reorder" (TrackReorderParams) -> ArrangementMutation, mutation(batch), project;
        TrackAudioInputSet = "track.audio-input.set" (AudioInputParams) -> ArrangementMutation, mutation(batch), project;
        TrackAudioInputClear = "track.audio-input.clear" (TrackIdParams) -> ArrangementMutation, mutation(batch), project;
        TrackMidiInputSet = "track.midi-input.set" (MidiInputParams) -> ArrangementMutation, mutation(batch), project;
        TrackMidiInputClear = "track.midi-input.clear" (TrackIdParams) -> ArrangementMutation, mutation(batch), project;
        MarkerAdd = "marker.add" (MarkerAddParams) -> ArrangementMutation, mutation(batch), project;
        MarkerUpdate = "marker.update" (MarkerUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MarkerRemove = "marker.remove" (MarkerIdParams) -> ArrangementMutation, mutation(batch), project;
        TimebaseUpdate = "timebase.update" (TimebaseUpdateParams) -> ArrangementMutation, mutation(batch), project;
        LoopRangeSet = "loop-range.set" (RangeParams) -> ArrangementMutation, mutation(batch), project;
        PunchRangeSet = "punch-range.set" (RangeParams) -> ArrangementMutation, mutation(batch), project;
        AutomationSet = "automation.set" (AutomationSetParams) -> ArrangementMutation, mutation(batch), project;
        AutomationClear = "automation.clear" (AutomationClearParams) -> ArrangementMutation, mutation(batch), project;

        AudioClipList = "audio-clip.list" (EmptyParams) -> AudioClips, read, project;
        AudioClipAddAsset = "audio-clip.add-asset" (ClipAddAssetParams) -> ArrangementMutation, mutation, project;
        AudioClipUpdate = "audio-clip.update" (AudioClipUpdateParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipMove = "audio-clip.move" (AudioClipMoveParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipTrim = "audio-clip.trim" (AudioClipTrimParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipSplit = "audio-clip.split" (ClipSplitParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipDuplicate = "audio-clip.duplicate" (ClipIdParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipCrossfade = "audio-clip.crossfade" (AudioClipCrossfadeParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipList = "midi-clip.list" (EmptyParams) -> MidiClips, read, project;
        MidiClipCreate = "midi-clip.create" (MidiClipCreateParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipAddAsset = "midi-clip.add-asset" (ClipAddAssetParams) -> ArrangementMutation, mutation, project;
        MidiClipUpdate = "midi-clip.update" (MidiClipUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipMove = "midi-clip.move" (MidiClipMoveParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipTrim = "midi-clip.trim" (MidiClipTrimParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipSplit = "midi-clip.split" (ClipSplitParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipDuplicate = "midi-clip.duplicate" (ClipIdParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteAdd = "midi-note.add" (MidiNoteAddParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteInsert = "midi-note.insert" (MidiNoteInsertParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteUpdate = "midi-note.update" (MidiNoteUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteUpdateMany = "midi-note.update-many" (MidiNoteUpdateManyParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteRemove = "midi-note.remove" (NoteIdParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteRemoveMany = "midi-note.remove-many" (NoteIdsParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteClear = "midi-note.clear" (ClipIdParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteQuantize = "midi-note.quantize" (MidiNoteQuantizeParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteTransform = "midi-note.transform" (MidiNoteTransformParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteDuplicate = "midi-note.duplicate" (MidiNoteDuplicateParams) -> ArrangementMutation, mutation(batch), project;
        ClipRemove = "clip.remove" (ClipRemoveParams) -> ArrangementMutation, mutation(batch), project;
        ClipPaste = "clip.paste" (ClipPasteParams) -> ArrangementMutation, mutation(batch), project;

        MusicMidiClipCreate = "music.midi-clip.create" (MusicalMidiClipCreateParams) -> ArrangementMutation, mutation(batch), project;
        MusicMidiClipResize = "music.midi-clip.resize" (MusicalMidiClipResizeParams) -> ArrangementMutation, mutation(batch), project;
        MusicNoteInsert = "music.note.insert" (MusicalNoteInsertParams) -> ArrangementMutation, mutation(batch), project;
        MusicNoteList = "music.note.list" (MusicalNoteListParams) -> MusicNotes, read, project;
        MusicNoteGet = "music.note.get" (NoteIdParams) -> MusicNote, read, project;
        MusicNoteUpdate = "music.note.update" (MusicalNoteUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MusicNoteRemove = "music.note.remove" (NoteIdParams) -> ArrangementMutation, mutation(batch), project;
        MusicNoteTransform = "music.note.transform" (MusicalNoteTransformParams) -> ArrangementMutation, mutation(batch), project;
        MusicHarmonyResolve = "music.harmony.resolve" (HarmonyResolveParams) -> HarmonyChord, read, project;
        MusicHarmonyList = "music.harmony.list" (EmptyParams) -> HarmonyEvents, read, project;
        MusicHarmonyInsert = "music.harmony.insert" (HarmonyInsertParams) -> ArrangementMutation, mutation(batch), project;
        MusicHarmonyUpdate = "music.harmony.update" (HarmonyUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MusicHarmonyRemove = "music.harmony.remove" (HarmonyRemoveParams) -> ArrangementMutation, mutation(batch), project;
        MusicHarmonyRealize = "music.harmony.realize" (HarmonyRealizeParams) -> ArrangementMutation, mutation(batch), project;
        MusicPhraseInsert = "music.phrase.insert" (PhraseInsertParams) -> ArrangementMutation, mutation(batch), project;
        MusicPhrasePreview = "music.phrase.preview" (PhrasePreviewParams) -> PhrasePreview, read, project;
        MusicRegionList = "music.region.list" (EmptyParams) -> Regions, read, project;
        MusicRegionAdd = "music.region.add" (RegionAddParams) -> ArrangementMutation, mutation(batch), project;
        MusicRegionUpdate = "music.region.update" (RegionUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MusicRegionRemove = "music.region.remove" (RegionIdParams) -> ArrangementMutation, mutation(batch), project;

        AssetImportMidi = "asset.import-midi" (AssetImportParams) -> AssetId, read, project;
        InstrumentList = "instrument.list" (EmptyParams) -> InstrumentLibrary, read, host;
        InstrumentSave = "instrument.save" (InstrumentSaveParams) -> InstrumentLibraryItem, read, host;
        InstrumentExport = "instrument.export" (InstrumentExportParams) -> InstrumentExport, read, host;
        InstrumentApply = "instrument.apply" (InstrumentApplyParams) -> ArrangementMutation, mutation(batch), project(long);
        InstrumentVst3Set = "instrument.vst3.set" (PluginPathParams) -> ArrangementMutation, mutation, project(long);
        InstrumentClear = "instrument.clear" (TrackIdParams) -> ArrangementMutation, mutation(batch), project;
        EffectAdd = "effect.add" (PluginPathParams) -> ArrangementMutation, mutation(batch), project(long);
        EffectRemove = "effect.remove" (TrackDeviceParams) -> ArrangementMutation, mutation(batch), project;
        EffectReorder = "effect.reorder" (EffectReorderParams) -> ArrangementMutation, mutation(batch), project;
        DeviceBypass = "device.bypass" (DeviceBypassParams) -> ArrangementMutation, mutation(batch), project;
        DeviceParameterSet = "device.parameter.set" (DeviceParameterSetParams) -> ArrangementMutation, mutation(batch), project;
        MissingRelink = "missing.relink" (MissingRelinkParams) -> ArrangementMutation, mutation, project;
        MissingDisablePlugin = "missing.disable-plugin" (DeviceIdParams) -> ArrangementMutation, mutation, project;
        MissingReplacePlugin = "missing.replace-plugin" (MissingPluginReplaceParams) -> ArrangementMutation, mutation, project(long);
    }
    project {
        ProjectList = "project.list" (EmptyParams) -> ProjectState, host;
        ProjectCreate = "project.create" (ProjectCreateParams) -> ProjectActivation, project;
        ProjectOpen = "project.open" (ProjectOpenParams) -> ProjectActivation, project;
        ProjectRename = "project.rename" (ProjectRenameParams) -> ProjectState, project;
        ProjectImport = "project.import" (ProjectImportParams) -> ProjectActivation, project;
        ProjectExport = "project.export" (ProjectExportParams) -> ProjectExport, project;
    }
    runtime {
        HostStatus = "host.status" (EmptyParams) -> HostStatus, host;
        HostInfo = "host.info" (EmptyParams) -> HostInfo, host;
        HostBootstrap = "host.bootstrap" (EmptyParams) -> HostBootstrap, host;
        HostShutdown = "host.shutdown" (EmptyParams) -> Ok, host;

        RuntimeProjectionGet = "runtime.projection.get" (EmptyParams) -> RuntimeProjection, project;
        RuntimeProjectionRetry = "runtime.projection.retry" (EmptyParams) -> RuntimeProjection, project;
        TransportPlay = "transport.play" (EmptyParams) -> Ok, project;
        TransportStop = "transport.stop" (EmptyParams) -> Ok, project;
        TransportGoToStart = "transport.go-to-start" (EmptyParams) -> Ok, project;
        TransportSeek = "transport.seek" (SeekParams) -> Ok, project;
        MasterGainPreview = "audio.master-gain.preview" (MasterGainParams) -> Ok, project;
        TrackMixPreview = "track.mix.preview" (TrackMixParams) -> Ok, project;

        AudioStatus = "audio.status" (EmptyParams) -> AudioStatus, host;
        AudioDiagnostics = "audio.diagnostics" (AudioDiagnosticsParams) -> AudioDiagnostics, host;
        AudioProbe = "audio.probe" (EmptyParams) -> AudioProbe, host;
        AudioChannelsProbe = "audio.channels.probe" (AudioChannelsProbeParams) -> DeviceChannels, host;
        AudioRecover = "audio.recover" (EmptyParams) -> AudioStatus, host;
        AudioStartupRetry = "audio.startup.retry" (EmptyParams) -> AudioStatus, host;
        AudioDriverGet = "audio.driver.get" (EmptyParams) -> AudioDriver, host;
        AudioDriverSet = "audio.driver.set" (AudioDriverConfig) -> AudioStatus, host;
        EmergencyMute = "audio.emergency-mute" (MuteParams) -> AudioStatus, host;
        FeedbackProtectionReset = "audio.feedback-protection.reset" (EmptyParams) -> AudioStatus, project;
        AssetPreview = "asset.preview" (AssetPreviewParams) -> AudioStatus, host;
        AssetPreviewStop = "asset.preview.stop" (EmptyParams) -> AudioStatus, host;
        InstrumentPreview = "instrument.preview" (InstrumentIdParams) -> AudioStatus, host;
        InstrumentPreviewStop = "instrument.preview.stop" (EmptyParams) -> AudioStatus, host;

        MidiListeningEnable = "midi.listening.enable" (EmptyParams) -> AudioStatus, host;
        MidiListeningDisable = "midi.listening.disable" (EmptyParams) -> AudioStatus, host;
        MidiSend = "midi.send" (MidiSendParams) -> Ok, project;
        MidiTargetSet = "midi.target.set" (LiveMidiTargetParams) -> Ok, project;
        MidiPanic = "midi.panic" (TrackIdParams) -> Ok, project;

        PluginCatalogList = "plugin.catalog.list" (EmptyParams) -> Plugins, host;
        PluginScan = "plugin.scan" (PluginScanParams) -> PluginScan, host;
        PluginScanStart = "plugin.scan.start" (PluginScanParams) -> Job, host;
        PluginEditorOpen = "plugin.editor.open" (TrackDeviceParams) -> Ok, project;
        DeviceInspect = "device.inspect" (TrackDeviceParams) -> DeviceInspection, project;
        DeviceParameterList = "device.parameter.list" (TrackDeviceParams) -> DeviceParameters, project;
        DeviceParameterGet = "device.parameter.get" (DeviceParameterGetParams) -> DeviceParameter, project;
        PluginPresetList = "plugin.preset.list" (TrackDeviceParams) -> PluginPresets, project;
        PluginPresetGet = "plugin.preset.get" (TrackDeviceParams) -> PluginPreset, project;
        PluginPresetSet = "plugin.preset.set" (PluginPresetSetParams) -> ArrangementMutation, project;
        PluginStateGet = "plugin.state.get" (TrackDeviceParams) -> PluginState, project;
        PluginStateSet = "plugin.state.set" (PluginStateSetParams) -> ArrangementMutation, project;
        PluginStatePersist = "plugin.state.persist" (PluginStatePersistParams) -> ArrangementMutation, project;
        PluginParameterPersist = "plugin.parameter.persist" (PluginParameterPersistParams) -> ArrangementMutation, project;
        MissingList = "missing.list" (EmptyParams) -> Missing, project;

        RecordStart = "record.start" (RecordStartParams) -> AudioStatus, project;
        RecordStop = "record.stop" (EmptyParams) -> RecordingStop, project;
        RecordStatus = "record.status" (EmptyParams) -> AudioStatus, project;
        RecordList = "record.list" (RecordListParams) -> Recordings, project;
        RecordRename = "record.rename" (RecordRenameParams) -> RecordingId, project;
        RecordArchive = "record.archive" (IdParams) -> RecordingId, project;
        RecordPromote = "record.promote" (IdParams) -> RecordingId, project;
        RecordTag = "record.tag" (LibraryTagParams) -> LibraryAsset, project;
        RecordDelete = "record.delete" (IdParams) -> Ok, project;
        RecordDuplicates = "record.duplicates" (EmptyParams) -> RecordingDuplicates, project;
        TakeActivate = "take.activate" (TakeActivateParams) -> ArrangementMutation, project;
        TakePlaceSeparateClip = "take.place-separate-clip" (TakeIdParams) -> ArrangementMutation, project;
        TakeVariantSet = "audio-clip.take-variant.set" (TakeVariantParams) -> ArrangementMutation, project;
        TakeComparisonStart = "take.comparison.start" (TakeIdParams) -> AudioStatus, project;
        TakeComparisonSwitch = "take.comparison.switch" (TakeComparisonParams) -> AudioStatus, project;
        TakeComparisonStop = "take.comparison.stop" (EmptyParams) -> AudioStatus, project;
        ProjectRestoreGeneration = "project.restore-generation" (ProjectRestoreParams) -> ArrangementMutation, project;

        RenderStart = "render.start" (RenderStartParams) -> Job, project;
        JobGet = "job.get" (IdParams) -> Job, host;
        JobCancel = "job.cancel" (IdParams) -> Job, host;
        AnalysisStart = "analysis.start" (AnalysisParams) -> Analysis, host;

        LibrarySearch = "library.search" (LibrarySearchParams) -> Library, host;
        LibraryAssetUpdate = "library.asset.update" (LibraryTagParams) -> LibraryAsset, host;
        LibraryRelated = "library.related" (IdParams) -> Library, host;
        LibraryInstrumentList = "library.instrument.list" (EmptyParams) -> InstrumentLibrary, host;
        LibraryInstrumentFavoriteSet = "library.instrument.favorite.set" (InstrumentFavoriteParams) -> InstrumentLibraryItem, host;
        LibraryInstrumentCategorySet = "library.instrument.category.set" (InstrumentCategoryParams) -> InstrumentLibraryItem, host;
        LibraryInstrumentTagsSet = "library.instrument.tags.set" (InstrumentTagsParams) -> InstrumentLibraryItem, host;
        LibraryInstrumentCollectionList = "library.instrument.collection.list" (EmptyParams) -> InstrumentCollections, host;
        LibraryInstrumentCollectionCreate = "library.instrument.collection.create" (InstrumentCollectionCreateParams) -> InstrumentCollection, host;
        LibraryInstrumentCollectionRename = "library.instrument.collection.rename" (InstrumentCollectionRenameParams) -> InstrumentCollection, host;
        LibraryInstrumentCollectionDelete = "library.instrument.collection.delete" (InstrumentCollectionIdParams) -> Ok, host;
        LibraryInstrumentCollectionMembershipSet = "library.instrument.collection.membership.set" (InstrumentCollectionMembershipParams) -> InstrumentLibraryItem, host;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scope_follows_active_project_dependency() {
        // Arrange
        let cases = [
            ("project.list", CommandScope::Host),
            ("instrument.list", CommandScope::Host),
            ("audio.emergency-mute", CommandScope::Host),
            ("instrument.preview", CommandScope::Host),
            (
                "session.get",
                CommandScope::Project {
                    long_running: false,
                },
            ),
            (
                "transport.play",
                CommandScope::Project {
                    long_running: false,
                },
            ),
            ("effect.add", CommandScope::Project { long_running: true }),
        ];
        let params = |name: &str| match name {
            "audio.emergency-mute" => json!({"muted": true}),
            "instrument.preview" => json!({"instrumentId": "builtin:bass"}),
            "effect.add" => json!({"trackId": "track:1", "pluginPath": "Reverb.vst3"}),
            _ => json!({}),
        };

        for (name, scope) in cases {
            // Act
            let policy = ControlCommand::decode(name, params(name)).unwrap().policy();

            // Assert
            assert_eq!(policy.scope, scope, "{name}");
        }
    }

    #[test]
    fn unknown_command_name_is_rejected() {
        // Act
        let error = ControlCommand::decode("track.rename", json!({})).unwrap_err();

        // Assert
        assert!(
            matches!(error, CommandDecodeError::UnknownCommand(name) if name == "track.rename")
        );
    }

    #[test]
    fn unknown_params_key_is_rejected_with_its_path() {
        // Act
        let error = ControlCommand::decode(
            "track.update",
            json!({"trackId": "track:1", "trackNmae": "Lead"}),
        )
        .unwrap_err();

        // Assert
        assert!(error.to_string().contains("unknown field `trackNmae`"));
        assert_eq!(error.details().unwrap()["path"], "/trackNmae");
    }

    #[test]
    fn mistyped_array_element_reports_path_index_and_value() {
        // Act
        let error = ControlCommand::decode(
            "music.note.insert",
            json!({"clipId": "midi-clip:1", "notes": [{"pitch": "C4", "position": "1:1", "duration": "1/8", "velocity": "loud"}]}),
        )
        .unwrap_err();

        // Assert
        let details = error.details().unwrap();
        assert_eq!(details["path"], "/notes/0/velocity");
        assert_eq!(details["index"], 0);
        assert_eq!(details["value"], "loud");
    }
}
