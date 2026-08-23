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
    /// Route commands to the running Desktop Host.
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
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteQuantizeArgs {
    #[arg(long)]
    pub clip_id: String,
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
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
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipPasteArgs {
    #[arg(long, value_delimiter = ',')]
    pub audio_clip_ids: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub midi_clip_ids: Vec<String>,
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
    #[arg(long, default_value_t = 960)]
    pub ppq: u32,
    #[arg(long)]
    pub bpm: f64,
    #[arg(long)]
    pub time_signature_numerator: u8,
    #[arg(long)]
    pub time_signature_denominator: u8,
}

#[derive(Debug, Subcommand)]
pub enum RangeCommand {
    Set(RangeArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeArgs {
    #[arg(long)]
    pub enabled: bool,
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
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetImportMidiArgs {
    pub path: PathBuf,
    #[arg(long)]
    pub name: Option<String>,
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
    pub bypassed: bool,
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
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    Catalog {
        #[command(subcommand)]
        command: PluginCatalogCommand,
    },
    Instrument(PluginPathArgs),
    Effect(PluginPathArgs),
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
    pub normalize: bool,
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
            MidiNoteCommand::RemoveMany(args) => value("midi-note.remove-many", args),
            MidiNoteCommand::Quantize(args) => value("midi-note.quantize", args),
            MidiNoteCommand::Transform(args) => value("midi-note.transform", args),
            MidiNoteCommand::Duplicate(args) => value("midi-note.duplicate", args),
        },
        CliCommand::Clip { command } => match command {
            ClipCommand::Remove(args) => value("clip.remove", args),
            ClipCommand::Paste(args) => value("clip.paste", args),
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
            RangeCommand::Set(args) => value("loop-range.set", args),
        },
        CliCommand::PunchRange { command } => match command {
            RangeCommand::Set(args) => value("punch-range.set", args),
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
            EffectCommand::Reorder(args) => value("effect.reorder", args),
        },
        CliCommand::Device { command } => match command {
            DeviceCommand::Bypass(args) => value("device.bypass", args),
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
        },
        CliCommand::Plugin { command } => match command {
            PluginCommand::Catalog { command } => match command {
                PluginCatalogCommand::List => simple("plugin.catalog.list"),
            },
            PluginCommand::Instrument(args) => value("instrument.set", args),
            PluginCommand::Effect(args) => value("effect.add", args),
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
                "normalize": args.normalize,
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
