use clap::{Args, Parser, Subcommand};
use riffra_control::ControlCommand;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "riffra", version, about = "Headless production editing host")]
pub struct Cli {
    /// Root directory containing the session, library, and Asset stores.
    #[arg(long)]
    pub data_root: PathBuf,
    /// Read JSON Lines requests from stdin.
    #[arg(long)]
    pub interactive: bool,
    /// Route commands to the running Riffra Host.
    #[arg(long)]
    pub attach: bool,
    /// Require a specific canonical sequence for a one-shot mutation.
    #[arg(long)]
    pub expected_sequence: Option<u64>,
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Start one foreground live Host and publish its local control endpoint.
    Serve(ServeArgs),
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    History {
        #[command(subcommand)]
        command: HistoryCommand,
    },
    Track {
        #[command(subcommand)]
        command: TrackCommand,
    },
    AudioClip {
        #[command(subcommand)]
        command: AudioClipCommand,
    },
    MidiClip {
        #[command(subcommand)]
        command: MidiClipCommand,
    },
    MidiNote {
        #[command(subcommand)]
        command: MidiNoteCommand,
    },
    Music {
        #[command(subcommand)]
        command: MusicCommand,
    },
    Clip {
        #[command(subcommand)]
        command: ClipCommand,
    },
    Marker {
        #[command(subcommand)]
        command: MarkerCommand,
    },
    Timebase {
        #[command(subcommand)]
        command: TimebaseCommand,
    },
    LoopRange {
        #[command(subcommand)]
        command: RangeCommand,
    },
    PunchRange {
        #[command(subcommand)]
        command: RangeCommand,
    },
    Automation {
        #[command(subcommand)]
        command: AutomationCommand,
    },
    Asset {
        #[command(subcommand)]
        command: AssetCommand,
    },
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Instrument {
        #[command(subcommand)]
        command: InstrumentCommand,
    },
    Effect {
        #[command(subcommand)]
        command: EffectCommand,
    },
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
    Transport {
        #[command(subcommand)]
        command: TransportCommand,
    },
    Midi {
        #[command(subcommand)]
        command: LiveMidiCommand,
    },
    Audio {
        #[command(subcommand)]
        command: AudioCommand,
    },
    Record {
        #[command(subcommand)]
        command: RecordCommand,
    },
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    Analysis {
        #[command(subcommand)]
        command: AnalysisCommand,
    },
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    Missing {
        #[command(subcommand)]
        command: MissingCommand,
    },
    Render {
        #[command(subcommand)]
        command: RenderCommand,
    },
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },
    Undo,
    Redo,
}

#[derive(Clone, Debug, Args)]
pub struct ServeArgs {
    /// Keep native audio, MIDI, and external plugin processes offline.
    #[arg(long)]
    pub safe_mode: bool,
}

#[derive(Debug, Subcommand)]
pub enum HostCommand {
    Status,
    Shutdown,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    Get,
    Settings {
        #[command(subcommand)]
        command: SessionSettingsCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum SessionSettingsCommand {
    Update(SessionSettingsArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettingsArgs {
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_db: Option<f64>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_enabled: Option<bool>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_in_beats: Option<u8>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metronome_enabled: Option<bool>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum HistoryCommand {
    Get,
}

#[derive(Debug, Subcommand)]
pub enum TrackCommand {
    List,
    Add(TrackAddArgs),
    Update(TrackUpdateArgs),
    Remove(IdArg),
    Duplicate(IdArg),
    Reorder(ReorderTrackArgs),
    AudioInput {
        #[command(subcommand)]
        command: AudioInputCommand,
    },
    MidiInput {
        #[command(subcommand)]
        command: MidiInputCommand,
    },
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAddArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub kind: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackUpdateArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f64>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pan: Option<f64>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solo: Option<bool>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armed: Option<bool>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring: Option<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderTrackArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub target_index: usize,
}

#[derive(Debug, Subcommand)]
pub enum AudioInputCommand {
    Set(AudioInputSetArgs),
    Clear(IdArg),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputSetArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub channel_index: u32,
}

#[derive(Debug, Subcommand)]
pub enum MidiInputCommand {
    Set(MidiInputSetArgs),
    Clear(IdArg),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiInputSetArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// MIDI channel number in the inclusive range 1..=16.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdArg {
    #[arg(long)]
    pub track_id: String,
}

#[derive(Debug, Subcommand)]
pub enum AudioClipCommand {
    List,
    AddAsset(AudioClipAddAssetArgs),
    Update(AudioClipUpdateArgs),
    Move(ClipMoveArgs),
    Trim(AudioClipTrimArgs),
    Split(ClipSplitArgs),
    Duplicate(ClipIdArg),
    Crossfade(AudioClipCrossfadeArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipAddAssetArgs {
    #[arg(long)]
    pub asset_id: String,
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_tick: Option<u64>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipUpdateArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub patch: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub track_id: Option<String>,
    #[arg(long)]
    pub start_tick: Option<u64>,
    #[arg(long)]
    pub gain_db: Option<f64>,
    #[arg(long)]
    pub pan: Option<f64>,
    #[arg(long)]
    pub loop_enabled: Option<bool>,
    #[arg(long)]
    pub muted: Option<bool>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipMoveArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub start_tick: u64,
    #[arg(long)]
    pub track_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipTrimArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub start_tick: u64,
    #[arg(long)]
    pub source_start: u64,
    #[arg(long)]
    pub source_end: u64,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSplitArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub split_tick: u64,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipIdArg {
    #[arg(long)]
    pub clip_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipCrossfadeArgs {
    #[arg(long)]
    pub first_clip_id: String,
    #[arg(long)]
    pub second_clip_id: String,
}

#[derive(Debug, Subcommand)]
pub enum MidiClipCommand {
    List,
    Create(MidiClipCreateArgs),
    AddAsset(MidiClipAddAssetArgs),
    Update(MidiClipUpdateArgs),
    Move(ClipMoveArgs),
    Trim(MidiClipTrimArgs),
    Split(ClipSplitArgs),
    Duplicate(ClipIdArg),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipCreateArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub start_tick: u64,
    #[arg(long)]
    pub duration_ticks: u64,
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipAddAssetArgs {
    #[arg(long)]
    pub asset_id: String,
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub start_tick: Option<u64>,
    #[arg(long)]
    pub track_id: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipUpdateArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub patch: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub track_id: Option<String>,
    #[arg(long)]
    pub start_tick: Option<u64>,
    #[arg(long)]
    pub duration_ticks: Option<u64>,
    #[arg(long)]
    pub muted: Option<bool>,
    #[arg(long)]
    pub loop_enabled: Option<bool>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipTrimArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub start_tick: u64,
    #[arg(long)]
    pub duration_ticks: u64,
}

#[derive(Debug, Subcommand)]
pub enum MidiNoteCommand {
    Add(MidiNoteAddArgs),
    Insert(MidiNoteBulkArgs),
    Update(MidiNoteUpdateArgs),
    UpdateMany(MidiNoteUpdatesArgs),
    Remove(MidiNoteIdArgs),
    RemoveMany(MidiNoteIdsArgs),
    Clear(ClipIdArg),
    Quantize(MidiNoteQuantizeArgs),
    Transform(MidiNoteTransformArgs),
    Duplicate(MidiNoteDuplicateArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteAddArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub pitch: u8,
    #[arg(long)]
    pub start_tick: u64,
    #[arg(long)]
    pub duration_ticks: u64,
    #[arg(long)]
    pub velocity: u8,
    /// MIDI channel number in the inclusive range 1..=16.
    #[arg(long)]
    pub channel: u8,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteBulkArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long, alias = "notes")]
    pub notes_json: String,
}

#[derive(Debug, Subcommand)]
pub enum MusicCommand {
    MidiClip {
        #[command(subcommand)]
        command: MusicMidiClipCommand,
    },
    Note {
        #[command(subcommand)]
        command: MusicNoteCommand,
    },
    Region {
        #[command(subcommand)]
        command: MusicRegionCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum MusicMidiClipCommand {
    Create(MusicalMidiClipCreateArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalMidiClipCreateArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub start: String,
    #[arg(long)]
    pub end: String,
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum MusicNoteCommand {
    Insert(MusicalNoteBulkArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteBulkArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long, alias = "notes")]
    pub notes_json: String,
}

#[derive(Debug, Subcommand)]
pub enum MusicRegionCommand {
    List,
    Add(MusicalRegionAddArgs),
    Update(MusicalRegionUpdateArgs),
    Remove(MusicalRegionIdArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalRegionAddArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub start: String,
    #[arg(long)]
    pub end: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalRegionUpdateArgs {
    #[arg(long)]
    pub region_id: String,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub start: Option<String>,
    #[arg(long)]
    pub end: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalRegionIdArgs {
    #[arg(long)]
    pub region_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteUpdateArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub note_id: String,
    #[arg(long)]
    pub patch: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteUpdatesArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub updates_json: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteIdArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long)]
    pub note_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteIdsArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_ids_json: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteQuantizeArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_ids_json: Option<String>,
    #[arg(long)]
    pub grid_ticks: u64,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteTransformArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_ids_json: Option<String>,
    #[arg(long, default_value_t = 0)]
    pub transpose_semitones: i16,
    #[arg(long, default_value_t = 0)]
    pub velocity_offset: i16,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteDuplicateArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_ids_json: Option<String>,
    #[arg(long)]
    pub offset_ticks: u64,
}

#[derive(Debug, Subcommand)]
pub enum ClipCommand {
    Remove(ClipRemoveArgs),
    Paste(ClipPasteArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipRemoveArgs {
    #[arg(long, value_delimiter = ',')]
    pub audio_clip_ids: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub midi_clip_ids: Vec<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_clip_ids_json: Option<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub midi_clip_ids_json: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipPasteArgs {
    #[arg(long, value_delimiter = ',')]
    pub audio_clip_ids: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub midi_clip_ids: Vec<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_clip_ids_json: Option<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub midi_clip_ids_json: Option<String>,
    #[arg(long)]
    pub start_tick: u64,
}

#[derive(Debug, Subcommand)]
pub enum MarkerCommand {
    Add(MarkerAddArgs),
    Update(MarkerUpdateArgs),
    Remove(MarkerIdArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerAddArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub tick: u64,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerUpdateArgs {
    #[arg(long)]
    pub marker_id: String,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub tick: Option<u64>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerIdArgs {
    #[arg(long)]
    pub marker_id: String,
}

#[derive(Debug, Subcommand)]
pub enum TimebaseCommand {
    Update(TimebaseArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimebaseArgs {
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_signature_numerator: Option<u8>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_signature_denominator: Option<u8>,
}

#[derive(Debug, Subcommand)]
pub enum RangeCommand {
    Set(RangeArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeArgs {
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[arg(long)]
    pub start_tick: u64,
    #[arg(long)]
    pub end_tick: u64,
}

#[derive(Debug, Subcommand)]
pub enum AutomationCommand {
    Set(AutomationSetArgs),
    Clear(AutomationClearArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSetArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub parameter: String,
    #[arg(long)]
    pub points_json: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationClearArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub parameter: String,
}

#[derive(Debug, Subcommand)]
pub enum AssetCommand {
    ImportMidi(AssetImportMidiArgs),
    Preview(AssetPreviewArgs),
    StopPreview,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetImportMidiArgs {
    pub path: PathBuf,
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPreviewArgs {
    #[arg(long)]
    pub asset_id: String,
    #[arg(long, default_value_t = 0)]
    pub start_ms: u64,
    #[arg(long)]
    pub end_ms: Option<u64>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looped: Option<bool>,
    #[arg(long, default_value_t = 1.0)]
    pub gain: f32,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    Export,
    Import(ProjectImportArgs),
}

#[derive(Debug, Args, Serialize)]
pub struct ProjectImportArgs {
    pub path: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum InstrumentCommand {
    Clear(IdArg),
}

#[derive(Debug, Subcommand)]
pub enum EffectCommand {
    Remove(EffectRemoveArgs),
    Reorder(EffectReorderArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectRemoveArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub device_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectReorderArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long, value_delimiter = ',')]
    pub device_ids: Vec<String>,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_ids_json: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    Bypass(DeviceBypassArgs),
    ParameterSet(DeviceParameterSetArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceBypassArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub device_id: String,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypassed: Option<bool>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceParameterSetArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub device_id: String,
    #[arg(long)]
    pub parameter_index: u32,
    #[arg(long)]
    pub value: f32,
}

#[derive(Debug, Subcommand)]
pub enum RuntimeCommand {
    Projection {
        #[command(subcommand)]
        command: RuntimeProjectionCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum RuntimeProjectionCommand {
    Get,
    Retry,
}

#[derive(Debug, Subcommand)]
pub enum TransportCommand {
    Play(TransportSequenceArgs),
    Stop(TransportSequenceArgs),
    GoToStart(TransportSequenceArgs),
    Seek(SeekArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportSequenceArgs {
    #[arg(long)]
    pub transport_sequence: u64,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekArgs {
    #[arg(long)]
    pub tick: u64,
}

#[derive(Debug, Subcommand)]
pub enum LiveMidiCommand {
    Send(LiveMidiSendArgs),
    Panic(IdArg),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveMidiSendArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long, value_delimiter = ',')]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Subcommand)]
pub enum AudioCommand {
    Status,
    Probe,
    ChannelsProbe(AudioChannelsProbeArgs),
    Driver {
        #[command(subcommand)]
        command: AudioDriverCommand,
    },
    Recover,
    StartupRetry,
}

#[derive(Debug, Subcommand)]
pub enum AudioDriverCommand {
    Get,
    Set(AudioDriverArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioChannelsProbeArgs {
    #[arg(long)]
    pub driver: String,
    #[arg(long)]
    pub input_device: String,
    #[arg(long)]
    pub output_device: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDriverArgs {
    #[arg(long)]
    pub driver: String,
    #[arg(long)]
    pub input_device: Option<String>,
    #[arg(long, default_value_t = 0)]
    pub input_channel: u32,
    #[arg(long)]
    pub output_device: Option<String>,
    #[arg(long)]
    pub sample_rate: Option<u32>,
    #[arg(long)]
    pub buffer_size: Option<u32>,
}

#[derive(Debug, Subcommand)]
pub enum RecordCommand {
    Start(RecordStartArgs),
    AnotherTake(RecordStartArgs),
    Stop,
    Status,
    List(RecordListArgs),
    Rename(RecordRenameArgs),
    Archive(RecordIdArgs),
    Promote(RecordIdArgs),
    Tag(RecordTagArgs),
    Delete(RecordIdArgs),
    Duplicates,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordStartArgs {
    #[arg(long)]
    pub recording_session_id: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordListArgs {
    #[arg(long)]
    pub query: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordIdArgs {
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordRenameArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long)]
    pub new_name: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordTagArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long)]
    pub tag: Option<String>,
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum LibraryCommand {
    Search(LibrarySearchArgs),
    AssetUpdate(LibraryAssetUpdateArgs),
    Related(LibraryIdArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySearchArgs {
    #[arg(long)]
    pub query: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAssetUpdateArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long)]
    pub tag: Option<String>,
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIdArgs {
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Subcommand)]
pub enum AnalysisCommand {
    Start(AnalysisStartArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStartArgs {
    #[arg(long)]
    pub asset_id: Option<String>,
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    Catalog {
        #[command(subcommand)]
        command: PluginCatalogCommand,
    },
    Instrument(PluginPathArgs),
    Effect(PluginPathArgs),
    Scan(PluginScanArgs),
    ScanStart(PluginScanArgs),
}

#[derive(Debug, Subcommand)]
pub enum PluginCatalogCommand {
    List,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPathArgs {
    #[arg(long)]
    pub track_id: String,
    #[arg(long)]
    pub plugin_path: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginScanArgs {
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum MissingCommand {
    List,
    Relink(MissingRelinkArgs),
    DisablePlugin(DeviceIdArg),
    ReplacePlugin(MissingPluginReplaceArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingRelinkArgs {
    #[arg(long)]
    pub asset_id: String,
    #[arg(long)]
    pub new_path: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdArg {
    #[arg(long)]
    pub device_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingPluginReplaceArgs {
    #[arg(long)]
    pub device_id: String,
    #[arg(long)]
    pub new_path: String,
}

#[derive(Debug, Subcommand)]
pub enum RenderCommand {
    Start(RenderStartArgs),
}

#[derive(Debug, Args)]
pub struct RenderStartArgs {
    #[arg(long, default_value = "entire-arrangement")]
    pub range: String,
    #[arg(long)]
    pub start_tick: Option<u64>,
    #[arg(long)]
    pub end_tick: Option<u64>,
    #[arg(long)]
    pub normalize: Option<bool>,
    #[arg(long)]
    pub track_id: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum JobCommand {
    Get(JobIdArgs),
    Cancel(JobIdArgs),
}

#[derive(Debug, Args, Serialize)]
pub struct JobIdArgs {
    #[arg(long)]
    pub id: String,
}

impl Cli {
    pub fn request(self) -> Result<ControlCommand, String> {
        let command = self
            .command
            .ok_or_else(|| "a command is required unless --interactive is used".to_string())?;
        command_request(command)
    }
}

fn command_request(command: CliCommand) -> Result<ControlCommand, String> {
    let request = match command {
        CliCommand::Serve(_) => {
            return Err("serve is a process mode and cannot be used as a one-shot command".into());
        }
        CliCommand::Host { command } => match command {
            HostCommand::Status => simple("host.status"),
            HostCommand::Shutdown => simple("host.shutdown"),
        },
        CliCommand::Session { command } => match command {
            SessionCommand::Get => simple("session.get"),
            SessionCommand::Settings { command } => match command {
                SessionSettingsCommand::Update(args) => value("session.settings.update", args),
            },
        },
        CliCommand::History { command } => match command {
            HistoryCommand::Get => simple("history.get"),
        },
        CliCommand::Track { command } => match command {
            TrackCommand::List => simple("track.list"),
            TrackCommand::Add(args) => value("track.add", args),
            TrackCommand::Update(args) => value("track.update", args),
            TrackCommand::Remove(args) => value("track.remove", json!({"trackId": args.track_id})),
            TrackCommand::Duplicate(args) => {
                value("track.duplicate", json!({"trackId": args.track_id}))
            }
            TrackCommand::Reorder(args) => value("track.reorder", args),
            TrackCommand::AudioInput { command } => match command {
                AudioInputCommand::Set(args) => value("track.audio-input.set", args),
                AudioInputCommand::Clear(args) => {
                    value("track.audio-input.clear", json!({"trackId": args.track_id}))
                }
            },
            TrackCommand::MidiInput { command } => match command {
                MidiInputCommand::Set(args) => value("track.midi-input.set", args),
                MidiInputCommand::Clear(args) => {
                    value("track.midi-input.clear", json!({"trackId": args.track_id}))
                }
            },
        },
        CliCommand::AudioClip { command } => match command {
            AudioClipCommand::List => simple("audio-clip.list"),
            AudioClipCommand::AddAsset(args) => value("audio-clip.add-asset", args),
            AudioClipCommand::Update(args) => audio_clip_update(args)?,
            AudioClipCommand::Move(args) => value(
                "audio-clip.move",
                json!({"moves":[{"clipId":args.clip_id,"startTick":args.start_tick,"trackId":args.track_id}]}),
            ),
            AudioClipCommand::Trim(args) => value(
                "audio-clip.trim",
                json!({
                    "clipId": args.clip_id,
                    "startTick": args.start_tick,
                    "sourceRange": {"start": args.source_start, "end": args.source_end}
                }),
            ),
            AudioClipCommand::Split(args) => value("audio-clip.split", args),
            AudioClipCommand::Duplicate(args) => {
                value("audio-clip.duplicate", json!({"clipId": args.clip_id}))
            }
            AudioClipCommand::Crossfade(args) => value("audio-clip.crossfade", args),
        },
        CliCommand::MidiClip { command } => match command {
            MidiClipCommand::List => simple("midi-clip.list"),
            MidiClipCommand::Create(args) => value("midi-clip.create", args),
            MidiClipCommand::AddAsset(args) => value("midi-clip.add-asset", args),
            MidiClipCommand::Update(args) => midi_clip_update(args)?,
            MidiClipCommand::Move(args) => value(
                "midi-clip.move",
                json!({"moves":[{"clipId":args.clip_id,"startTick":args.start_tick,"trackId":args.track_id}]}),
            ),
            MidiClipCommand::Trim(args) => value("midi-clip.trim", args),
            MidiClipCommand::Split(args) => value("midi-clip.split", args),
            MidiClipCommand::Duplicate(args) => {
                value("midi-clip.duplicate", json!({"clipId": args.clip_id}))
            }
        },
        CliCommand::MidiNote { command } => match command {
            MidiNoteCommand::Add(args) => value("midi-note.add", args),
            MidiNoteCommand::Insert(args) => {
                json_string("midi-note.insert", args.clip_id, "notes", args.notes_json)?
            }
            MidiNoteCommand::Update(args) => {
                let patch: Value = serde_json::from_str(&args.patch)
                    .map_err(|error| format!("--patch is invalid JSON: {error}"))?;
                value(
                    "midi-note.update",
                    json!({"clipId":args.clip_id,"noteId":args.note_id,"patch":patch}),
                )
            }
            MidiNoteCommand::UpdateMany(args) => json_string(
                "midi-note.update-many",
                args.clip_id,
                "updates",
                args.updates_json,
            )?,
            MidiNoteCommand::Remove(args) => value("midi-note.remove", args),
            MidiNoteCommand::RemoveMany(args) => {
                id_list_value("midi-note.remove-many", args, &["noteIds"])?
            }
            MidiNoteCommand::Clear(args) => value("midi-note.clear", args),
            MidiNoteCommand::Quantize(args) => {
                id_list_value("midi-note.quantize", args, &["noteIds"])?
            }
            MidiNoteCommand::Transform(args) => {
                id_list_value("midi-note.transform", args, &["noteIds"])?
            }
            MidiNoteCommand::Duplicate(args) => {
                id_list_value("midi-note.duplicate", args, &["noteIds"])?
            }
        },
        CliCommand::Music { command } => match command {
            MusicCommand::MidiClip { command } => match command {
                MusicMidiClipCommand::Create(args) => value("music.midi-clip.create", args),
            },
            MusicCommand::Note { command } => match command {
                MusicNoteCommand::Insert(args) => {
                    json_string("music.note.insert", args.clip_id, "notes", args.notes_json)?
                }
            },
            MusicCommand::Region { command } => match command {
                MusicRegionCommand::List => simple("music.region.list"),
                MusicRegionCommand::Add(args) => value("music.region.add", args),
                MusicRegionCommand::Update(args) => value("music.region.update", args),
                MusicRegionCommand::Remove(args) => value("music.region.remove", args),
            },
        },
        CliCommand::Clip { command } => match command {
            ClipCommand::Remove(args) => {
                id_list_value("clip.remove", args, &["audioClipIds", "midiClipIds"])?
            }
            ClipCommand::Paste(args) => {
                id_list_value("clip.paste", args, &["audioClipIds", "midiClipIds"])?
            }
        },
        CliCommand::Marker { command } => match command {
            MarkerCommand::Add(args) => value("marker.add", args),
            MarkerCommand::Update(args) => value("marker.update", args),
            MarkerCommand::Remove(args) => value("marker.remove", args),
        },
        CliCommand::Timebase { command } => match command {
            TimebaseCommand::Update(args) => value("timebase.update", args),
        },
        CliCommand::LoopRange { command } => match command {
            RangeCommand::Set(args) => range_value("loop-range.set", args),
        },
        CliCommand::PunchRange { command } => match command {
            RangeCommand::Set(args) => range_value("punch-range.set", args),
        },
        CliCommand::Automation { command } => match command {
            AutomationCommand::Set(args) => {
                let points_json = args.points_json.clone();
                json_string_with_fields("automation.set", args, "points", points_json)?
            }
            AutomationCommand::Clear(args) => value("automation.clear", args),
        },
        CliCommand::Asset { command } => match command {
            AssetCommand::ImportMidi(args) => value(
                "asset.import-midi",
                json!({"path":args.path,"name":args.name}),
            ),
            AssetCommand::Preview(args) => asset_preview_value(args),
            AssetCommand::StopPreview => simple("asset.preview.stop"),
        },
        CliCommand::Project { command } => match command {
            ProjectCommand::Export => simple("project.export"),
            ProjectCommand::Import(args) => value("project.import", json!({"path":args.path})),
        },
        CliCommand::Instrument { command } => match command {
            InstrumentCommand::Clear(args) => {
                value("instrument.clear", json!({"trackId": args.track_id}))
            }
        },
        CliCommand::Effect { command } => match command {
            EffectCommand::Remove(args) => value("effect.remove", args),
            EffectCommand::Reorder(args) => id_list_value("effect.reorder", args, &["deviceIds"])?,
        },
        CliCommand::Device { command } => match command {
            DeviceCommand::Bypass(args) => device_bypass_value(args),
            DeviceCommand::ParameterSet(args) => value("device.parameter.set", args),
        },
        CliCommand::Runtime { command } => match command {
            RuntimeCommand::Projection { command } => match command {
                RuntimeProjectionCommand::Get => simple("runtime.projection.get"),
                RuntimeProjectionCommand::Retry => simple("runtime.projection.retry"),
            },
        },
        CliCommand::Transport { command } => match command {
            TransportCommand::Play(args) => value("transport.play", args),
            TransportCommand::Stop(args) => value("transport.stop", args),
            TransportCommand::GoToStart(args) => value("transport.go-to-start", args),
            TransportCommand::Seek(args) => value("transport.seek", args),
        },
        CliCommand::Midi { command } => match command {
            LiveMidiCommand::Send(args) => value("midi.send", args),
            LiveMidiCommand::Panic(args) => value("midi.panic", json!({"trackId": args.track_id})),
        },
        CliCommand::Audio { command } => match command {
            AudioCommand::Status => simple("audio.status"),
            AudioCommand::Probe => simple("audio.probe"),
            AudioCommand::ChannelsProbe(args) => value("audio.channels.probe", args),
            AudioCommand::Driver { command } => match command {
                AudioDriverCommand::Get => simple("audio.driver.get"),
                AudioDriverCommand::Set(args) => value("audio.driver.set", args),
            },
            AudioCommand::Recover => simple("audio.recover"),
            AudioCommand::StartupRetry => simple("audio.startup.retry"),
        },
        CliCommand::Record { command } => match command {
            RecordCommand::Start(args) => value("record.start", args),
            RecordCommand::AnotherTake(args) => value("record.start", args),
            RecordCommand::Stop => simple("record.stop"),
            RecordCommand::Status => simple("record.status"),
            RecordCommand::List(args) => value("record.list", args),
            RecordCommand::Rename(args) => value("record.rename", args),
            RecordCommand::Archive(args) => value("record.archive", args),
            RecordCommand::Promote(args) => value("record.promote", args),
            RecordCommand::Tag(args) => value("record.tag", args),
            RecordCommand::Delete(args) => value("record.delete", args),
            RecordCommand::Duplicates => simple("record.duplicates"),
        },
        CliCommand::Library { command } => match command {
            LibraryCommand::Search(args) => value("library.search", args),
            LibraryCommand::AssetUpdate(args) => value("library.asset.update", args),
            LibraryCommand::Related(args) => value("library.related", args),
        },
        CliCommand::Analysis { command } => match command {
            AnalysisCommand::Start(args) => value("analysis.start", args),
        },
        CliCommand::Plugin { command } => match command {
            PluginCommand::Catalog { command } => match command {
                PluginCatalogCommand::List => simple("plugin.catalog.list"),
            },
            PluginCommand::Instrument(args) => value("instrument.set", args),
            PluginCommand::Effect(args) => value("effect.add", args),
            PluginCommand::Scan(args) => value("plugin.scan", args),
            PluginCommand::ScanStart(args) => value("plugin.scan.start", args),
        },
        CliCommand::Missing { command } => match command {
            MissingCommand::List => simple("missing.list"),
            MissingCommand::Relink(args) => value("missing.relink", args),
            MissingCommand::DisablePlugin(args) => value("missing.disable-plugin", args),
            MissingCommand::ReplacePlugin(args) => value("missing.replace-plugin", args),
        },
        CliCommand::Render { command } => match command {
            RenderCommand::Start(args) => render_start(args)?,
        },
        CliCommand::Job { command } => match command {
            JobCommand::Get(args) => value("job.get", args),
            JobCommand::Cancel(args) => value("job.cancel", args),
        },
        CliCommand::Undo => simple("undo"),
        CliCommand::Redo => simple("redo"),
    };
    Ok(request)
}

fn simple(command: &str) -> ControlCommand {
    ControlCommand {
        name: command.into(),
        params: json!({}),
    }
}

fn value<T: Serialize>(command: &str, params: T) -> ControlCommand {
    ControlCommand {
        name: command.into(),
        params: serde_json::to_value(params).expect("CLI arguments must serialize"),
    }
}

fn render_start(args: RenderStartArgs) -> Result<ControlCommand, String> {
    let range = match args.range.as_str() {
        "entire-arrangement" => json!({"kind": "entireArrangement"}),
        "loop-range" => json!({"kind": "loopRange"}),
        "time-selection" => {
            let start_tick = args
                .start_tick
                .ok_or_else(|| "--start-tick is required for --range time-selection".to_string())?;
            let end_tick = args
                .end_tick
                .ok_or_else(|| "--end-tick is required for --range time-selection".to_string())?;
            json!({
                "kind": "timeSelection",
                "startTick": start_tick,
                "endTick": end_tick,
            })
        }
        other => {
            return Err(format!(
                "--range must be entire-arrangement, loop-range, or time-selection (got {other})"
            ));
        }
    };
    Ok(value(
        "render.start",
        json!({
            "options": {
                "range": range,
                "normalize": args.normalize.unwrap_or(false),
                "trackId": args.track_id,
            }
        }),
    ))
}

fn json_string(
    command: &str,
    clip_id: String,
    field: &str,
    encoded: String,
) -> Result<ControlCommand, String> {
    let points: Value = serde_json::from_str(&encoded)
        .map_err(|error| format!("--{field}-json is invalid JSON: {error}"))?;
    Ok(value(command, json!({"clipId": clip_id, field: points})))
}

fn json_string_with_fields<T: Serialize>(
    command: &str,
    fields: T,
    field: &str,
    encoded: String,
) -> Result<ControlCommand, String> {
    let mut object = serde_json::to_value(fields)
        .map_err(|error| format!("CLI arguments could not be encoded: {error}"))?;
    object
        .as_object_mut()
        .ok_or_else(|| "CLI arguments did not form an object".to_string())?
        .insert(
            field.into(),
            serde_json::from_str(&encoded)
                .map_err(|error| format!("--{field}-json is invalid JSON: {error}"))?,
        );
    Ok(value(command, object))
}

fn id_list_value<T: Serialize>(
    command: &str,
    params: T,
    fields: &[&str],
) -> Result<ControlCommand, String> {
    let mut params = serde_json::to_value(params)
        .map_err(|error| format!("CLI arguments could not be encoded: {error}"))?;
    for field in fields {
        replace_id_list_from_json(&mut params, field)?;
    }
    Ok(value(command, params))
}

fn replace_id_list_from_json(params: &mut Value, field: &str) -> Result<(), String> {
    let object = params
        .as_object_mut()
        .ok_or_else(|| "CLI arguments did not form an object".to_string())?;
    let json_field = format!("{field}Json");
    let encoded = object.remove(&json_field);
    let Some(encoded) = encoded else {
        return Ok(());
    };
    let encoded = encoded
        .as_str()
        .ok_or_else(|| format!("--{json_field} must contain a JSON array of strings"))?;
    if object
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|ids| !ids.is_empty())
    {
        return Err(format!("--{field} and --{json_field} cannot be combined"));
    }
    let ids = serde_json::from_str::<Vec<String>>(encoded)
        .map_err(|error| format!("--{json_field} is invalid JSON: {error}"))?;
    object.insert(
        field.to_owned(),
        serde_json::to_value(ids).expect("String IDs serialize"),
    );
    Ok(())
}

fn range_value(command: &str, args: RangeArgs) -> ControlCommand {
    value(
        command,
        json!({
            "enabled": args.enabled.unwrap_or(false),
            "startTick": args.start_tick,
            "endTick": args.end_tick,
        }),
    )
}

fn asset_preview_value(args: AssetPreviewArgs) -> ControlCommand {
    value(
        "asset.preview",
        json!({
            "assetId": args.asset_id,
            "startMs": args.start_ms,
            "endMs": args.end_ms,
            "looped": args.looped.unwrap_or(false),
            "gain": args.gain,
        }),
    )
}

fn device_bypass_value(args: DeviceBypassArgs) -> ControlCommand {
    value(
        "device.bypass",
        json!({
            "trackId": args.track_id,
            "deviceId": args.device_id,
            "bypassed": args.bypassed.unwrap_or(false),
        }),
    )
}

fn audio_clip_update(args: AudioClipUpdateArgs) -> Result<ControlCommand, String> {
    if let Some(patch) = args.patch {
        let patch: Value = serde_json::from_str(&patch)
            .map_err(|error| format!("--patch is invalid JSON: {error}"))?;
        return Ok(value(
            "audio-clip.update",
            json!({"clipId": args.clip_id, "patch": patch}),
        ));
    }
    let patch = json!({
        "name": args.name,
        "trackId": args.track_id,
        "startTick": args.start_tick,
        "gainDb": args.gain_db,
        "pan": args.pan,
        "loopEnabled": args.loop_enabled,
        "muted": args.muted,
    });
    Ok(value(
        "audio-clip.update",
        json!({"clipId": args.clip_id, "patch": patch}),
    ))
}

fn midi_clip_update(args: MidiClipUpdateArgs) -> Result<ControlCommand, String> {
    if let Some(patch) = args.patch {
        let patch: Value = serde_json::from_str(&patch)
            .map_err(|error| format!("--patch is invalid JSON: {error}"))?;
        return Ok(value(
            "midi-clip.update",
            json!({"clipId": args.clip_id, "patch": patch}),
        ));
    }
    let patch = json!({
        "name": args.name,
        "trackId": args.track_id,
        "startTick": args.start_tick,
        "durationTicks": args.duration_ticks,
        "muted": args.muted,
        "loopEnabled": args.loop_enabled,
    });
    Ok(value(
        "midi-clip.update",
        json!({"clipId": args.clip_id, "patch": patch}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn timebase_update_accepts_a_partial_patch() {
        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "timebase",
            "update",
            "--bpm",
            "140",
        ])
        .unwrap();

        let request = cli.request().unwrap();
        assert_eq!(request.name, "timebase.update");
        assert_eq!(request.params, json!({"bpm": 140.0}));
    }

    #[test]
    fn music_commands_preserve_musical_input_without_calculating_ticks() {
        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "music",
            "midi-clip",
            "create",
            "--track-id",
            "track:1",
            "--start",
            "5:1",
            "--end",
            "13:1",
            "--name",
            "Piano",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "music.midi-clip.create");
        assert_eq!(
            request.params,
            json!({"trackId":"track:1","start":"5:1","end":"13:1","name":"Piano"})
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "music",
            "note",
            "insert",
            "--clip-id",
            "midi-clip:1",
            "--notes-json",
            r#"[{"pitch":"C4","position":"5:1","duration":"1/8"}]"#,
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "music.note.insert");
        assert_eq!(
            request.params,
            json!({
                "clipId":"midi-clip:1",
                "notes":[{"pitch":"C4","position":"5:1","duration":"1/8"}]
            })
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "music",
            "region",
            "add",
            "--name",
            "A'",
            "--start",
            "5:1",
            "--end",
            "13:1",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "music.region.add");
        assert_eq!(
            request.params,
            json!({"name":"A'","start":"5:1","end":"13:1"})
        );
    }

    #[test]
    fn timebase_update_does_not_accept_ppq_as_an_external_field() {
        assert!(
            Cli::try_parse_from([
                "riffra",
                "--data-root",
                "data",
                "timebase",
                "update",
                "--ppq",
                "960",
            ])
            .is_err()
        );
    }

    #[test]
    fn id_json_arguments_preserve_paths_and_comma_arguments() {
        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "clip",
            "remove",
            "--midi-clip-ids-json",
            r#"["midi-clip:recording-slot:take:C:\\takes\\lead.wav:track:1"]"#,
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(
            request.params,
            json!({
                "audioClipIds": [],
                "midiClipIds": ["midi-clip:recording-slot:take:C:\\takes\\lead.wav:track:1"]
            })
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "midi-note",
            "remove-many",
            "--clip-id",
            "clip:1",
            "--note-ids",
            "note:a,note:b",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(
            request.params,
            json!({"clipId":"clip:1","noteIds":["note:a","note:b"]})
        );
    }

    #[test]
    fn boolean_command_arguments_require_explicit_values() {
        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "loop-range",
            "set",
            "--enabled",
            "true",
            "--start-tick",
            "0",
            "--end-tick",
            "960",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(
            request.params,
            json!({"enabled":true,"startTick":0,"endTick":960})
        );
        assert!(
            Cli::try_parse_from([
                "riffra",
                "--data-root",
                "data",
                "loop-range",
                "set",
                "--enabled",
                "--start-tick",
                "0",
                "--end-tick",
                "960",
            ])
            .is_err()
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "render",
            "start",
            "--normalize",
            "true",
        ])
        .unwrap();
        assert_eq!(cli.request().unwrap().params["options"]["normalize"], true);
    }

    #[test]
    fn midi_note_clear_maps_to_the_clip_command() {
        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "midi-note",
            "clear",
            "--clip-id",
            "midi-clip:1",
        ])
        .unwrap();

        let request = cli.request().unwrap();
        assert_eq!(request.name, "midi-note.clear");
        assert_eq!(request.params, json!({"clipId":"midi-clip:1"}));
    }
}
