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

        impl CanonicalCommand {
            pub(crate) fn access(&self) -> CanonicalAccess {
                match self {
                    $( Self::$canonical(_) => control_commands!(@access $access $(($batch))?), )*
                }
            }
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

        /// Returns every Control Command wire name in table order.
        pub fn command_names() -> &'static [&'static str] {
            &[
                $( $canonical_name, )*
                $( $project_name, )*
                $( $runtime_name, )*
            ]
        }

        /// Generates the params schema for one Control Command into `generator`.
        ///
        /// Returns `None` for an unknown command. Sharing one generator across
        /// several commands lets their `$defs` coexist at a single root.
        pub fn command_params_subschema(
            name: &str,
            generator: &mut schemars::SchemaGenerator,
        ) -> Option<schemars::Schema> {
            match name {
                $( $canonical_name => Some(generator.subschema_for::<$canonical_params>()), )*
                $( $project_name => Some(generator.subschema_for::<$project_params>()), )*
                $( $runtime_name => Some(generator.subschema_for::<$runtime_params>()), )*
                _ => None,
            }
        }

        /// Generates a standalone params schema for one Control Command.
        ///
        /// Returns `None` for an unknown command.
        pub fn command_params_schema(name: &str) -> Option<schemars::Schema> {
            let mut generator = schemars::SchemaGenerator::default();
            let mut schema = command_params_subschema(name, &mut generator)?;
            let definitions = generator.take_definitions(false);
            if !definitions.is_empty() {
                schema
                    .ensure_object()
                    .insert("$defs".into(), serde_json::Value::Object(definitions));
            }
            Some(schema)
        }

        /// Returns the names of canonical commands accepted by `session.apply`.
        pub fn batchable_command_names() -> impl Iterator<Item = &'static str> {
            [
                $( control_commands!(@batchable_name $canonical_name $access $(($batch))?), )*
            ]
            .into_iter()
            .flatten()
        }

        /// Reports whether a command accepts an omitted `params` object.
        ///
        /// Returns `None` for an unknown command.
        pub fn command_params_allow_omitted(name: &str) -> Option<bool> {
            match name {
                $(
                    $canonical_name => Some(
                        std::any::TypeId::of::<$canonical_params>()
                            == std::any::TypeId::of::<EmptyParams>(),
                    ),
                )*
                $(
                    $project_name => Some(
                        std::any::TypeId::of::<$project_params>()
                            == std::any::TypeId::of::<EmptyParams>(),
                    ),
                )*
                $(
                    $runtime_name => Some(
                        std::any::TypeId::of::<$runtime_params>()
                            == std::any::TypeId::of::<EmptyParams>(),
                    ),
                )*
                _ => None,
            }
        }
    };
    (@access read) => { CanonicalAccess::Read };
    (@access mutation) => { CanonicalAccess::Mutation { batchable: false } };
    (@access mutation (batch)) => { CanonicalAccess::Mutation { batchable: true } };
    (@batchable_name $name:literal mutation (batch)) => { Some($name) };
    (@batchable_name $name:literal $access:ident) => { None };
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
        Undo = "history.undo" (EmptyParams) -> ArrangementMutation, mutation, project;
        Redo = "history.redo" (EmptyParams) -> ArrangementMutation, mutation, project;
        MasterGainSet = "audio.master-gain.set" (MasterGainParams) -> ArrangementMutation, mutation, project;

        MixerGet = "mixer.get" (EmptyParams) -> Mixer, read, project;
        MixBusList = "mixer.bus.list" (EmptyParams) -> MixBuses, read, project;
        MixBusAdd = "mixer.bus.add" (MixBusAddParams) -> ArrangementMutation, mutation(batch), project;
        MixBusUpdate = "mixer.bus.update" (MixBusUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MixBusRemove = "mixer.bus.remove" (MixBusIdParams) -> ArrangementMutation, mutation(batch), project;
        TrackOutputSet = "mixer.output.set" (TrackOutputParams) -> ArrangementMutation, mutation(batch), project;
        SendList = "mixer.send.list" (EmptyParams) -> Sends, read, project;
        SendAdd = "mixer.send.add" (SendAddParams) -> ArrangementMutation, mutation(batch), project;
        SendUpdate = "mixer.send.update" (SendUpdateParams) -> ArrangementMutation, mutation(batch), project;
        SendRemove = "mixer.send.remove" (SendIdParams) -> ArrangementMutation, mutation(batch), project;
        SidechainSendList = "mixer.sidechain.list" (EmptyParams) -> SidechainSends, read, project;
        SidechainSendAdd = "mixer.sidechain.add" (SidechainSendAddParams) -> ArrangementMutation, mutation(batch), project;
        SidechainSendUpdate = "mixer.sidechain.update" (SidechainSendUpdateParams) -> ArrangementMutation, mutation(batch), project;
        SidechainSendRemove = "mixer.sidechain.remove" (SidechainSendIdParams) -> ArrangementMutation, mutation(batch), project;
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
        MarkerAdd = "music.marker.add" (MarkerAddParams) -> ArrangementMutation, mutation(batch), project;
        MarkerUpdate = "music.marker.update" (MarkerUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MarkerRemove = "music.marker.remove" (MarkerIdParams) -> ArrangementMutation, mutation(batch), project;
        TimebaseUpdate = "music.timebase.update" (TimebaseUpdateParams) -> ArrangementMutation, mutation(batch), project;
        TimebaseGetMap = "music.timebase.get-map" (EmptyParams) -> Timebase, read, project;
        MixdownGet = "render.settings.get" (EmptyParams) -> Mixdown, read, project;
        MixdownSet = "render.settings.set" (riffra_core::MixdownSettings) -> ArrangementMutation, mutation(batch), project;
        TrackExternalAudioInputSet = "track.external-audio-input.set" (TrackExternalAudioInputSetParams) -> ArrangementMutation, mutation(batch), project;
        TrackExternalAudioInputClear = "track.external-audio-input.clear" (TrackIdParams) -> ArrangementMutation, mutation(batch), project;
        InstrumentEventList = "clip.midi.instrument-event.list" (InstrumentEventClipParams) -> InstrumentEvents, read, project;
        InstrumentEventAdd = "clip.midi.instrument-event.add" (InstrumentEventAddParams) -> ArrangementMutation, mutation(batch), project;
        InstrumentEventSet = "clip.midi.instrument-event.set" (InstrumentEventSetParams) -> ArrangementMutation, mutation(batch), project;
        InstrumentEventUpdate = "clip.midi.instrument-event.update" (InstrumentEventUpdateParams) -> ArrangementMutation, mutation(batch), project;
        InstrumentEventRemove = "clip.midi.instrument-event.remove" (InstrumentEventRemoveParams) -> ArrangementMutation, mutation(batch), project;
        TimebaseSetMap = "music.timebase.set-map" (TimebaseSetMapParams) -> ArrangementMutation, mutation(batch), project;
        LoopRangeSet = "transport.loop.set" (RangeParams) -> ArrangementMutation, mutation(batch), project;
        PunchRangeSet = "record.punch.set" (RangeParams) -> ArrangementMutation, mutation(batch), project;
        AutomationList = "automation.list" (AutomationListParams) -> AutomationLanes, read, project;
        AutomationGet = "automation.get" (AutomationGetParams) -> AutomationLanes, read, project;
        AutomationSet = "automation.set" (AutomationSetParams) -> ArrangementMutation, mutation(batch), project;
        AutomationClear = "automation.clear" (AutomationClearParams) -> ArrangementMutation, mutation(batch), project;

        AudioClipList = "clip.audio.list" (EmptyParams) -> AudioClips, read, project;
        AudioClipAddAsset = "clip.audio.add-asset" (ClipAddAssetParams) -> ArrangementMutation, mutation, project;
        AudioClipUpdate = "clip.audio.update" (AudioClipUpdateParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipMove = "clip.audio.move" (AudioClipMoveParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipTrim = "clip.audio.trim" (AudioClipTrimParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipSplit = "clip.audio.split" (ClipSplitParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipDuplicate = "clip.audio.duplicate" (ClipIdParams) -> ArrangementMutation, mutation(batch), project;
        AudioClipCrossfade = "clip.audio.crossfade" (AudioClipCrossfadeParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipList = "clip.midi.list" (EmptyParams) -> MidiClips, read, project;
        MidiClipCreate = "clip.midi.create" (MidiClipCreateParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipAddAsset = "clip.midi.add-asset" (ClipAddAssetParams) -> ArrangementMutation, mutation, project;
        MidiClipUpdate = "clip.midi.update" (MidiClipUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipMove = "clip.midi.move" (MidiClipMoveParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipTrim = "clip.midi.trim" (MidiClipTrimParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipMerge = "clip.midi.merge" (ClipMergeParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipSplit = "clip.midi.split" (ClipSplitParams) -> ArrangementMutation, mutation(batch), project;
        MidiClipDuplicate = "clip.midi.duplicate" (ClipIdParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteAdd = "clip.midi.note.add" (MidiNoteAddParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteInsert = "clip.midi.note.insert" (MidiNoteInsertParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteUpdate = "clip.midi.note.update" (MidiNoteUpdateParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteUpdateMany = "clip.midi.note.update-many" (MidiNoteUpdateManyParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteRemoveMany = "clip.midi.note.remove-many" (NoteIdsParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteClear = "clip.midi.note.clear" (ClipIdParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteQuantize = "clip.midi.note.quantize" (MidiNoteQuantizeParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteTransform = "clip.midi.note.transform" (MidiNoteTransformParams) -> ArrangementMutation, mutation(batch), project;
        MidiNoteDuplicate = "clip.midi.note.duplicate" (MidiNoteDuplicateParams) -> ArrangementMutation, mutation(batch), project;
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
        InstrumentApply = "instrument.apply" (InstrumentApplyParams) -> ArrangementMutation, mutation(batch), project;
        InstrumentMidiGet = "instrument.midi.get" (TrackIdParams) -> InstrumentMidi, read, project;
        InstrumentMidiSet = "instrument.midi.set" (InstrumentMidiSetParams) -> ArrangementMutation, mutation(batch), project;
        InstrumentVst3Set = "instrument.vst3.set" (PluginPathParams) -> ArrangementMutation, mutation(batch), project(long);
        PluginStateImport = "plugin.state.import" (PluginStateImportParams) -> ArrangementMutation, mutation(batch), project;
        InstrumentClear = "instrument.clear" (TrackIdParams) -> ArrangementMutation, mutation(batch), project;
        EffectAdd = "effect.add" (EffectAddParams) -> ArrangementMutation, mutation(batch), project(long);
        EffectRemove = "effect.remove" (DeviceParams) -> ArrangementMutation, mutation(batch), project;
        EffectReorder = "effect.reorder" (EffectReorderParams) -> ArrangementMutation, mutation(batch), project;
        DeviceBypass = "device.bypass" (DeviceBypassParams) -> ArrangementMutation, mutation(batch), project;
        DeviceParameterSet = "device.parameter.set" (DeviceParameterSetParams) -> DeviceParameterMutation, mutation(batch), project;
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
        ProjectImportScore = "project.import-score" (ProjectImportScoreParams) -> ProjectActivation, project;
        MusicExportMidi = "music.export-midi" (MidiExportParams) -> MidiExport, project;
        ProjectExport = "project.export" (ProjectExportParams) -> ProjectExport, project;
    }
    runtime {
        HostStatus = "host.status" (EmptyParams) -> HostStatus, host;
        HostInfo = "host.info" (EmptyParams) -> HostInfo, host;
        HostBootstrap = "host.bootstrap" (EmptyParams) -> HostBootstrap, host;
        HostShutdown = "host.shutdown" (EmptyParams) -> Ok, host;

        RuntimeProjectionGet = "audio.projection.get" (EmptyParams) -> RuntimeProjection, project;
        RuntimeProjectionRetry = "audio.projection.retry" (EmptyParams) -> RuntimeProjection, project;
        TransportPlay = "transport.play" (EmptyParams) -> Ok, project;
        TransportPause = "transport.pause" (EmptyParams) -> Ok, project;
        TransportStop = "transport.stop" (EmptyParams) -> Ok, project;
        TransportGoToStart = "transport.go-to-start" (EmptyParams) -> Ok, project;
        TransportSeek = "transport.seek" (SeekParams) -> Ok, project;
        MasterGainPreview = "audio.master-gain.preview" (MasterGainParams) -> Ok, project;
        SendLevelPreview = "mixer.send.preview" (SendLevelPreviewParams) -> Ok, project;
        SidechainSendLevelPreview = "mixer.sidechain.preview" (SidechainSendLevelPreviewParams) -> Ok, project;
        ChannelMixPreview = "mixer.channel.preview" (ChannelMixParams) -> Ok, project;
        AutomationPreview = "automation.preview" (AutomationPreviewParams) -> Ok, project;

        AudioStatus = "audio.status" (EmptyParams) -> AudioStatus, host;
        AudioDiagnostics = "audio.diagnostics" (AudioDiagnosticsParams) -> AudioDiagnostics, host;
        AudioProbe = "audio.probe" (EmptyParams) -> AudioProbe, host;
        AudioChannelsProbe = "audio.probe-channels" (AudioChannelsProbeParams) -> DeviceChannels, host;
        AudioRecover = "audio.recover" (EmptyParams) -> AudioStatus, host;
        AudioStartupRetry = "audio.startup-retry" (EmptyParams) -> AudioStatus, host;
        AudioDriverGet = "audio.driver.get" (EmptyParams) -> AudioDriver, host;
        AudioDriverSet = "audio.driver.set" (AudioDriverConfig) -> AudioStatus, host;
        EmergencyMute = "audio.emergency-mute" (MuteParams) -> AudioStatus, host;
        FeedbackProtectionReset = "audio.feedback-protection.reset" (EmptyParams) -> AudioStatus, project;
        AssetPreview = "asset.preview" (AssetPreviewParams) -> AudioStatus, host;
        AssetPreviewStop = "asset.stop-preview" (EmptyParams) -> AudioStatus, host;
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
        PluginEditorOpen = "plugin.editor.open" (DeviceParams) -> Ok, project;
        PluginAuditionOpen = "plugin.audition.open" (PluginAuditionParams) -> AudioStatus, host;
        DeviceInspect = "device.inspect" (DeviceParams) -> DeviceInspection, project;
        DeviceParameterList = "device.parameter.list" (DeviceParams) -> DeviceParameters, project;
        DeviceParameterGet = "device.parameter.get" (DeviceParameterGetParams) -> DeviceParameter, project;
        DeviceParameterFormat = "device.parameter.format" (DeviceParameterFormatParams) -> DeviceParameterText, project;
        PluginPresetList = "plugin.preset.list" (DeviceParams) -> PluginPresets, project;
        PluginPresetGet = "plugin.preset.get" (DeviceParams) -> PluginPreset, project;
        PluginPresetSet = "plugin.preset.set" (PluginPresetSetParams) -> ArrangementMutation, project;
        PluginStateGet = "plugin.state.get" (DeviceParams) -> PluginState, project;
        PluginStateSet = "plugin.state.set" (PluginStateSetParams) -> ArrangementMutation, project;
        PluginStateInspect = "plugin.state.inspect" (DeviceParams) -> PluginStateInspection, project;
        PluginStateRead = "plugin.state.read" (PluginStateReadParams) -> PluginStateRead, project;
        PluginStatePatch = "plugin.state.patch" (PluginStatePatchParams) -> ArrangementMutation, project;
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
        TakeVariantSet = "clip.audio.take-variant.set" (TakeVariantParams) -> ArrangementMutation, project;
        TakeComparisonStart = "take.comparison.start" (TakeIdParams) -> AudioStatus, project;
        TakeComparisonSwitch = "take.comparison.switch" (TakeComparisonParams) -> AudioStatus, project;
        TakeComparisonStop = "take.comparison.stop" (EmptyParams) -> AudioStatus, project;
        ProjectRestoreGeneration = "project.restore-generation" (ProjectRestoreParams) -> ArrangementMutation, project;

        RenderStart = "render.start" (RenderStartParams) -> Job, project;
        RenderStems = "render.stems" (RenderStemsParams) -> Job, project;
        RenderExport = "render.export" (RenderExportParams) -> RenderExport, host;
        RenderList = "render.list" (EmptyParams) -> RenderList, host;
        RenderDelete = "render.delete" (RenderDeleteParams) -> RenderDelete, project;
        JobGet = "job.get" (IdParams) -> Job, host;
        JobCancel = "job.cancel" (IdParams) -> Job, host;
        AnalysisMeasure = "analysis.measure" (AnalysisMeasureParams) -> Job, host;
        AnalysisInspect = "analysis.inspect" (AnalysisParams) -> Analysis, host;

        LibrarySearch = "asset.search" (LibrarySearchParams) -> Library, host;
        LibraryAssetUpdate = "asset.update" (LibraryTagParams) -> LibraryAsset, host;
        LibraryRelated = "asset.related" (IdParams) -> Library, host;
        LibraryInstrumentList = "library.instrument.list" (EmptyParams) -> InstrumentLibrary, host;
        LibraryInstrumentFavoriteSet = "library.instrument.favorite.set" (InstrumentFavoriteParams) -> InstrumentLibraryItem, host;
        LibraryInstrumentCategoryList = "library.instrument.category.list" (EmptyParams) -> InstrumentCategories, host;
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
    use serde_json::{Value, json};

    fn schema_value(name: &str) -> serde_json::Value {
        command_params_schema(name)
            .unwrap_or_else(|| panic!("missing params schema for {name}"))
            .to_value()
    }

    fn assert_schema(name: &str, params: serde_json::Value, expected: bool) {
        let schema = schema_value(name);
        assert_eq!(
            jsonschema::is_valid(&schema, &params),
            expected,
            "{name} params: {params}\nschema: {schema}"
        );
    }

    #[test]
    fn every_command_exposes_a_deterministic_params_schema() {
        // Arrange
        let names = command_names();

        // Act / Assert
        assert_eq!(names.len(), 219);
        let unique: std::collections::HashSet<_> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len());
        for name in names {
            let first = schema_value(name);
            let second = schema_value(name);
            assert_eq!(first, second, "{name}");
            assert!(command_params_allow_omitted(name).is_some(), "{name}");
            assert_ne!(
                first,
                serde_json::json!(true),
                "{name} must not fall back to an unconstrained schema"
            );
        }
        assert_eq!(command_params_schema("unknown.command"), None);
        assert_eq!(command_params_allow_omitted("unknown.command"), None);
    }

    #[test]
    fn batchable_names_follow_canonical_batch_access() {
        // Arrange
        let names: Vec<_> = batchable_command_names().collect();
        let all: std::collections::HashSet<_> = command_names().iter().copied().collect();

        // Act / Assert
        assert!(names.contains(&"track.add"));
        assert!(names.contains(&"clip.midi.create"));
        assert!(names.contains(&"music.note.insert"));
        assert!(!names.contains(&"session.apply"));
        assert!(!names.contains(&"track.list"));
        assert!(!names.contains(&"clip.audio.add-asset"));
        assert!(!names.contains(&"render.start"));
        assert!(!names.contains(&"project.create"));
        assert!(!names.contains(&"instrument.save"));
        assert!(!names.contains(&"host.status"));
        assert!(names.iter().all(|name| all.contains(name)));
        let unique: std::collections::HashSet<_> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn params_schemas_follow_the_serde_input_shape() {
        // Arrange / Act / Assert
        assert_schema(
            "music.note.insert",
            json!({
                "clipId": "midi-clip:1",
                "notes": [{
                    "pitch": "C4",
                    "position": "1:1",
                    "duration": "1/8",
                    "velocity": 100,
                    "channel": 1
                }],
                "clipToEnd": false
            }),
            true,
        );
        assert_schema(
            "music.note.insert",
            json!({"clipId": "midi-clip:1", "notes": [{"pitch": 60, "position": "1:1", "duration": "1/8"}]}),
            false,
        );
        assert_schema(
            "music.note.insert",
            json!({"clipId": "midi-clip:1", "notes": [{"pitch": "C4", "position": 1, "duration": "1/8"}]}),
            false,
        );
        assert_schema(
            "music.note.insert",
            json!({"clipId": "midi-clip:1", "notes": [], "unknown": true}),
            false,
        );
        assert_schema(
            "music.note.update",
            json!({"clipId": "midi-clip:1", "noteId": "note:1", "lyric": null}),
            true,
        );
        assert_schema(
            "music.note.insert",
            json!({"clipId": "midi-clip:1", "notes": [{"pitch": "C4", "position": "1:1", "duration": "1/8", "velocity": null}]}),
            false,
        );
        assert_schema(
            "session.settings.update",
            json!({"projectName": null}),
            true,
        );
        assert_schema("clip.audio.list", json!({}), true);
        assert_schema("clip.audio.list", json!({"unexpected": 1}), false);
        assert_schema(
            "project.import",
            json!({"path": "C:/tmp/project.riffra"}),
            true,
        );
        assert_schema("job.get", json!({"id": "job:1"}), true);
        assert_schema("job.get", json!({}), false);
        assert_schema(
            "plugin.state.set",
            json!({
                "owner": {"kind": "track", "id": "track:1"},
                "deviceId": "device:1",
                "state": {"schemaVersion": 1, "pluginPath": "Reverb.vst3", "parameterValues": [0.5]}
            }),
            true,
        );
        assert_schema(
            "audio.driver.set",
            json!({
                "driver": "WASAPI",
                "inputChannel": 0,
                "inputDevice": null,
                "outputDevice": null,
                "sampleRate": null,
                "bufferSize": null
            }),
            true,
        );
        assert_schema(
            "clip.audio.trim",
            json!({
                "clipId": "clip:1",
                "startTick": 0,
                "sourceRange": {"start": 0, "end": 44100}
            }),
            true,
        );
        assert_schema(
            "clip.audio.trim",
            json!({
                "clipId": "clip:1",
                "startTick": "0",
                "sourceRange": {"start": 0, "end": 44100}
            }),
            false,
        );
        assert_schema(
            "automation.set",
            json!({
                "target": {"kind": "trackMix", "trackId": "track:1", "parameter": "volume"},
                "interpolation": "linear",
                "points": [{"id": "point:1", "tick": 0, "value": -6.0}]
            }),
            true,
        );
        assert_schema(
            "automation.set",
            json!({
                "target": {"kind": "trackMix", "trackId": "track:1", "parameter": "Volume"},
                "interpolation": "linear",
                "points": []
            }),
            false,
        );
        assert_schema(
            "clip.midi.update",
            json!({"clipId": "midi-clip:1", "patch": {"notes": []}}),
            true,
        );
        assert_schema(
            "music.timebase.set-map",
            json!({
                "tempoChanges": [{"tick": 0, "bpm": 120.0}],
                "timeSignatureChanges": [{"tick": 0, "numerator": 4, "denominator": 4}]
            }),
            true,
        );
        assert_schema(
            "track.add",
            json!({"name": "Lead", "kind": "instrument"}),
            true,
        );
        assert_schema(
            "track.add",
            json!({"name": "Lead", "kind": "instrument", "__unexpected": true}),
            false,
        );
        assert_schema("track.add", json!({"kind": "instrument"}), false);
        assert_schema("session.apply", json!({"operations": []}), true);
        assert_schema(
            "session.apply",
            json!({"operations": [], "__unexpected": true}),
            false,
        );
        assert_schema(
            "session.apply",
            json!({"operations": [{"command": "track.add"}]}),
            false,
        );
    }

    #[test]
    fn every_command_schema_reference_resolves_inside_its_own_definitions() {
        // Arrange / Act / Assert
        for name in command_names() {
            let schema = schema_value(name);
            let mut references = Vec::new();
            collect_schema_references(&schema, &mut references);
            for reference in references {
                let pointer = reference
                    .strip_prefix('#')
                    .unwrap_or_else(|| panic!("{name} has a non-local reference {reference}"));
                assert!(
                    schema.pointer(pointer).is_some(),
                    "{name} has an unresolved reference {reference}"
                );
            }
        }
    }

    fn collect_schema_references(value: &Value, references: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                    references.push(reference.to_owned());
                }
                for value in object.values() {
                    collect_schema_references(value, references);
                }
            }
            Value::Array(values) => {
                for value in values {
                    collect_schema_references(value, references);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn command_names_and_policies_match_the_wire_contract() {
        let map = typescript_result_map();
        let names: Vec<_> = map
            .lines()
            .filter_map(|line| line.trim().strip_prefix('\''))
            .filter_map(|line| line.split_once('\'').map(|(name, _)| name))
            .collect();
        let unique: std::collections::HashSet<_> = names.iter().copied().collect();
        assert_eq!(names.len(), 219);
        assert_eq!(unique.len(), names.len());

        for (name, params) in [
            ("clip.audio.list", json!({})),
            (
                "clip.midi.merge",
                json!({"firstClipId":"left", "secondClipId":"right"}),
            ),
            (
                "music.note.remove",
                json!({"clipId":"clip:1", "noteId":"note:1"}),
            ),
            ("audio.projection.get", json!({})),
            ("history.undo", json!({})),
        ] {
            let command = ControlCommand::decode(name, params).unwrap();
            assert_eq!(command.name(), name);
            assert!(unique.contains(name));
            let serialized = serde_json::to_value(&command).unwrap();
            let decoded = ControlCommand::decode(name, serialized["params"].clone()).unwrap();
            assert_eq!(decoded.policy(), command.policy());
        }
    }

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
            "effect.add" => {
                json!({"owner":{"kind":"track","id":"track:1"}, "pluginPath": "Reverb.vst3"})
            }
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
    fn automation_get_is_a_project_scoped_canonical_read() {
        let command = ControlCommand::decode(
            "automation.get",
            json!({"target": {"kind":"trackMix", "trackId": "track:1", "parameter": "volume"}}),
        )
        .unwrap();

        assert_eq!(
            command.policy().executor,
            CommandExecutor::Canonical {
                access: CanonicalAccess::Read,
            }
        );
        assert_eq!(
            command.policy().scope,
            CommandScope::Project {
                long_running: false,
            }
        );
    }

    #[test]
    fn unknown_command_name_is_rejected() {
        // Act
        let error = ControlCommand::decode("track.rename", json!({})).unwrap_err();

        // Assert
        assert!(
            matches!(&error, CommandDecodeError::UnknownCommand(name) if name == "track.rename")
        );
        let protocol_error: riffra_control::ProtocolError = error.into();
        assert_eq!(
            protocol_error.code,
            riffra_control::ErrorCode::InvalidRequest
        );
        assert_eq!(protocol_error.details.unwrap()["path"], "/command");
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
        assert_eq!(error.details()["path"], "/trackNmae");
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
        let details = error.details();
        assert_eq!(details["path"], "/notes/0/velocity");
        assert_eq!(details["index"], 0);
        assert_eq!(details["value"], "loud");
    }
}
