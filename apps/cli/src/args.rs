use clap::{Args, Parser, Subcommand};
use riffra_control::ControlCommand;
use serde::Serialize;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

fn is_false(value: &bool) -> bool {
    !*value
}

fn finite_f64(value: &str) -> Result<f64, String> {
    let parsed: f64 = value
        .parse()
        .map_err(|error| format!("`{value}` is not a number ({error})"))?;
    if !parsed.is_finite() {
        return Err(format!("`{value}` is not a finite number"));
    }
    Ok(parsed)
}

#[derive(Debug, Parser)]
#[command(
    name = "riffra",
    version,
    about = "Edit a Riffra project from the command line and control a running Riffra Host",
    long_about = "Edit a Riffra project from the command line and control a running Riffra Host.\n\n\
Use --data-root for standalone project access against a project directory, or --attach to \
send commands to a running Riffra Host. Use --host when more than one Host is running to pick \
the target instance, and --expected-sequence as an optimistic concurrency check that a one-shot \
command runs only against a project state at the given sequence.\n\n\
Use --interactive to read JSON Lines requests from stdin and return one JSON response per request."
)]
pub struct Cli {
    /// Root directory containing the session, library, and Asset stores; required for standalone access and incompatible with --attach.
    #[arg(long)]
    pub data_root: Option<PathBuf>,
    /// Select one Host instance by its id when more than one Riffra Host is running; only valid together with --attach.
    #[arg(long)]
    pub host: Option<String>,
    /// Read JSON Lines requests from stdin and write one JSON response per request; cannot be combined with a one-shot command.
    #[arg(long)]
    pub interactive: bool,
    /// Route commands to a running Riffra Host instead of opening the project directory directly.
    #[arg(long)]
    pub attach: bool,
    /// Require a specific sequence for a one-shot command so it only runs when the project state is at that sequence; reported as a conflict failure otherwise.
    #[arg(long)]
    pub expected_sequence: Option<u64>,
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Start one foreground live Host and publish its local control endpoint.
    ///
    /// Runs the process modes of the Riffra Host and cannot be combined with --attach, --interactive,
    /// or --expected-sequence. Requires --data-root.
    Serve(ServeArgs),
    /// Discover, inspect, and shut down running Riffra Host instances.
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
    /// Read and update session-level project state.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Read the history stack of completed mutations.
    History {
        #[command(subcommand)]
        command: HistoryCommand,
    },
    /// Create and manage Tracks, their mix state, and their input routes.
    Track {
        #[command(subcommand)]
        command: TrackCommand,
    },
    /// Create and edit Audio Clips on the arrangement timeline using raw ticks.
    AudioClip {
        #[command(subcommand)]
        command: AudioClipCommand,
    },
    /// Create and edit MIDI Clips on the arrangement timeline using raw ticks.
    MidiClip {
        #[command(subcommand)]
        command: MidiClipCommand,
    },
    /// Edit individual MIDI Notes by note ID using raw MIDI pitch and Clip-relative ticks.
    MidiNote {
        #[command(subcommand)]
        command: MidiNoteCommand,
    },
    /// Edit notes, clips, regions, harmony, and phrases in musical coordinates such as bar:beat positions and note names.
    ///
    /// Use this command group for arrangement-absolute musical notation. Use midi-note for raw MIDI pitch and
    /// Clip-relative tick editing, and midi-clip for raw-tick MIDI Clip placement.
    Music {
        #[command(subcommand)]
        command: MusicCommand,
    },
    /// Remove or paste whole Audio and MIDI Clips as a group.
    Clip {
        #[command(subcommand)]
        command: ClipCommand,
    },
    /// Add, update, and remove named markers on the arrangement timeline.
    Marker {
        #[command(subcommand)]
        command: MarkerCommand,
    },
    /// Update the project tempo and time signature.
    Timebase {
        #[command(subcommand)]
        command: TimebaseCommand,
    },
    /// Set the loop playback range.
    LoopRange {
        #[command(subcommand)]
        command: RangeCommand,
    },
    /// Set the punch-in recording range.
    PunchRange {
        #[command(subcommand)]
        command: RangeCommand,
    },
    /// Set or clear volume and pan automation lanes on a Track.
    Automation {
        #[command(subcommand)]
        command: AutomationCommand,
    },
    /// Import MIDI files, preview Assets, and stop previews.
    Asset {
        #[command(subcommand)]
        command: AssetCommand,
    },
    /// List, create, open, rename, export, and import projects.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Manage Instruments: list, save, export, apply to Tracks, and clear; init, validate, inspect, render, and audition forward to the bundled Sonalloy CLI.
    ///
    /// For installing or configuring a VST3 instrument plugin on a Track, use `plugin instrument` instead.
    Instrument {
        #[command(subcommand)]
        command: InstrumentCommand,
    },
    /// Remove and reorder effect devices in a Track rack.
    Effect {
        #[command(subcommand)]
        command: EffectCommand,
    },
    /// Bypass devices and read or write device parameters.
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    /// Inspect and retry the audio runtime projection.
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
    /// Control transport playback: play, stop, return to start, and seek.
    Transport {
        #[command(subcommand)]
        command: TransportCommand,
    },
    /// Send live MIDI bytes to a Track and panic stuck notes.
    Midi {
        #[command(subcommand)]
        command: LiveMidiCommand,
    },
    /// Inspect and configure the live audio engine: status, device probes, diagnostics, driver selection, and recovery.
    ///
    /// For playing back an imported Asset sample, use `asset preview` instead.
    Audio {
        #[command(subcommand)]
        command: AudioCommand,
    },
    /// Start, stop, and manage recording takes.
    Record {
        #[command(subcommand)]
        command: RecordCommand,
    },
    /// Search the Asset library and read related Assets.
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Start background analysis of an Asset.
    Analysis {
        #[command(subcommand)]
        command: AnalysisCommand,
    },
    /// Discover VST3 plugins, manage presets and state, and load instrument or effect plugins onto Tracks.
    ///
    /// For built-in and User Instruments defined by Sonalloy definitions, use `instrument` instead.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// List missing Assets and plugins, then relink, disable, or replace them.
    Missing {
        #[command(subcommand)]
        command: MissingCommand,
    },
    /// Render the arrangement, a range, or a single Track stem to an audio Asset as a background Job.
    Render {
        #[command(subcommand)]
        command: RenderCommand,
    },
    /// Inspect, cancel, and wait for background Jobs.
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },
    /// Revert the most recent mutation; requires --interactive in standalone mode because history is process-local, and works as a one-shot command with --attach.
    Undo,
    /// Reapply the most recently reverted mutation; requires --interactive in standalone mode because history is process-local, and works as a one-shot command with --attach.
    Redo,
}

#[derive(Clone, Debug, Args)]
pub struct ServeArgs {
    /// Keep native audio, MIDI, and external plugin processes offline; playback, recording, device probes, plugin scans, and live previews are rejected while Safe Mode is active.
    #[arg(long)]
    pub safe_mode: bool,
}

#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// List running Riffra Host instances with their ids, process ids, and data roots; handled locally by the CLI.
    List,
    /// Report the running Host status; requires a running Riffra Host accessed with --attach.
    Status,
    /// Shut down the running Riffra Host; requires --attach.
    Shutdown,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Return the full session snapshot including all Tracks, Clips, and Notes.
    ///
    /// Prefer `session inspect` when only the structure and current sequence are needed; get returns
    /// heavyweight Note and recording detail.
    Get,
    /// Return a structural summary of the project, optionally filtered by musical range or Track.
    Inspect(SessionInspectArgs),
    /// Apply a batch of operations from a JSON Lines file as one atomic mutation.
    ///
    /// The file is UTF-8 JSON Lines. Empty lines are ignored. Each non-empty line is one operation
    /// holding `command` and `params`. `requestId` and `expectedSequence` must not appear inside
    /// an operation. A file with zero operations fails.
    ///
    /// The batch is atomic: the project changes only when every operation succeeds, and one
    /// failing operation leaves the project unchanged. Later operations observe the changes of
    /// earlier ones, so a Track or Clip created earlier in the batch can be referenced by name.
    /// `--expected-sequence` acts as the sequence precondition for the batch as a whole.
    ///
    /// Batch operations may resolve `trackName` instead of `trackId`, and `clipName` instead of
    /// `clipId`. Specifying both forms fails. Zero matches or multiple matches fail. `clipName`
    /// also requires `trackId` or `trackName` and resolves only within that Track.
    ///
    /// Only these commands may appear in a batch: session.settings.update; track.add, track.update,
    /// track.remove, track.duplicate, track.reorder, track.audio-input.set, track.audio-input.clear,
    /// track.midi-input.set, track.midi-input.clear; audio-clip.update, audio-clip.move,
    /// audio-clip.trim, audio-clip.split, audio-clip.duplicate, audio-clip.crossfade;
    /// midi-clip.create, midi-clip.update, midi-clip.move, midi-clip.trim, midi-clip.split,
    /// midi-clip.duplicate; midi-note.add, midi-note.insert, midi-note.update, midi-note.update-many,
    /// midi-note.remove, midi-note.remove-many, midi-note.clear, midi-note.quantize,
    /// midi-note.transform, midi-note.duplicate; music.midi-clip.create, music.midi-clip.resize,
    /// music.note.insert, music.note.update, music.note.remove, music.note.transform,
    /// music.harmony.insert, music.harmony.update, music.harmony.remove, music.harmony.realize,
    /// music.phrase.insert, music.region.add, music.region.update, music.region.remove;
    /// clip.remove, clip.paste; marker.add, marker.update, marker.remove; timebase.update;
    /// loop-range.set, punch-range.set; automation.set, automation.clear; instrument.apply,
    /// instrument.clear; effect.add, effect.remove, effect.reorder; device.bypass,
    /// device.parameter.set. instrument.apply accepts only `builtin:` instruments in a batch;
    /// `user:` instruments are rejected.
    Apply(SessionApplyArgs),
    /// Read or update session settings such as project name, master gain, loop, and metronome.
    Settings {
        #[command(subcommand)]
        command: SessionSettingsCommand,
    },
}

#[derive(Debug, Args)]
pub struct SessionApplyArgs {
    /// UTF-8 JSON Lines file; one operation per non-empty line, each holding `command` and `params`; empty lines are ignored and a file with no operations fails.
    #[arg(long)]
    pub file: PathBuf,
    /// Return the identities allocated by the complete batch in `createdEntityIds`.
    #[arg(long)]
    pub include_created_ids: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInspectArgs {
    /// Optional half-open range start on the arrangement as a musical position in bar:beat or bar:beat+fraction notation; must be provided together with --end, and --end must be after --start.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// Optional half-open range end on the arrangement as a musical position in bar:beat or bar:beat+fraction notation; must be provided together with --start, and must be after --start.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// Optional Track id; when set, only that Track is summarized.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum SessionSettingsCommand {
    /// Update session settings; only the supplied fields change.
    Update(SessionSettingsArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettingsArgs {
    /// New project display name.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    /// Master gain in dB; non-finite values are rejected and the value is clamped to -90..=0.
    #[arg(long, value_parser = finite_f64, allow_hyphen_values = true)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_db: Option<f64>,
    /// Enable or disable loop playback.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_enabled: Option<bool>,
    /// Count-in length in beats before recording or playback starts; values above 8 are clamped to 8.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_in_beats: Option<u8>,
    /// Enable or disable the metronome click.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metronome_enabled: Option<bool>,
    /// Free-form session note stored with the settings.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum HistoryCommand {
    /// Return the undo and redo stacks of completed mutations.
    Get,
}

#[derive(Debug, Subcommand)]
pub enum TrackCommand {
    /// List all Tracks with their mix state, inputs, and instrument assignment.
    List,
    /// Add a new audio or instrument Track.
    Add(TrackAddArgs),
    /// Update mix state, naming, monitoring, or color on one Track.
    ///
    /// Gain is specified in dB and clamped to -90..=24; pan is clamped to -1.0..=1.0; non-finite
    /// values are rejected. When any Solo Track exists, every non-solo Track is silent; a Track
    /// that is both muted and solo is silent; clip mute silences only that Clip's output.
    Update(TrackUpdateArgs),
    /// Remove one Track and its Clips and devices.
    Remove(IdArg),
    /// Duplicate one Track together with its Clips and devices under a new identity.
    Duplicate(IdArg),
    /// Move one Track to a new position in the Track list.
    Reorder(ReorderTrackArgs),
    /// Set or clear the physical audio input routed to an audio Track.
    AudioInput {
        #[command(subcommand)]
        command: AudioInputCommand,
    },
    /// Set or clear the MIDI input source and channel filter for an instrument Track.
    MidiInput {
        #[command(subcommand)]
        command: MidiInputCommand,
    },
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAddArgs {
    /// Display name for the new Track.
    #[arg(long)]
    pub name: String,
    /// Track kind: `audio` for a physical audio input Track, `instrument` for a MIDI-driven instrument Track.
    #[arg(long, value_parser = ["audio", "instrument"])]
    pub kind: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackUpdateArgs {
    /// Id of the Track to update.
    #[arg(long)]
    pub track_id: String,
    /// New display name.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Track gain in dB; non-finite values are rejected and the value is clamped to -90..=24.
    #[arg(long, value_parser = finite_f64, allow_hyphen_values = true)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f64>,
    /// Track stereo pan; non-finite values are rejected and values are clamped to -1.0 (left) through 1.0 (right).
    #[arg(long, value_parser = finite_f64, allow_hyphen_values = true)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pan: Option<f64>,
    /// Mute or unmute this Track; a muted Track is silent regardless of automation.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    /// Solo or unsolo this Track; when any Track is solo, every non-solo Track is silent, and a Track that is both muted and solo is silent.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solo: Option<bool>,
    /// Arm or unarm this Track for recording.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armed: Option<bool>,
    /// Input monitoring mode: `off` never monitors, `auto` monitors while armed, `on` always monitors.
    #[arg(long, value_parser = ["off", "auto", "on"])]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring: Option<String>,
    /// Presentation color; an empty string clears it and returns the Track to automatic coloring.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderTrackArgs {
    /// Id of the Track to move.
    #[arg(long)]
    pub track_id: String,
    /// Zero-based destination index in the Track list.
    #[arg(long)]
    pub target_index: usize,
}

#[derive(Debug, Subcommand)]
pub enum AudioInputCommand {
    /// Route a physical input channel to an audio Track.
    Set(AudioInputSetArgs),
    /// Clear the physical audio input from an audio Track.
    Clear(IdArg),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputSetArgs {
    /// Id of the audio Track to route.
    #[arg(long)]
    pub track_id: String,
    /// Zero-based physical input channel index on the audio device.
    #[arg(long)]
    pub channel_index: u32,
}

#[derive(Debug, Subcommand)]
pub enum MidiInputCommand {
    /// Set the MIDI input source and channel filter for an instrument Track.
    Set(MidiInputSetArgs),
    /// Clear the MIDI input route from an instrument Track.
    Clear(IdArg),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiInputSetArgs {
    /// Id of the instrument Track to route.
    #[arg(long)]
    pub track_id: String,
    /// MIDI input device id; omit to accept input from any device.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// MIDI channel number in the inclusive range 1..=16; omit to accept all channels.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdArg {
    /// Id of the target Track.
    #[arg(long)]
    pub track_id: String,
}

#[derive(Debug, Subcommand)]
pub enum AudioClipCommand {
    /// List Audio Clips with their arrangement positions and mix state.
    List,
    /// Add an Asset to the arrangement as an Audio Clip.
    AddAsset(AudioClipAddAssetArgs),
    /// Update one Audio Clip; supply --patch for a JSON patch or the individual field flags.
    ///
    /// Gain is specified in dB and clamped to -90..=24; pan is clamped to -1.0..=1.0. --patch is
    /// mutually exclusive with the individual field flags.
    Update(AudioClipUpdateArgs),
    /// Move one Audio Clip to a new arrangement tick and Track.
    Move(ClipMoveArgs),
    /// Change where an Audio Clip starts on the arrangement and which source sample range it plays.
    Trim(AudioClipTrimArgs),
    /// Split one Audio Clip into two Clips at an arrangement tick.
    Split(ClipSplitArgs),
    /// Duplicate one Audio Clip under a new identity.
    Duplicate(ClipIdArg),
    /// Create an equal-power crossfade across the time overlap of two Audio Clips on the same Track.
    ///
    /// The two Clips must be different, live on the same Track, and overlap in time. The earlier
    /// Clip fades out and the later Clip fades in across the overlap; the two ids may be given in
    /// either order.
    Crossfade(AudioClipCrossfadeArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipAddAssetArgs {
    /// Id of the imported Asset to place on the arrangement.
    #[arg(long)]
    pub asset_id: String,
    /// Display name for the new Audio Clip.
    #[arg(long)]
    pub name: String,
    /// Absolute arrangement start position in timeline ticks; omit to append the Clip after the last existing Audio Clip.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_tick: Option<u64>,
    /// Destination Track id; omit to use the first existing audio Track, creating one named `Audio 1` when no audio Track exists.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipUpdateArgs {
    /// Id of the Audio Clip to update.
    #[arg(long)]
    pub clip_id: String,
    /// JSON object patch with the allowed fields name, trackId, startTick, timelineDuration, sourceRange, gainDb, pan, fadeIn, fadeOut, fadeShape, loopEnabled, and muted; mutually exclusive with the individual field flags.
    #[arg(long, conflicts_with_all = ["name", "track_id", "start_tick", "gain_db", "pan", "loop_enabled", "muted"])]
    pub patch: Option<String>,
    /// New display name.
    #[arg(long)]
    pub name: Option<String>,
    /// Destination Track id.
    #[arg(long)]
    pub track_id: Option<String>,
    /// Absolute arrangement start position in timeline ticks.
    #[arg(long)]
    pub start_tick: Option<u64>,
    /// Clip gain in dB; non-finite values are rejected and the value is clamped to -90..=24.
    #[arg(long, value_parser = finite_f64, allow_hyphen_values = true)]
    pub gain_db: Option<f64>,
    /// Clip stereo pan; non-finite values are rejected and values are clamped to -1.0 (left) through 1.0 (right).
    #[arg(long, value_parser = finite_f64, allow_hyphen_values = true)]
    pub pan: Option<f64>,
    /// Loop or unloop this Clip's source material.
    #[arg(long)]
    pub loop_enabled: Option<bool>,
    /// Mute or unmute only this Clip's output.
    #[arg(long)]
    pub muted: Option<bool>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipMoveArgs {
    /// Id of the Clip to move.
    #[arg(long)]
    pub clip_id: String,
    /// Absolute arrangement start position in timeline ticks for the Clip after the move.
    #[arg(long)]
    pub start_tick: u64,
    /// Destination Track id.
    #[arg(long)]
    pub track_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipTrimArgs {
    /// Id of the Audio Clip to trim.
    #[arg(long)]
    pub clip_id: String,
    /// Absolute arrangement start position in timeline ticks after the trim.
    #[arg(long)]
    pub start_tick: u64,
    /// Inclusive start of the source sample range in samples from the beginning of the Asset.
    #[arg(long)]
    pub source_start: u64,
    /// Exclusive end of the source sample range in samples; must be greater than --source-start.
    #[arg(long)]
    pub source_end: u64,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSplitArgs {
    /// Id of the Clip to split.
    #[arg(long)]
    pub clip_id: String,
    /// Absolute arrangement position in timeline ticks at which to split.
    #[arg(long)]
    pub split_tick: u64,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipIdArg {
    /// Id of the target Clip.
    #[arg(long)]
    pub clip_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipCrossfadeArgs {
    /// Id of one Clip in the crossfade pair.
    #[arg(long)]
    pub first_clip_id: String,
    /// Id of the other Clip in the crossfade pair; the two ids may be given in either order.
    #[arg(long)]
    pub second_clip_id: String,
}

#[derive(Debug, Subcommand)]
pub enum MidiClipCommand {
    /// List MIDI Clips with their arrangement positions and note counts.
    List,
    /// Create an empty MIDI Clip on a Track at an absolute arrangement tick.
    Create(MidiClipCreateArgs),
    /// Import a MIDI Asset to the arrangement as a MIDI Clip.
    AddAsset(MidiClipAddAssetArgs),
    /// Update one MIDI Clip; supply --patch for a JSON patch or the individual field flags.
    ///
    /// --patch is mutually exclusive with the individual field flags.
    Update(MidiClipUpdateArgs),
    /// Move one MIDI Clip to a new arrangement tick and Track.
    Move(ClipMoveArgs),
    /// Change a MIDI Clip's arrangement start and duration in timeline ticks.
    Trim(MidiClipTrimArgs),
    /// Split one MIDI Clip into two Clips at an arrangement tick.
    Split(ClipSplitArgs),
    /// Duplicate one MIDI Clip under a new identity.
    Duplicate(ClipIdArg),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipCreateArgs {
    /// Destination instrument Track id.
    #[arg(long)]
    pub track_id: String,
    /// Absolute arrangement start position in timeline ticks; the project timebase defines ticks per beat.
    #[arg(long)]
    pub start_tick: u64,
    /// Clip length in timeline ticks.
    #[arg(long)]
    pub duration_ticks: u64,
    /// Display name for the new MIDI Clip.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipAddAssetArgs {
    /// Id of the imported MIDI Asset to place on the arrangement.
    #[arg(long)]
    pub asset_id: String,
    /// Display name for the new MIDI Clip.
    #[arg(long)]
    pub name: String,
    /// Absolute arrangement start position in timeline ticks; omit to place the Clip at arrangement tick 0.
    #[arg(long)]
    pub start_tick: Option<u64>,
    /// Destination instrument Track id; omit to use the first existing instrument Track, creating one named `Instrument 1` when no instrument Track exists.
    #[arg(long)]
    pub track_id: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipUpdateArgs {
    /// Id of the MIDI Clip to update.
    #[arg(long)]
    pub clip_id: String,
    /// JSON object patch with the allowed fields name, trackId, startTick, durationTicks, notes, events, muted, and loopEnabled; mutually exclusive with the individual field flags.
    #[arg(long, conflicts_with_all = ["name", "track_id", "start_tick", "duration_ticks", "muted", "loop_enabled"])]
    pub patch: Option<String>,
    /// New display name.
    #[arg(long)]
    pub name: Option<String>,
    /// Destination instrument Track id.
    #[arg(long)]
    pub track_id: Option<String>,
    /// Absolute arrangement start position in timeline ticks.
    #[arg(long)]
    pub start_tick: Option<u64>,
    /// Clip length in timeline ticks.
    #[arg(long)]
    pub duration_ticks: Option<u64>,
    /// Mute or unmute only this Clip's output.
    #[arg(long)]
    pub muted: Option<bool>,
    /// Loop or unloop this Clip's contents.
    #[arg(long)]
    pub loop_enabled: Option<bool>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipTrimArgs {
    /// Id of the MIDI Clip to trim.
    #[arg(long)]
    pub clip_id: String,
    /// Absolute arrangement start position in timeline ticks after the trim.
    #[arg(long)]
    pub start_tick: u64,
    /// Clip length in timeline ticks after the trim.
    #[arg(long)]
    pub duration_ticks: u64,
}

#[derive(Debug, Subcommand)]
pub enum MidiNoteCommand {
    /// Add one MIDI Note to a Clip at a Clip-relative tick.
    Add(MidiNoteAddArgs),
    /// Insert many MIDI Notes into a Clip from a JSON array in one mutation.
    Insert(MidiNoteBulkArgs),
    /// Replace fields on one MIDI Note with a JSON patch object.
    Update(MidiNoteUpdateArgs),
    /// Replace fields on many MIDI Notes in one mutation from a JSON array of updates.
    UpdateMany(MidiNoteUpdatesArgs),
    /// Remove one MIDI Note from a Clip by note id.
    Remove(MidiNoteIdArgs),
    /// Remove many MIDI Notes from a Clip by id list or JSON array.
    RemoveMany(MidiNoteIdsArgs),
    /// Remove every MIDI Note from a Clip.
    Clear(ClipIdArg),
    /// Move selected MIDI Notes onto a tick grid.
    Quantize(MidiNoteQuantizeArgs),
    /// Transpose and re-velocity selected MIDI Notes in one mutation.
    Transform(MidiNoteTransformArgs),
    /// Duplicate selected MIDI Notes by a tick offset.
    Duplicate(MidiNoteDuplicateArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteAddArgs {
    /// Id of the MIDI Clip that receives the Note.
    #[arg(long)]
    pub clip_id: String,
    /// MIDI pitch number 0..=127 where 60 is middle C.
    #[arg(long)]
    pub pitch: u8,
    /// Note start relative to the Clip start in timeline ticks; the project timebase defines ticks per beat.
    #[arg(long)]
    pub start_tick: u64,
    /// Note length in timeline ticks.
    #[arg(long)]
    pub duration_ticks: u64,
    /// MIDI velocity 0..=127.
    #[arg(long)]
    pub velocity: u8,
    /// MIDI channel number in the inclusive range 1..=16.
    #[arg(long)]
    pub channel: u8,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteBulkArgs {
    /// Id of the MIDI Clip that receives the Notes.
    #[arg(long)]
    pub clip_id: String,
    /// Inline JSON array of notes; each element is an object with pitch, startTick, durationTicks, velocity, and channel. Mutually exclusive with --notes-file and --stdin.
    #[arg(long, alias = "notes")]
    pub notes_json: Option<String>,
    /// Path to a JSON file containing an array of notes; mutually exclusive with --notes-json and --stdin.
    #[arg(long)]
    pub notes_file: Option<PathBuf>,
    /// Read the notes JSON array from stdin; mutually exclusive with --notes-json and --notes-file.
    #[arg(long)]
    pub stdin: bool,
}

#[derive(Debug, Subcommand)]
pub enum MusicCommand {
    /// Create and resize MIDI Clips using absolute musical bar:beat positions rather than raw ticks.
    ///
    /// Use this for arrangement-absolute placement in musical coordinates. Use `midi-clip` for
    /// Clip placement expressed directly in timeline ticks.
    MidiClip {
        #[command(subcommand)]
        command: MusicMidiClipCommand,
    },
    /// List, read, insert, update, remove, and transform Notes using note names and bar:beat positions.
    ///
    /// Use this for musical notation such as pitch C4, position 5:1, and duration 1/8. Use
    /// `midi-note` for raw MIDI pitch numbers and Clip-relative ticks.
    Note {
        #[command(subcommand)]
        command: MusicNoteCommand,
    },
    /// List, add, update, and remove named timeline regions in musical positions.
    Region {
        #[command(subcommand)]
        command: MusicRegionCommand,
    },
    /// Resolve chord symbols and manage harmony events over absolute musical ranges.
    Harmony {
        #[command(subcommand)]
        command: MusicHarmonyCommand,
    },
    /// Insert and preview reusable rhythmic phrases anchored to musical positions.
    Phrase {
        #[command(subcommand)]
        command: MusicPhraseCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum MusicHarmonyCommand {
    /// Resolve a chord symbol to its tones without storing it.
    Resolve(MusicalHarmonyResolveArgs),
    /// List stored harmony events with their absolute musical ranges.
    List,
    /// Insert harmony events from a JSON array in one mutation.
    Insert(MusicalHarmonyInsertArgs),
    /// Update one harmony event's range or chord definition from a JSON patch object.
    Update(MusicalHarmonyUpdateArgs),
    /// Remove harmony events by id list in one mutation.
    Remove(MusicalHarmonyRemoveArgs),
    /// Realize stored harmony events into MIDI Notes on a Clip using an optional rhythm pattern.
    Realize(MusicalHarmonyRealizeArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalHarmonyResolveArgs {
    /// Chord symbol to resolve, such as `C`, `Dm9`, or `G7(b9,#11)/F`.
    #[arg(long)]
    pub chord: String,
}

#[derive(Debug, Args)]
pub struct MusicalHarmonyInsertArgs {
    /// Inline JSON array of harmony events; each element is an object with start, end, and one of chord or explicit pitches/root/bass/label. Mutually exclusive with --events-file.
    #[arg(long, alias = "events")]
    pub events_json: Option<String>,
    /// Path to a JSON file containing an array of harmony events; mutually exclusive with --events-json.
    #[arg(long)]
    pub events_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct MusicalHarmonyUpdateArgs {
    /// Id of the harmony event to update.
    #[arg(long)]
    pub event_id: String,
    /// JSON object patch; top-level fields are start, end, chord, pitches, root, bass, label. eventId must not appear in the patch. Provide either a full chord symbol or explicit tone fields, not a partial merge of both.
    #[arg(long, alias = "patch")]
    pub patch_json: String,
}

#[derive(Debug, Args)]
pub struct MusicalHarmonyRemoveArgs {
    /// JSON array of harmony event ids to remove.
    #[arg(long, alias = "event-ids")]
    pub event_ids_json: String,
}

#[derive(Debug, Args)]
pub struct MusicalHarmonyRealizeArgs {
    /// Id of the destination MIDI Clip that receives the realized Notes.
    #[arg(long)]
    pub clip_id: String,
    /// Optional half-open range start on the arrangement as a musical position in bar:beat or bar:beat+fraction notation; restricts which harmony events are realized.
    #[arg(long)]
    pub start: Option<String>,
    /// Optional half-open range end on the arrangement as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub end: Option<String>,
    /// Lowest octave used for deterministic chord voicing; defaults to 3.
    #[arg(long)]
    #[arg(allow_hyphen_values = true)]
    pub lowest_octave: Option<i8>,
    /// Inline JSON rhythm object; top-level fields are length (whole-note fraction) and steps (array of offset, duration, optional velocity). Mutually exclusive with --rhythm-file.
    #[arg(long)]
    pub rhythm_json: Option<String>,
    /// Path to a JSON file containing the rhythm object; mutually exclusive with --rhythm-json.
    #[arg(long)]
    pub rhythm_file: Option<PathBuf>,
    /// MIDI velocity 0..=127 applied to realized Notes; defaults to 100 when omitted.
    #[arg(long)]
    pub velocity: Option<u8>,
    /// MIDI channel number in the inclusive range 1..=16 applied to realized Notes; defaults to 1 when omitted.
    #[arg(long)]
    pub channel: Option<u8>,
}

#[derive(Debug, Subcommand)]
pub enum MusicPhraseCommand {
    /// Insert a phrase's pattern and placements as MIDI Notes on a Clip in one mutation.
    Insert(MusicalPhraseInsertArgs),
    /// Resolve a phrase and return the Notes it would create without storing them.
    Preview(MusicalPhrasePreviewArgs),
}

#[derive(Debug, Args)]
pub struct MusicalPhraseInsertArgs {
    /// Id of the destination MIDI Clip.
    #[arg(long)]
    pub clip_id: String,
    /// Inline JSON object with pattern (length, notes with offset, duration, semitones, optional velocity) and placements (array of position, anchor, repeats 1..=256). clipId must not appear inside. Mutually exclusive with --phrase-file.
    #[arg(long, alias = "phrase")]
    pub phrase_json: Option<String>,
    /// Path to a JSON file containing the phrase object; mutually exclusive with --phrase-json.
    #[arg(long)]
    pub phrase_file: Option<PathBuf>,
    /// MIDI channel number in the inclusive range 1..=16 applied to inserted Notes; defaults to 1 when omitted.
    #[arg(long)]
    pub channel: Option<u8>,
}

#[derive(Debug, Args)]
pub struct MusicalPhrasePreviewArgs {
    /// Id of the MIDI Clip the phrase would target.
    #[arg(long)]
    pub clip_id: String,
    /// Inline JSON object with pattern (length, notes with offset, duration, semitones, optional velocity) and placements (array of position, anchor, repeats 1..=256). clipId must not appear inside. Mutually exclusive with --phrase-file.
    #[arg(long, alias = "phrase")]
    pub phrase_json: Option<String>,
    /// Path to a JSON file containing the phrase object; mutually exclusive with --phrase-json.
    #[arg(long)]
    pub phrase_file: Option<PathBuf>,
    /// MIDI channel number in the inclusive range 1..=16 used when resolving Notes; defaults to 1 when omitted.
    #[arg(long)]
    pub channel: Option<u8>,
    /// Include the fully resolved Notes in the preview response, using musical pitch, position, and duration notation.
    #[arg(long)]
    pub include_notes: bool,
}

#[derive(Debug, Subcommand)]
pub enum MusicMidiClipCommand {
    /// Create a MIDI Clip spanning an absolute musical range on a Track.
    Create(MusicalMidiClipCreateArgs),
    /// Change a MIDI Clip's absolute musical start or end, keeping Note positions in arrangement coordinates.
    ///
    /// At least one of --start or --end is required. Notes that would fall outside the resized Clip
    /// cause the whole mutation to fail rather than being cropped or deleted.
    Resize(MusicalMidiClipResizeArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalMidiClipCreateArgs {
    /// Destination instrument Track id.
    #[arg(long)]
    pub track_id: String,
    /// Absolute arrangement start as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub start: String,
    /// Absolute arrangement end as a musical position in bar:beat or bar:beat+fraction notation; must be after --start.
    #[arg(long)]
    pub end: String,
    /// Display name for the new MIDI Clip.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct MusicalMidiClipResizeArgs {
    /// Id of the MIDI Clip to resize.
    #[arg(long)]
    pub clip_id: String,
    /// New absolute arrangement start as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub start: Option<String>,
    /// New absolute arrangement end as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub end: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum MusicNoteCommand {
    /// List Notes in a half-open musical range, returning any Note whose interval overlaps the range.
    ///
    /// Exactly one of --clip-id or --track-id is required; supplying both or neither fails. A
    /// range is optional with --clip-id, but --start and --end must be provided together, and
    /// both are required with --track-id. The range is the half-open interval
    /// [start, end): a Note is returned when its own interval overlaps the range, even if its
    /// start lies outside it. Results are grouped per Clip for Track scope. Note ids appear only
    /// with --include-ids; raw MIDI and tick values appear only with --raw.
    List(MusicalNoteListArgs),
    /// Read one Note by Clip id and Note id.
    Get(MusicalNoteGetArgs),
    /// Insert Notes into a Clip from a JSON array in one mutation, using musical pitch, position, and duration.
    Insert(MusicalNoteBulkArgs),
    /// Update fields on one Note using musical pitch, position, and duration.
    Update(MusicalNoteUpdateArgs),
    /// Remove one Note from a Clip by Note id.
    Remove(MusicalNoteRemoveArgs),
    /// Transform Notes whose start lies in a half-open musical range in one atomic mutation.
    ///
    /// Exactly one of --clip-id or --track-id is required; supplying both or neither fails. A
    /// range is optional with --clip-id, but --start and --end must be provided together, and
    /// both are required with --track-id. Selection uses each Note's start
    /// position inside [start, end): a Note that merely overlaps the range without starting in it
    /// is not selected. --pitch and --channel filter on pre-transform values. --timing-offset is a
    /// signed whole-note fraction such as +1/48 or -1/48. --velocity-offset results clamp to
    /// MIDI velocity 0..127. A --transpose-semitones result outside MIDI pitch 0..127 fails the
    /// whole mutation, as does a timing shift that moves a Note outside its Clip. The mutation
    /// fails when no Note is selected or when no Note actually changes.
    Transform(MusicalNoteTransformArgs),
}

#[derive(Debug, Args)]
pub struct MusicalNoteBulkArgs {
    /// Id of the MIDI Clip that receives the Notes.
    #[arg(long)]
    pub clip_id: String,
    /// Inline JSON array of notes; each element is an object with pitch (note name such as C4), position (arrangement-absolute bar:beat), duration (whole-note fraction such as 1/8), and optional velocity (0..=127, default 100) and channel (1..=16, default 1). Mutually exclusive with --notes-file and --stdin.
    #[arg(long, alias = "notes")]
    pub notes_json: Option<String>,
    /// Path to a JSON file containing an array of musical notes; mutually exclusive with --notes-json and --stdin.
    #[arg(long)]
    pub notes_file: Option<PathBuf>,
    /// Read the notes JSON array from stdin; mutually exclusive with --notes-json and --notes-file.
    #[arg(long)]
    pub stdin: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteListArgs {
    /// Scope to one MIDI Clip; mutually exclusive with --track-id and exactly one scope option is required.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_id: Option<String>,
    /// Scope to every MIDI Clip on one Track; mutually exclusive with --clip-id, and --start and --end are required with this scope.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    /// Half-open range start as an arrangement-absolute musical position in bar:beat or bar:beat+fraction notation; must be provided together with --end, and both are required with --track-id.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// Half-open range end as an arrangement-absolute musical position in bar:beat or bar:beat+fraction notation; must be provided together with --start, and both are required with --track-id.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// Include stable Note ids in the response.
    #[arg(long)]
    #[serde(skip_serializing_if = "is_false")]
    pub include_ids: bool,
    /// Return raw MIDI and tick values instead of musical coordinates; Clip startTick is arrangement-absolute while Note startTick is Clip-relative, and the response includes timebase metadata.
    #[arg(long)]
    #[serde(skip_serializing_if = "is_false")]
    pub raw: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteGetArgs {
    /// Id of the MIDI Clip containing the Note.
    #[arg(long)]
    pub clip_id: String,
    /// Id of the Note to read.
    #[arg(long)]
    pub note_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteUpdateArgs {
    /// Id of the MIDI Clip containing the Note.
    #[arg(long)]
    pub clip_id: String,
    /// Id of the Note to update.
    #[arg(long)]
    pub note_id: String,
    /// New pitch as a note name such as C4 or F#3.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<String>,
    /// New arrangement-absolute position as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    /// New duration as a whole-note fraction such as 1/8 or 3/16.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    /// New MIDI velocity 0..=127.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity: Option<u8>,
    /// New MIDI channel number in the inclusive range 1..=16.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteRemoveArgs {
    /// Id of the MIDI Clip containing the Note.
    #[arg(long)]
    pub clip_id: String,
    /// Id of the Note to remove.
    #[arg(long)]
    pub note_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteTransformArgs {
    /// Scope to one MIDI Clip; mutually exclusive with --track-id and exactly one scope option is required.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_id: Option<String>,
    /// Scope to every MIDI Clip on one Track; mutually exclusive with --clip-id, and --start and --end are required with this scope.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    /// Half-open selection range start as an arrangement-absolute musical position in bar:beat or bar:beat+fraction notation; selects Notes whose start lies in the range; must be provided together with --end, and both are required with --track-id.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// Half-open selection range end as an arrangement-absolute musical position in bar:beat or bar:beat+fraction notation; must be provided together with --start, and both are required with --track-id.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// Pre-transform pitch filter as a note name such as D2; only Notes currently at this pitch are selected.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<String>,
    /// Pre-transform MIDI channel filter in the inclusive range 1..=16; only Notes currently on this channel are selected.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    /// Signed whole-note timing displacement such as +1/48 or -1/48 applied to every selected Note; a resulting Note outside its Clip fails the whole mutation.
    #[arg(long, allow_hyphen_values = true)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing_offset: Option<String>,
    /// Signed velocity displacement added to every selected Note and clamped to MIDI velocity 0..127.
    #[arg(long, allow_hyphen_values = true)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity_offset: Option<i32>,
    /// Signed semitone transposition applied to every selected Note; a resulting pitch outside MIDI 0..127 fails the whole mutation.
    #[arg(long, allow_hyphen_values = true)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transpose_semitones: Option<i16>,
}

#[derive(Debug, Subcommand)]
pub enum MusicRegionCommand {
    /// List named timeline regions with their absolute musical ranges.
    List,
    /// Add a named region spanning an absolute musical range.
    Add(MusicalRegionAddArgs),
    /// Update a region's name or absolute musical range.
    Update(MusicalRegionUpdateArgs),
    /// Remove one region by id.
    Remove(MusicalRegionIdArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalRegionAddArgs {
    /// Free-form region name such as Intro or Verse; names are not constrained to a fixed set and may repeat.
    #[arg(long)]
    pub name: String,
    /// Absolute arrangement start as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub start: String,
    /// Absolute arrangement end as a musical position in bar:beat or bar:beat+fraction notation; must be after --start.
    #[arg(long)]
    pub end: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalRegionUpdateArgs {
    /// Id of the region to update.
    #[arg(long)]
    pub region_id: String,
    /// New free-form region name.
    #[arg(long)]
    pub name: Option<String>,
    /// New absolute arrangement start as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub start: Option<String>,
    /// New absolute arrangement end as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub end: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalRegionIdArgs {
    /// Id of the region.
    #[arg(long)]
    pub region_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteUpdateArgs {
    /// Id of the MIDI Clip containing the Note.
    #[arg(long)]
    pub clip_id: String,
    /// Id of the Note to update.
    #[arg(long)]
    pub note_id: String,
    /// JSON object patch; allowed fields are note (MIDI pitch 0..=127), startTick (Clip-relative), durationTicks, velocity (0..=127), and channel (1..=16).
    #[arg(long)]
    pub patch: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteUpdatesArgs {
    /// Id of the MIDI Clip containing the Notes.
    #[arg(long)]
    pub clip_id: String,
    /// JSON array of updates; each element is an object with noteId and a patch object of note, startTick, durationTicks, velocity, and channel.
    #[arg(long)]
    pub updates_json: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteIdArgs {
    /// Id of the MIDI Clip containing the Note.
    #[arg(long)]
    pub clip_id: String,
    /// Id of the Note.
    #[arg(long)]
    pub note_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteIdsArgs {
    /// Id of the MIDI Clip containing the Notes.
    #[arg(long)]
    pub clip_id: String,
    /// Comma-separated Note ids; mutually exclusive with --note-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
    /// JSON array of Note ids as an alternative to --note-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_ids_json: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteQuantizeArgs {
    /// Id of the MIDI Clip containing the Notes.
    #[arg(long)]
    pub clip_id: String,
    /// Comma-separated Note ids; mutually exclusive with --note-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
    /// JSON array of Note ids as an alternative to --note-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_ids_json: Option<String>,
    /// Quantization grid length in timeline ticks; the project timebase defines ticks per beat.
    #[arg(long)]
    pub grid_ticks: u64,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteTransformArgs {
    /// Id of the MIDI Clip containing the Notes.
    #[arg(long)]
    pub clip_id: String,
    /// Comma-separated Note ids; mutually exclusive with --note-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
    /// JSON array of Note ids as an alternative to --note-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_ids_json: Option<String>,
    /// Signed semitone transposition applied to every selected Note; the resulting pitch clamps to MIDI 0..127.
    #[arg(long, default_value_t = 0)]
    #[arg(allow_hyphen_values = true)]
    pub transpose_semitones: i16,
    /// Signed velocity displacement applied to every selected Note; the resulting velocity clamps to MIDI 0..127.
    #[arg(long, default_value_t = 0)]
    #[arg(allow_hyphen_values = true)]
    pub velocity_offset: i16,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteDuplicateArgs {
    /// Id of the MIDI Clip containing the Notes.
    #[arg(long)]
    pub clip_id: String,
    /// Comma-separated Note ids; mutually exclusive with --note-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub note_ids: Vec<String>,
    /// JSON array of Note ids as an alternative to --note-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_ids_json: Option<String>,
    /// Tick offset applied to each duplicated Note relative to its original Clip-relative start.
    #[arg(long)]
    pub offset_ticks: u64,
}

#[derive(Debug, Subcommand)]
pub enum ClipCommand {
    /// Remove Audio and MIDI Clips by id list or JSON array in one mutation.
    Remove(ClipRemoveArgs),
    /// Paste copied Audio and MIDI Clips at an absolute arrangement tick in one mutation.
    Paste(ClipPasteArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipRemoveArgs {
    /// Comma-separated Audio Clip ids; mutually exclusive with --audio-clip-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub audio_clip_ids: Vec<String>,
    /// Comma-separated MIDI Clip ids; mutually exclusive with --midi-clip-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub midi_clip_ids: Vec<String>,
    /// JSON array of Audio Clip ids as an alternative to --audio-clip-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_clip_ids_json: Option<String>,
    /// JSON array of MIDI Clip ids as an alternative to --midi-clip-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub midi_clip_ids_json: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipPasteArgs {
    /// Comma-separated Audio Clip ids to paste; mutually exclusive with --audio-clip-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub audio_clip_ids: Vec<String>,
    /// Comma-separated MIDI Clip ids to paste; mutually exclusive with --midi-clip-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub midi_clip_ids: Vec<String>,
    /// JSON array of Audio Clip ids as an alternative to --audio-clip-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_clip_ids_json: Option<String>,
    /// JSON array of MIDI Clip ids as an alternative to --midi-clip-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub midi_clip_ids_json: Option<String>,
    /// Absolute arrangement destination in timeline ticks for the pasted Clips.
    #[arg(long)]
    pub start_tick: u64,
}

#[derive(Debug, Subcommand)]
pub enum MarkerCommand {
    /// Add a named marker at an absolute musical position.
    Add(MarkerAddArgs),
    /// Update a marker's name or absolute musical position.
    Update(MarkerUpdateArgs),
    /// Remove one marker by id.
    Remove(MarkerIdArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerAddArgs {
    /// Marker display name.
    #[arg(long)]
    pub name: String,
    /// Absolute arrangement position as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub position: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerUpdateArgs {
    /// Id of the marker to update.
    #[arg(long)]
    pub marker_id: String,
    /// New marker display name.
    #[arg(long)]
    pub name: Option<String>,
    /// New absolute arrangement position as a musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub position: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerIdArgs {
    /// Id of the marker.
    #[arg(long)]
    pub marker_id: String,
}

#[derive(Debug, Subcommand)]
pub enum TimebaseCommand {
    /// Update the project tempo and time signature; only the supplied fields change.
    Update(TimebaseArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimebaseArgs {
    /// Tempo in beats per minute; non-finite values are rejected and the value must be within 20.0..=400.0.
    #[arg(long, value_parser = finite_f64)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// Time signature numerator, such as 4 in 4/4; must be within 1..=255.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_signature_numerator: Option<u8>,
    /// Time signature denominator, such as 4 in 4/4; must be one of 1, 2, 4, 8, 16, or 32.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_signature_denominator: Option<u8>,
}

#[derive(Debug, Subcommand)]
pub enum RangeCommand {
    /// Set the range boundaries and whether the range is enabled.
    Set(RangeArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeArgs {
    /// Enable or disable the range; defaults to false when omitted.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Range start as an arrangement-absolute musical position in bar:beat or bar:beat+fraction notation.
    #[arg(long)]
    pub start: String,
    /// Range end as an arrangement-absolute musical position in bar:beat or bar:beat+fraction notation; must be after --start.
    #[arg(long)]
    pub end: String,
}

#[derive(Debug, Subcommand)]
pub enum AutomationCommand {
    /// Replace a Track's automation lane for one parameter with the supplied points.
    ///
    /// points is a JSON array of objects holding id, tick, and value. id must be non-empty and
    /// unique within the lane. tick is an absolute timeline tick and duplicates fail. Points are
    /// ordered by tick. volume values are dB clamped to -90..=24; pan values are clamped to
    /// -1.0..=1.0; non-finite values fail. At most 16384 points are accepted. The lane holds
    /// only the supplied points: an empty array deletes the lane. Where a non-empty lane is
    /// active, its values replace the Track's static gain or pan; outside the lane the static
    /// values apply. Mute and solo decide output independently of automation.
    Set(AutomationSetArgs),
    /// Delete a Track's automation lane for one parameter.
    Clear(AutomationClearArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSetArgs {
    /// Id of the Track that owns the automation lane.
    #[arg(long)]
    pub track_id: String,
    /// Automated Track parameter: `volume` in dB or `pan` as a unit value.
    #[arg(long, value_parser = ["volume", "pan"])]
    pub parameter: String,
    /// JSON array of point objects; each element is an object with id (string, unique, non-empty), tick (absolute timeline tick), and value (volume: dB clamped to -90..=24, pan: clamped to -1.0..=1.0); an empty array deletes the lane; at most 16384 points.
    #[arg(long)]
    pub points_json: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationClearArgs {
    /// Id of the Track that owns the automation lane.
    #[arg(long)]
    pub track_id: String,
    /// Automated Track parameter whose lane is deleted.
    #[arg(long, value_parser = ["volume", "pan"])]
    pub parameter: String,
}

#[derive(Debug, Subcommand)]
pub enum AssetCommand {
    /// Import a MIDI file from disk into the Asset library.
    ImportMidi(AssetImportMidiArgs),
    /// Start live playback of an imported Asset through the audio engine; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode.
    Preview(AssetPreviewArgs),
    /// Stop the currently playing Asset preview; requires a running Riffra Host accessed with --attach.
    StopPreview,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetImportMidiArgs {
    /// Path to the MIDI file to import.
    pub path: PathBuf,
    /// Library display name for the imported Asset; defaults to the file name.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPreviewArgs {
    /// Id of the imported Asset to play.
    #[arg(long)]
    pub asset_id: String,
    /// Preview start offset in milliseconds from the beginning of the Asset; defaults to 0.
    #[arg(long, default_value_t = 0)]
    pub start_ms: u64,
    /// Optional exclusive preview end offset in milliseconds; omit to play to the end of the Asset.
    #[arg(long)]
    pub end_ms: Option<u64>,
    /// Loop the preview range; defaults to false when omitted.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looped: Option<bool>,
    /// Linear playback gain applied to the preview; 1.0 is unity gain.
    #[arg(long, default_value_t = 1.0)]
    pub gain: f32,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// List available projects with their ids and names.
    List,
    /// Create a new project and return its id.
    Create(ProjectCreateArgs),
    /// Open an existing project by id and make it active.
    Open(ProjectOpenArgs),
    /// Rename the active project.
    Rename(ProjectRenameArgs),
    /// Export the active project to an output path.
    Export(ProjectExportArgs),
    /// Import a project file from disk and make it active.
    Import(ProjectImportArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreateArgs {
    /// Display name for the new project; omit to use a generated default.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOpenArgs {
    /// Id of the project to open, as reported by `project list`.
    pub project_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRenameArgs {
    /// New display name for the active project.
    pub name: String,
}

#[derive(Debug, Args, Serialize)]
pub struct ProjectImportArgs {
    /// Path to the project file to import.
    pub path: PathBuf,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExportArgs {
    /// Output path for the exported project file.
    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum InstrumentCommand {
    /// Create a draft instrument definition, or forward all arguments to the bundled Sonalloy CLI when arguments are already supplied.
    ///
    /// Requires --data-root when creating a draft. Additional arguments and --help are forwarded
    /// opaquely to the bundled Sonalloy CLI rather than described by Riffra.
    Init(SonalloyArgs),
    /// Validate a Sonalloy instrument definition; arguments are forwarded to the bundled Sonalloy CLI.
    Validate(SonalloyArgs),
    /// Inspect a Sonalloy instrument definition; arguments are forwarded to the bundled Sonalloy CLI.
    Inspect(SonalloyArgs),
    /// Render instrument audio through the bundled Sonalloy renderer; arguments are forwarded opaquely.
    Render(SonalloyArgs),
    /// Audition instrument audio through the bundled Sonalloy renderer; arguments are forwarded opaquely.
    Audition(SonalloyArgs),
    /// List built-in and User Instruments available to apply to Tracks.
    List,
    /// Save a Sonalloy definition package as a User Instrument.
    Save(InstrumentSaveArgs),
    /// Export a User Instrument package to an output path.
    Export(InstrumentExportArgs),
    /// Apply a built-in or User Instrument to an Instrument Track.
    Apply(InstrumentApplyArgs),
    /// Clear the Instrument assignment from an Instrument Track.
    Clear(IdArg),
}

#[derive(Debug, Args)]
pub struct SonalloyArgs {
    /// Opaque arguments forwarded to the bundled Sonalloy CLI; use --help after the subcommand to see Sonalloy's own option list.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<OsString>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentSaveArgs {
    /// Path to the Sonalloy definition JSON to save as a User Instrument.
    pub definition_path: PathBuf,
    /// Explicit User Instrument id; omit to allocate one.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument_id: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentExportArgs {
    /// Id of the User Instrument to export, such as `user:<id>`.
    #[arg(long)]
    pub instrument_id: String,
    /// Output path for the exported Instrument package.
    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentApplyArgs {
    /// Id of the Instrument Track that receives the Instrument.
    #[arg(long)]
    pub track_id: String,
    /// Instrument id: `builtin:<preset-id>` for a built-in Instrument or `user:<id>` for a User Instrument.
    #[arg(long)]
    pub instrument_id: String,
}

#[derive(Debug, Subcommand)]
pub enum EffectCommand {
    /// Remove one effect device from a Track rack.
    Remove(EffectRemoveArgs),
    /// Reorder the effect devices in a Track rack to the supplied id order.
    Reorder(EffectReorderArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectRemoveArgs {
    /// Id of the Track whose rack is modified.
    #[arg(long)]
    pub track_id: String,
    /// Id of the effect device to remove.
    #[arg(long)]
    pub device_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectReorderArgs {
    /// Id of the Track whose rack is reordered.
    #[arg(long)]
    pub track_id: String,
    /// Comma-separated device ids in the desired order; mutually exclusive with --device-ids-json.
    #[arg(long, value_delimiter = ',')]
    pub device_ids: Vec<String>,
    /// JSON array of device ids in the desired order as an alternative to --device-ids; the two cannot be combined.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_ids_json: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    /// Bypass or unbypass one device in a Track rack.
    Bypass(DeviceBypassArgs),
    /// Inspect one device's identity and state; requires a running Riffra Host accessed with --attach.
    Inspect(DeviceInspectArgs),
    /// Read and write device parameters by index.
    Parameter {
        #[command(subcommand)]
        command: DeviceParameterCommand,
    },
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceBypassArgs {
    /// Id of the Track that owns the device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the device to bypass or unbypass.
    #[arg(long)]
    pub device_id: String,
    /// Bypass state: true bypasses the device, false re-enables it; defaults to false when omitted.
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypassed: Option<bool>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceParameterSetArgs {
    /// Id of the Track that owns the device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the device whose parameter is written.
    #[arg(long)]
    pub device_id: String,
    /// Zero-based parameter index on the device.
    #[arg(long)]
    pub parameter_index: u32,
    /// New parameter value; values are clamped to the device's 0.0..=1.0 parameter range.
    #[arg(long)]
    pub value: f32,
}

#[derive(Debug, Subcommand)]
pub enum DeviceParameterCommand {
    /// List a device's parameters with their indices and current values; requires a running Riffra Host accessed with --attach.
    List(DeviceParameterListArgs),
    /// Read one device parameter by index; requires a running Riffra Host accessed with --attach.
    Get(DeviceParameterGetArgs),
    /// Write one device parameter by index.
    Set(DeviceParameterSetArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInspectArgs {
    /// Id of the Track that owns the device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the device to inspect.
    #[arg(long)]
    pub device_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceParameterListArgs {
    /// Id of the Track that owns the device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the device whose parameters are listed.
    #[arg(long)]
    pub device_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceParameterGetArgs {
    /// Id of the Track that owns the device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the device whose parameter is read.
    #[arg(long)]
    pub device_id: String,
    /// Zero-based parameter index on the device.
    #[arg(long)]
    pub parameter_index: u32,
}

#[derive(Debug, Subcommand)]
pub enum RuntimeCommand {
    /// Read or retry the audio runtime projection state.
    Projection {
        #[command(subcommand)]
        command: RuntimeProjectionCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum RuntimeProjectionCommand {
    /// Report the current runtime projection status; requires a running Riffra Host accessed with --attach.
    Get,
    /// Resubmit the current project state to the audio runtime; requires --attach and is unavailable in Safe Mode, which keeps the runtime projection offline.
    Retry,
}

#[derive(Debug, Subcommand)]
pub enum TransportCommand {
    /// Start transport playback from the current position; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps playback offline.
    Play,
    /// Stop transport playback; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps playback offline.
    Stop,
    /// Stop playback and return the playhead to the arrangement start; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps playback offline.
    GoToStart,
    /// Move the playhead to an absolute arrangement tick; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps playback offline.
    Seek(SeekArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekArgs {
    /// Absolute arrangement position in timeline ticks; the project timebase defines ticks per beat.
    #[arg(long)]
    pub tick: u64,
}

#[derive(Debug, Subcommand)]
pub enum LiveMidiCommand {
    /// Send raw MIDI bytes to a Track's live output; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps MIDI output offline.
    Send(LiveMidiSendArgs),
    /// Send an all-notes-off panic to a Track's live output; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps MIDI output offline.
    Panic(IdArg),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveMidiSendArgs {
    /// Id of the Track that receives the live MIDI bytes.
    #[arg(long)]
    pub track_id: String,
    /// Comma-separated MIDI status and data bytes, for example 144,60,100 for note-on on channel 1.
    #[arg(long, value_delimiter = ',')]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Subcommand)]
pub enum AudioCommand {
    /// Report the live audio engine status; requires a running Riffra Host accessed with --attach.
    Status,
    /// Probe available audio devices; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps device probing offline.
    Probe,
    /// Probe channel counts for a specific driver and device pair; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps channel probing offline.
    ChannelsProbe(AudioChannelsProbeArgs),
    /// Print audio engine diagnostics; requires a running Riffra Host accessed with --attach.
    ///
    /// A failure response from the Host is returned as structured JSON on stdout with a non-zero
    /// exit status.
    Diagnostics(AudioDiagnosticsArgs),
    /// Read or set the audio driver and device selection.
    Driver {
        #[command(subcommand)]
        command: AudioDriverCommand,
    },
    /// Recover a lost or failed audio device; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which isolates external audio devices.
    Recover,
    /// Retry audio runtime startup after a failed start; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which isolates external audio devices.
    StartupRetry,
}

#[derive(Debug, Args)]
pub struct AudioDiagnosticsArgs {
    /// Emit the machine-readable diagnostic object without the control envelope.
    #[arg(long)]
    pub json: bool,
    /// Include unstable lifecycle and graph details for focused debugging.
    #[arg(long)]
    pub debug: bool,
}

#[derive(Debug, Subcommand)]
pub enum AudioDriverCommand {
    /// Read the current audio driver preferences; requires a running Riffra Host accessed with --attach.
    Get,
    /// Set the audio driver, devices, sample rate, and buffer size; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which isolates external audio devices.
    Set(AudioDriverArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioChannelsProbeArgs {
    /// Audio driver name to probe, such as the platform's default driver.
    #[arg(long)]
    pub driver: String,
    /// Input device name to probe on the driver.
    #[arg(long)]
    pub input_device: String,
    /// Output device name to probe on the driver.
    #[arg(long)]
    pub output_device: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDriverArgs {
    /// Audio driver name to activate, such as the platform's default driver.
    #[arg(long)]
    pub driver: String,
    /// Input device name; omit to leave the current input selection unchanged.
    #[arg(long)]
    pub input_device: Option<String>,
    /// Zero-based input channel index to use; defaults to 0.
    #[arg(long, default_value_t = 0)]
    pub input_channel: u32,
    /// Output device name; omit to leave the current output selection unchanged.
    #[arg(long)]
    pub output_device: Option<String>,
    /// Sample rate in Hz; omit to keep the current rate.
    #[arg(long)]
    pub sample_rate: Option<u32>,
    /// Buffer size in frames; omit to keep the current size.
    #[arg(long)]
    pub buffer_size: Option<u32>,
}

#[derive(Debug, Subcommand)]
pub enum RecordCommand {
    /// Start recording into a new take; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps recording input offline.
    Start(RecordStartArgs),
    /// Start another take in the current recording session; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which keeps recording input offline.
    AnotherTake(RecordStartArgs),
    /// Stop recording and finalize the current take; requires a running Riffra Host accessed with --attach.
    Stop,
    /// Report the current recording status; requires a running Riffra Host accessed with --attach.
    Status,
    /// List recorded takes with an optional free-text query; requires a running Riffra Host accessed with --attach.
    List(RecordListArgs),
    /// Rename one recorded take; requires a running Riffra Host accessed with --attach.
    Rename(RecordRenameArgs),
    /// Archive one recorded take; requires a running Riffra Host accessed with --attach.
    Archive(RecordIdArgs),
    /// Promote one recorded take to be the active take for its slot; requires a running Riffra Host accessed with --attach.
    Promote(RecordIdArgs),
    /// Attach or update a tag or note on one recorded take; requires a running Riffra Host accessed with --attach.
    Tag(RecordTagArgs),
    /// Delete one recorded take; requires a running Riffra Host accessed with --attach.
    Delete(RecordIdArgs),
    /// List duplicate recorded takes; requires a running Riffra Host accessed with --attach.
    Duplicates,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordStartArgs {
    /// Existing recording session id to continue with another take; omit to start a new recording session.
    #[arg(long)]
    pub recording_session_id: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordListArgs {
    /// Free-text query matched against take names and notes; omit to list all takes.
    #[arg(long)]
    pub query: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordIdArgs {
    /// Id of the recorded take.
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordRenameArgs {
    /// Id of the recorded take to rename.
    #[arg(long)]
    pub id: String,
    /// New display name for the take.
    #[arg(long)]
    pub new_name: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordTagArgs {
    /// Id of the recorded take to tag.
    #[arg(long)]
    pub id: String,
    /// Tag to attach; omit to leave tags unchanged.
    #[arg(long)]
    pub tag: Option<String>,
    /// Free-form note to attach; omit to leave notes unchanged.
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum LibraryCommand {
    /// Search the Asset library by free text; requires a running Riffra Host accessed with --attach.
    Search(LibrarySearchArgs),
    /// Update the tag or note on a library Asset; requires a running Riffra Host accessed with --attach.
    AssetUpdate(LibraryAssetUpdateArgs),
    /// List library Assets related to one Asset; requires a running Riffra Host accessed with --attach.
    Related(LibraryIdArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySearchArgs {
    /// Free-text search query matched against Asset metadata.
    #[arg(long)]
    pub query: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAssetUpdateArgs {
    /// Id of the library Asset to update.
    #[arg(long)]
    pub id: String,
    /// New tag value; omit to leave the tag unchanged.
    #[arg(long)]
    pub tag: Option<String>,
    /// New note value; omit to leave the note unchanged.
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIdArgs {
    /// Id of the library Asset whose relations are read.
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Subcommand)]
pub enum AnalysisCommand {
    /// Start background analysis of an Asset as a Job; requires a running Riffra Host accessed with --attach.
    Start(AnalysisStartArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStartArgs {
    /// Id of an already imported Asset to analyze; mutually exclusive with --path.
    #[arg(long)]
    pub asset_id: Option<String>,
    /// File path to analyze without pre-importing it; mutually exclusive with --asset-id.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Read the discovered VST3 plugin catalog.
    Catalog {
        #[command(subcommand)]
        command: PluginCatalogCommand,
    },
    /// Load or replace a VST3 instrument plugin on an Instrument Track.
    Instrument(PluginPathArgs),
    /// Add a VST3 effect plugin to a Track rack.
    Effect(PluginPathArgs),
    /// Scan a directory for VST3 plugins and wait for the report; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which blocks VST3 discovery and load validation.
    Scan(PluginScanArgs),
    /// Start a background VST3 plugin scan as a Job; requires a running Riffra Host accessed with --attach and is unavailable in Safe Mode, which blocks VST3 discovery and load validation.
    ScanStart(PluginScanArgs),
    /// List, read, and set presets on a plugin device.
    Preset {
        #[command(subcommand)]
        command: PluginPresetCommand,
    },
    /// Save and load a plugin device's full state to and from disk.
    State {
        #[command(subcommand)]
        command: PluginStateCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCatalogCommand {
    /// List the discovered VST3 plugin catalog; requires a running Riffra Host accessed with --attach.
    List,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPathArgs {
    /// Id of the Track that hosts the plugin device.
    #[arg(long)]
    pub track_id: String,
    /// Filesystem path of the VST3 plugin to load.
    #[arg(long)]
    pub plugin_path: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginScanArgs {
    /// Directory to scan for VST3 plugins; omit to scan the platform default plugin root.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum PluginPresetCommand {
    /// List a plugin device's presets; requires a running Riffra Host accessed with --attach.
    List(PluginDeviceArgs),
    /// Read a plugin device's current preset; requires a running Riffra Host accessed with --attach.
    Get(PluginDeviceArgs),
    /// Set a plugin device's preset by name or index; requires a running Riffra Host accessed with --attach.
    Set(PluginPresetSetArgs),
}

#[derive(Debug, Subcommand)]
pub enum PluginStateCommand {
    /// Read a plugin device's full state and write it to an output file; requires a running Riffra Host accessed with --attach.
    Save(PluginStateSaveArgs),
    /// Load a plugin device's full state from a JSON file; requires a running Riffra Host accessed with --attach.
    Load(PluginStateLoadArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDeviceArgs {
    /// Id of the Track that hosts the plugin device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the plugin device.
    #[arg(long)]
    pub device_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPresetSetArgs {
    /// Id of the Track that hosts the plugin device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the plugin device whose preset is set.
    #[arg(long)]
    pub device_id: String,
    /// Preset name to activate; mutually exclusive with --preset-index, and exactly one of the two is required.
    #[arg(long)]
    pub preset: Option<String>,
    /// Zero-based preset index to activate; mutually exclusive with --preset, and exactly one of the two is required.
    #[arg(long)]
    pub preset_index: Option<u32>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStateSaveArgs {
    /// Id of the Track that hosts the plugin device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the plugin device whose state is saved.
    #[arg(long)]
    pub device_id: String,
    /// Output path for the pretty-printed state JSON.
    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStateLoadArgs {
    /// Id of the Track that hosts the plugin device.
    #[arg(long)]
    pub track_id: String,
    /// Id of the plugin device whose state is replaced.
    #[arg(long)]
    pub device_id: String,
    /// Path to the state JSON file to load.
    #[arg(long)]
    pub file: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum MissingCommand {
    /// List missing Assets and plugins referenced by the project; requires a running Riffra Host accessed with --attach.
    List,
    /// Relink a missing Asset to a new file path.
    Relink(MissingRelinkArgs),
    /// Disable a missing plugin device so the project can open without it.
    DisablePlugin(DeviceIdArg),
    /// Replace a missing plugin device with a different plugin path.
    ReplacePlugin(MissingPluginReplaceArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingRelinkArgs {
    /// Id of the missing Asset to relink.
    #[arg(long)]
    pub asset_id: String,
    /// New filesystem path for the Asset.
    #[arg(long)]
    pub new_path: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdArg {
    /// Id of the plugin device.
    #[arg(long)]
    pub device_id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingPluginReplaceArgs {
    /// Id of the missing plugin device to replace.
    #[arg(long)]
    pub device_id: String,
    /// New filesystem path of the replacement VST3 plugin.
    #[arg(long)]
    pub new_path: String,
}

#[derive(Debug, Subcommand)]
pub enum RenderCommand {
    /// Render a range, the whole arrangement, or one Track stem to an audio Asset as a background Job; requires a running Riffra Host accessed with --attach.
    ///
    /// --range selects `entire-arrangement` (default) or `loop-range`. Supplying --start and --end
    /// together performs a time-selection render of that arrangement-absolute musical range and
    /// cannot be combined with `loop-range`; --start and --end must be provided together. --track-id
    /// renders only that Track as a stem. --normalize applies peak normalization. --expected-sequence
    /// is the sequence precondition for the project state being rendered. Success returns a Job;
    /// under an attached one-shot, use `job wait` to follow it to a terminal state. `job wait`
    /// without --timeout-ms waits until a terminal state, and only a supplied --timeout-ms ends
    /// the wait early.
    Start(RenderStartArgs),
}

#[derive(Debug, Args)]
pub struct RenderStartArgs {
    /// Render range: `entire-arrangement` (default) or `loop-range`; supplying --start and --end switches to a time-selection render, which cannot be combined with `loop-range`.
    #[arg(long, default_value = "entire-arrangement", value_parser = ["entire-arrangement", "loop-range"])]
    pub range: String,
    /// Time-selection start as an arrangement-absolute musical position in bar:beat or bar:beat+fraction notation; must be provided together with --end.
    #[arg(long)]
    pub start: Option<String>,
    /// Time-selection end as an arrangement-absolute musical position in bar:beat or bar:beat+fraction notation; must be provided together with --start.
    #[arg(long)]
    pub end: Option<String>,
    /// Apply peak normalization to the rendered audio; defaults to false when omitted.
    #[arg(long)]
    pub normalize: Option<bool>,
    /// Render only this Track as a stem instead of the full mix.
    #[arg(long)]
    pub track_id: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum JobCommand {
    /// Read one background Job's state by id; requires a running Riffra Host accessed with --attach.
    Get(JobIdArgs),
    /// Cancel one background Job by id; requires a running Riffra Host accessed with --attach.
    Cancel(JobIdArgs),
    /// Poll a background Job until it reaches a terminal state; requires --attach and cannot be combined with --data-root or --expected-sequence.
    ///
    /// Without --timeout-ms the wait continues until the Job is completed, failed, or cancelled.
    /// A terminal Host failure is returned as structured JSON on stdout with a non-zero exit status.
    Wait(JobWaitArgs),
}

#[derive(Debug, Args, Serialize)]
pub struct JobIdArgs {
    /// Id of the background Job.
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Args)]
pub struct JobWaitArgs {
    /// Id of the background Job to wait for.
    #[arg(long)]
    pub id: String,
    /// Maximum time to wait in milliseconds; omit to wait until the Job reaches a terminal state.
    #[arg(long)]
    pub timeout_ms: Option<u64>,
}

impl Cli {
    pub fn request(self) -> Result<ControlCommand, String> {
        let command = self
            .command
            .ok_or_else(|| "a command is required unless --interactive is used".to_string())?;
        command_request(command)
    }

    pub(crate) fn plugin_state_save_output(&self) -> Option<PathBuf> {
        match self.command.as_ref() {
            Some(CliCommand::Plugin {
                command:
                    PluginCommand::State {
                        command: PluginStateCommand::Save(args),
                    },
            }) => Some(args.output.clone()),
            _ => None,
        }
    }

    pub(crate) fn audio_diagnostics_options(&self) -> Option<(bool, bool)> {
        match self.command.as_ref() {
            Some(CliCommand::Audio {
                command: AudioCommand::Diagnostics(args),
            }) => Some((args.json, args.debug)),
            _ => None,
        }
    }

    pub(crate) fn batch_operation_lines(&self) -> Result<Option<Vec<usize>>, String> {
        let Some(CliCommand::Session {
            command: SessionCommand::Apply(args),
        }) = self.command.as_ref()
        else {
            return Ok(None);
        };
        let contents = fs::read_to_string(&args.file)
            .map_err(|error| format!("session apply file could not be read: {error}"))?;
        Ok(Some(
            contents
                .lines()
                .enumerate()
                .filter_map(|(index, line)| (!line.trim().is_empty()).then_some(index + 1))
                .collect(),
        ))
    }
}

fn command_request(command: CliCommand) -> Result<ControlCommand, String> {
    let request = match command {
        CliCommand::Serve(_) => {
            return Err("serve is a process mode and cannot be used as a one-shot command".into());
        }
        CliCommand::Host { command } => match command {
            HostCommand::List => {
                return Err("host list is handled locally by the CLI".into());
            }
            HostCommand::Status => simple("host.status"),
            HostCommand::Shutdown => simple("host.shutdown"),
        },
        CliCommand::Session { command } => match command {
            SessionCommand::Get => simple("session.get"),
            SessionCommand::Inspect(args) => value("session.inspect", args),
            SessionCommand::Apply(args) => session_apply(args)?,
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
            MidiNoteCommand::Insert(args) => note_source_command(
                "midi-note.insert",
                args.clip_id,
                args.notes_json,
                args.notes_file,
                args.stdin,
            )?,
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
                MusicMidiClipCommand::Resize(args) => {
                    if args.start.is_none() && args.end.is_none() {
                        return Err("--start or --end is required for clip resize".into());
                    }
                    value(
                        "music.midi-clip.resize",
                        json!({
                            "clipId": args.clip_id,
                            "start": args.start,
                            "end": args.end,
                        }),
                    )
                }
            },
            MusicCommand::Note { command } => match command {
                MusicNoteCommand::List(args) => value("music.note.list", args),
                MusicNoteCommand::Get(args) => value("music.note.get", args),
                MusicNoteCommand::Insert(args) => note_source_command(
                    "music.note.insert",
                    args.clip_id,
                    args.notes_json,
                    args.notes_file,
                    args.stdin,
                )?,
                MusicNoteCommand::Update(args) => value("music.note.update", args),
                MusicNoteCommand::Remove(args) => value("music.note.remove", args),
                MusicNoteCommand::Transform(args) => value("music.note.transform", args),
            },
            MusicCommand::Region { command } => match command {
                MusicRegionCommand::List => simple("music.region.list"),
                MusicRegionCommand::Add(args) => value("music.region.add", args),
                MusicRegionCommand::Update(args) => value("music.region.update", args),
                MusicRegionCommand::Remove(args) => value("music.region.remove", args),
            },
            MusicCommand::Harmony { command } => match command {
                MusicHarmonyCommand::Resolve(args) => value("music.harmony.resolve", args),
                MusicHarmonyCommand::List => simple("music.harmony.list"),
                MusicHarmonyCommand::Insert(args) => harmony_insert(args)?,
                MusicHarmonyCommand::Update(args) => harmony_update(args)?,
                MusicHarmonyCommand::Remove(args) => {
                    let ids = serde_json::from_str::<Vec<String>>(&args.event_ids_json)
                        .map_err(|error| format!("--event-ids-json is invalid JSON: {error}"))?;
                    value("music.harmony.remove", json!({"eventIds": ids}))
                }
                MusicHarmonyCommand::Realize(args) => harmony_realize(args)?,
            },
            MusicCommand::Phrase { command } => match command {
                MusicPhraseCommand::Insert(args) => phrase_insert(args)?,
                MusicPhraseCommand::Preview(args) => phrase_preview(args)?,
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
            MarkerCommand::Add(args) => value(
                "marker.add",
                json!({"name": args.name, "position": args.position}),
            ),
            MarkerCommand::Update(args) => value(
                "marker.update",
                json!({
                    "markerId": args.marker_id,
                    "name": args.name,
                    "position": args.position,
                }),
            ),
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
            ProjectCommand::List => simple("project.list"),
            ProjectCommand::Create(args) => value("project.create", args),
            ProjectCommand::Open(args) => {
                value("project.open", json!({"projectId": args.project_id}))
            }
            ProjectCommand::Rename(args) => value("project.rename", args),
            ProjectCommand::Export(args) => value("project.export", args),
            ProjectCommand::Import(args) => value("project.import", json!({"path":args.path})),
        },
        CliCommand::Instrument { command } => match command {
            InstrumentCommand::Init(_)
            | InstrumentCommand::Validate(_)
            | InstrumentCommand::Inspect(_)
            | InstrumentCommand::Render(_)
            | InstrumentCommand::Audition(_) => {
                unreachable!("Sonalloy instrument commands are handled directly by the CLI")
            }
            InstrumentCommand::List => simple("instrument.list"),
            InstrumentCommand::Save(args) => value("instrument.save", args),
            InstrumentCommand::Export(args) => value("instrument.export", args),
            InstrumentCommand::Apply(args) => value("instrument.apply", args),
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
            DeviceCommand::Inspect(args) => value("device.inspect", args),
            DeviceCommand::Parameter { command } => match command {
                DeviceParameterCommand::List(args) => value("device.parameter.list", args),
                DeviceParameterCommand::Get(args) => value("device.parameter.get", args),
                DeviceParameterCommand::Set(args) => value("device.parameter.set", args),
            },
        },
        CliCommand::Runtime { command } => match command {
            RuntimeCommand::Projection { command } => match command {
                RuntimeProjectionCommand::Get => simple("runtime.projection.get"),
                RuntimeProjectionCommand::Retry => simple("runtime.projection.retry"),
            },
        },
        CliCommand::Transport { command } => match command {
            TransportCommand::Play => simple("transport.play"),
            TransportCommand::Stop => simple("transport.stop"),
            TransportCommand::GoToStart => simple("transport.go-to-start"),
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
            AudioCommand::Diagnostics(args) => {
                value("audio.diagnostics", json!({"debug": args.debug}))
            }
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
            PluginCommand::Instrument(args) => value("instrument.vst3.set", args),
            PluginCommand::Effect(args) => value("effect.add", args),
            PluginCommand::Scan(args) => value("plugin.scan", args),
            PluginCommand::ScanStart(args) => value("plugin.scan.start", args),
            PluginCommand::Preset { command } => match command {
                PluginPresetCommand::List(args) => value("plugin.preset.list", args),
                PluginPresetCommand::Get(args) => value("plugin.preset.get", args),
                PluginPresetCommand::Set(args) => plugin_preset_set(args)?,
            },
            PluginCommand::State { command } => match command {
                PluginStateCommand::Save(args) => value(
                    "plugin.state.get",
                    json!({
                        "trackId": args.track_id,
                        "deviceId": args.device_id,
                    }),
                ),
                PluginStateCommand::Load(args) => plugin_state_load(args)?,
            },
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
            JobCommand::Wait(_) => {
                return Err("job wait is handled locally by an attached one-shot CLI".into());
            }
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
    let has_start = args.start.is_some();
    let has_end = args.end.is_some();
    if has_start != has_end {
        return Err("--start and --end must be provided together".into());
    }
    let range = match (args.range.as_str(), has_start) {
        ("entire-arrangement", false) => json!({"kind": "entireArrangement"}),
        ("loop-range", false) => json!({"kind": "loopRange"}),
        ("entire-arrangement", true) => json!({
            "kind": "timeSelection",
            "start": args.start.expect("start was checked"),
            "end": args.end.expect("end was checked"),
        }),
        ("loop-range", true) => {
            return Err("--range loop-range cannot be combined with --start or --end".into());
        }
        (other, _) => {
            return Err(format!(
                "--range must be entire-arrangement or loop-range (got {other})"
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

fn note_source_command(
    command: &str,
    clip_id: String,
    notes_json: Option<String>,
    notes_file: Option<PathBuf>,
    use_stdin: bool,
) -> Result<ControlCommand, String> {
    let notes = json_source(notes_json, notes_file, use_stdin, "notes", true)?
        .expect("required JSON source is present");
    if !notes.is_array() {
        return Err("note input must be a JSON array".into());
    }
    Ok(value(command, json!({"clipId": clip_id, "notes": notes})))
}

fn harmony_insert(args: MusicalHarmonyInsertArgs) -> Result<ControlCommand, String> {
    let events = json_source(args.events_json, args.events_file, false, "events", true)?
        .expect("required JSON source is present");
    if !events.is_array() {
        return Err("events input must be a JSON array".into());
    }
    Ok(value("music.harmony.insert", json!({"events": events})))
}

fn json_source(
    inline: Option<String>,
    file: Option<PathBuf>,
    use_stdin: bool,
    field: &str,
    required: bool,
) -> Result<Option<Value>, String> {
    let source_names = if use_stdin {
        format!("--{field}-json, --{field}-file, or --stdin")
    } else {
        format!("--{field}-json or --{field}-file")
    };
    let source_count =
        usize::from(inline.is_some()) + usize::from(file.is_some()) + usize::from(use_stdin);
    if source_count > 1 {
        return Err(format!("only one of {source_names} may be specified"));
    }
    if required && source_count == 0 {
        return Err(format!("one of {source_names} is required"));
    }
    if source_count == 0 {
        return Ok(None);
    }
    let encoded = if let Some(inline) = inline {
        inline
    } else if let Some(file) = file {
        std::fs::read_to_string(&file)
            .map_err(|error| format!("--{field}-file could not be read: {error}"))?
    } else {
        let mut encoded = String::new();
        std::io::stdin()
            .read_to_string(&mut encoded)
            .map_err(|error| format!("--stdin could not be read: {error}"))?;
        encoded
    };
    serde_json::from_str::<Value>(&encoded)
        .map(Some)
        .map_err(|error| format!("{field} input is invalid JSON: {error}"))
}

fn plugin_preset_set(args: PluginPresetSetArgs) -> Result<ControlCommand, String> {
    if args.preset.is_some() == args.preset_index.is_some() {
        return Err(
            "--preset and --preset-index are mutually exclusive and one is required".into(),
        );
    }
    Ok(value(
        "plugin.preset.set",
        json!({
            "trackId": args.track_id,
            "deviceId": args.device_id,
            "preset": args.preset,
            "presetIndex": args.preset_index,
        }),
    ))
}

fn plugin_state_load(args: PluginStateLoadArgs) -> Result<ControlCommand, String> {
    let encoded = std::fs::read_to_string(&args.file)
        .map_err(|error| format!("--file could not be read: {error}"))?;
    let state = serde_json::from_str::<Value>(&encoded)
        .map_err(|error| format!("--file is invalid JSON: {error}"))?;
    Ok(value(
        "plugin.state.set",
        json!({
            "trackId": args.track_id,
            "deviceId": args.device_id,
            "state": state,
        }),
    ))
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

fn harmony_update(args: MusicalHarmonyUpdateArgs) -> Result<ControlCommand, String> {
    let patch = serde_json::from_str::<Value>(&args.patch_json)
        .map_err(|error| format!("--patch-json is invalid JSON: {error}"))?;
    let mut params = json!({"eventId": args.event_id});
    let object = patch
        .as_object()
        .ok_or_else(|| "--patch-json must contain a JSON object".to_string())?;
    if object.contains_key("eventId") {
        return Err("--patch-json must not contain eventId".into());
    }
    params
        .as_object_mut()
        .expect("object literal produces an object")
        .extend(object.clone());
    Ok(value("music.harmony.update", params))
}

fn harmony_realize(args: MusicalHarmonyRealizeArgs) -> Result<ControlCommand, String> {
    let mut params = serde_json::Map::new();
    params.insert("clipId".into(), Value::String(args.clip_id));
    if let Some(start) = args.start {
        params.insert("start".into(), Value::String(start));
    }
    if let Some(end) = args.end {
        params.insert("end".into(), Value::String(end));
    }
    if let Some(lowest_octave) = args.lowest_octave {
        params.insert("lowestOctave".into(), json!(lowest_octave));
    }
    if let Some(rhythm) = json_source(args.rhythm_json, args.rhythm_file, false, "rhythm", false)? {
        params.insert("rhythm".into(), rhythm);
    }
    if let Some(velocity) = args.velocity {
        params.insert("velocity".into(), json!(velocity));
    }
    if let Some(channel) = args.channel {
        params.insert("channel".into(), json!(channel));
    }
    Ok(value("music.harmony.realize", Value::Object(params)))
}

fn session_apply(args: SessionApplyArgs) -> Result<ControlCommand, String> {
    let contents = fs::read_to_string(&args.file)
        .map_err(|error| format!("session apply file could not be read: {error}"))?;
    let mut operations = Vec::new();
    for (line_index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let operation = serde_json::from_str::<ControlCommand>(line).map_err(|error| {
            format!(
                "session apply line {} is not a valid Control Command: {error}",
                line_index + 1
            )
        })?;
        operations.push(operation);
    }
    if operations.is_empty() {
        return Err("session apply file must contain at least one command".into());
    }
    Ok(value(
        "session.apply",
        json!({
            "operations": operations,
            "includeCreatedIds": args.include_created_ids,
        }),
    ))
}

fn phrase_params(
    clip_id: String,
    phrase_json: Option<String>,
    phrase_file: Option<PathBuf>,
    channel: Option<u8>,
) -> Result<serde_json::Map<String, Value>, String> {
    let phrase = json_source(phrase_json, phrase_file, false, "phrase", true)?
        .expect("required JSON source is present");
    let mut phrase = phrase
        .as_object()
        .cloned()
        .ok_or_else(|| "phrase input must contain a JSON object".to_string())?;
    if phrase.contains_key("clipId") {
        return Err("--phrase-json must not contain clipId".into());
    }
    let pattern = phrase
        .remove("pattern")
        .ok_or_else(|| "--phrase-json must contain pattern".to_string())?;
    let placements = phrase
        .remove("placements")
        .ok_or_else(|| "--phrase-json must contain placements".to_string())?;
    if !phrase.is_empty() {
        return Err("--phrase-json may contain only pattern and placements".into());
    }
    let mut params = serde_json::Map::new();
    params.insert("clipId".into(), Value::String(clip_id));
    params.insert("pattern".into(), pattern);
    params.insert("placements".into(), placements);
    if let Some(channel) = channel {
        params.insert("channel".into(), json!(channel));
    }
    Ok(params)
}

fn phrase_insert(args: MusicalPhraseInsertArgs) -> Result<ControlCommand, String> {
    let params = phrase_params(
        args.clip_id,
        args.phrase_json,
        args.phrase_file,
        args.channel,
    )?;
    Ok(value("music.phrase.insert", Value::Object(params)))
}

fn phrase_preview(args: MusicalPhrasePreviewArgs) -> Result<ControlCommand, String> {
    let mut params = phrase_params(
        args.clip_id,
        args.phrase_json,
        args.phrase_file,
        args.channel,
    )?;
    params.insert("includeNotes".into(), json!(args.include_notes));
    Ok(value("music.phrase.preview", Value::Object(params)))
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
            "start": args.start,
            "end": args.end,
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
    use clap::{CommandFactory, Parser};

    #[test]
    fn every_command_and_option_has_help() {
        fn assert_help(command: &clap::Command, path: &str) {
            let about = command
                .get_about()
                .map(|about| about.to_string())
                .unwrap_or_default();
            assert!(
                !about.trim().is_empty(),
                "command `{path}` is missing about"
            );
            for arg in command.get_arguments() {
                if arg.get_id() == "help" || arg.get_id() == "version" {
                    continue;
                }
                if arg.get_id() == "args" && arg.is_positional() {
                    continue;
                }
                let help = arg
                    .get_help()
                    .map(|help| help.to_string())
                    .unwrap_or_default();
                assert!(
                    !help.trim().is_empty(),
                    "option `{path}` `{}` is missing help",
                    arg.get_id()
                );
            }
            for sub in command.get_subcommands() {
                let sub_path = format!("{path} {}", sub.get_name());
                assert_help(sub, &sub_path);
            }
        }

        Cli::command().debug_assert();
        assert_help(&Cli::command(), "riffra");
    }

    #[test]
    fn clip_update_patch_conflicts_with_field_flags() {
        let audio = Cli::try_parse_from([
            "riffra",
            "audio-clip",
            "update",
            "--clip-id",
            "clip:1",
            "--patch",
            "{}",
            "--name",
            "renamed",
        ]);
        assert!(audio.is_err(), "--patch must reject individual field flags");

        let midi = Cli::try_parse_from([
            "riffra",
            "midi-clip",
            "update",
            "--clip-id",
            "clip:1",
            "--patch",
            "{}",
            "--name",
            "renamed",
        ]);
        assert!(midi.is_err(), "--patch must reject individual field flags");

        let fields_only = Cli::try_parse_from([
            "riffra",
            "audio-clip",
            "update",
            "--clip-id",
            "clip:1",
            "--name",
            "renamed",
        ])
        .expect("field flags alone must parse");
        assert!(fields_only.request().is_ok());
    }

    #[test]
    fn float_options_reject_non_finite_values() {
        for arguments in [
            vec![
                "riffra",
                "session",
                "settings",
                "update",
                "--master-db",
                "NaN",
            ],
            vec![
                "riffra",
                "track",
                "update",
                "--track-id",
                "track:1",
                "--gain-db",
                "inf",
            ],
            vec![
                "riffra",
                "track",
                "update",
                "--track-id",
                "track:1",
                "--pan",
                "-inf",
            ],
            vec![
                "riffra",
                "audio-clip",
                "update",
                "--clip-id",
                "clip:1",
                "--gain-db",
                "Infinity",
            ],
            vec![
                "riffra",
                "audio-clip",
                "update",
                "--clip-id",
                "clip:1",
                "--pan",
                "NaN",
            ],
            vec!["riffra", "timebase", "update", "--bpm", "NaN"],
        ] {
            assert!(
                Cli::try_parse_from(arguments).is_err(),
                "non-finite float values must be rejected"
            );
        }

        let finite = Cli::try_parse_from([
            "riffra",
            "track",
            "update",
            "--track-id",
            "track:1",
            "--gain-db",
            "-3.5",
        ])
        .expect("finite float values must parse");
        assert!(finite.request().is_ok());
    }

    #[test]
    fn fixed_value_options_expose_possible_values() {
        fn possible_values(path: &[&str], long: &str) -> Vec<String> {
            let command = Cli::command();
            let mut current = &command;
            for name in path {
                current = current
                    .find_subcommand(name)
                    .unwrap_or_else(|| panic!("missing subcommand {name}"));
            }
            current
                .get_arguments()
                .find(|arg| arg.get_long() == Some(long))
                .unwrap_or_else(|| panic!("missing --{long}"))
                .get_possible_values()
                .iter()
                .map(|value| value.get_name().to_owned())
                .collect()
        }

        assert_eq!(
            possible_values(&["track", "add"], "kind"),
            ["audio", "instrument"]
        );
        assert_eq!(
            possible_values(&["track", "update"], "monitoring"),
            ["off", "auto", "on"]
        );
        assert_eq!(
            possible_values(&["automation", "set"], "parameter"),
            ["volume", "pan"]
        );
        assert_eq!(
            possible_values(&["automation", "clear"], "parameter"),
            ["volume", "pan"]
        );
        assert_eq!(
            possible_values(&["render", "start"], "range"),
            ["entire-arrangement", "loop-range"]
        );
    }

    #[test]
    fn instrument_commands_use_common_ids_and_keep_vst3_distinct() {
        let cli =
            Cli::try_parse_from(["riffra", "--data-root", "data", "instrument", "list"]).unwrap();
        assert_eq!(cli.request().unwrap().name, "instrument.list");

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "instrument",
            "apply",
            "--track-id",
            "track:keys",
            "--instrument-id",
            "builtin:01-clean-sub-bass",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "instrument.apply");
        assert_eq!(
            request.params,
            json!({"trackId":"track:keys","instrumentId":"builtin:01-clean-sub-bass"})
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "instrument",
            "save",
            "definition.json",
            "--instrument-id",
            "user:018f5d40-1b9e-7b9d-a70b-7a5b4f4e4c3e",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "instrument.save");
        assert_eq!(request.params["definitionPath"], "definition.json");

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "plugin",
            "instrument",
            "--track-id",
            "track:keys",
            "--plugin-path",
            "C:\\Plugins\\Keys.vst3",
        ])
        .unwrap();
        assert_eq!(cli.request().unwrap().name, "instrument.vst3.set");
    }

    #[test]
    fn preserves_cli_local_options_without_leaking_into_protocol_params() {
        let cli = Cli::try_parse_from([
            "riffra",
            "plugin",
            "state",
            "save",
            "--track-id",
            "track:keys",
            "--device-id",
            "device:synth",
            "--output",
            "state.json",
        ])
        .unwrap();

        assert_eq!(
            cli.plugin_state_save_output(),
            Some(PathBuf::from("state.json"))
        );
        let request = cli.request().unwrap();
        assert_eq!(
            request,
            ControlCommand::new(
                "plugin.state.get",
                json!({"trackId":"track:keys","deviceId":"device:synth"})
            )
        );

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

        let cli =
            Cli::try_parse_from(["riffra", "audio", "diagnostics", "--json", "--debug"]).unwrap();

        assert_eq!(cli.audio_diagnostics_options(), Some((true, true)));
        let request = cli.request().unwrap();
        assert_eq!(request.name, "audio.diagnostics");
        assert_eq!(request.params, json!({"debug": true}));
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
            "note",
            "transform",
            "--track-id",
            "track:drums",
            "--start",
            "5:1",
            "--end",
            "13:1",
            "--pitch",
            "D2",
            "--timing-offset",
            "-1/48",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "music.note.transform");
        assert_eq!(
            request.params,
            json!({
                "trackId":"track:drums",
                "start":"5:1",
                "end":"13:1",
                "pitch":"D2",
                "timingOffset":"-1/48"
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

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "session",
            "inspect",
            "--start",
            "9:1",
            "--end",
            "13:1",
            "--track-id",
            "track:keys",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "session.inspect");
        assert_eq!(
            request.params,
            json!({"start":"9:1","end":"13:1","trackId":"track:keys"})
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "render",
            "start",
            "--start",
            "9:1",
            "--end",
            "13:1",
            "--track-id",
            "track:keys",
        ])
        .unwrap();
        assert_eq!(
            cli.request().unwrap().params["options"]["range"],
            json!({"kind":"timeSelection","start":"9:1","end":"13:1"})
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "render",
            "start",
            "--range",
            "loop-range",
            "--start",
            "9:1",
            "--end",
            "13:1",
        ])
        .unwrap();
        assert!(cli.request().is_err());

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "render",
            "start",
            "--start",
            "9:1",
        ])
        .unwrap();
        assert!(cli.request().is_err());

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "music",
            "harmony",
            "insert",
            "--events-json",
            r#"[{"start":"1:1","end":"2:1","chord":"Dm9"}]"#,
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "music.harmony.insert");
        assert_eq!(
            request.params,
            json!({"events":[{"start":"1:1","end":"2:1","chord":"Dm9"}]})
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "music",
            "harmony",
            "realize",
            "--clip-id",
            "midi-clip:1",
            "--lowest-octave",
            "3",
            "--rhythm-json",
            r#"{"length":"1/2","steps":[{"offset":"0/1","duration":"1/8"}]}"#,
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "music.harmony.realize");
        assert_eq!(request.params["clipId"], "midi-clip:1");
        assert_eq!(request.params["lowestOctave"], 3);
        assert_eq!(request.params["rhythm"]["length"], "1/2");

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "music",
            "phrase",
            "insert",
            "--clip-id",
            "midi-clip:1",
            "--phrase-json",
            r#"{"pattern":{"length":"1/1","notes":[{"offset":"0/1","duration":"1/8","semitones":0}]},"placements":[{"position":"1:1","anchor":"C4","repeats":1}]}"#,
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "music.phrase.insert");
        assert_eq!(request.params["clipId"], "midi-clip:1");
        assert_eq!(request.params["pattern"]["length"], "1/1");
        assert_eq!(request.params["placements"][0]["anchor"], "C4");

        let cli = Cli::try_parse_from([
            "riffra",
            "marker",
            "add",
            "--name",
            "Chorus",
            "--position",
            "17:1",
        ])
        .unwrap();
        assert_eq!(
            cli.request().unwrap().params,
            json!({"name":"Chorus","position":"17:1"})
        );
        assert!(
            Cli::try_parse_from([
                "riffra", "marker", "add", "--name", "Chorus", "--tick", "960"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "riffra",
                "loop-range",
                "set",
                "--start-tick",
                "960",
                "--end-tick",
                "1920",
            ])
            .is_err()
        );
    }

    #[test]
    fn bulk_note_sources_are_exclusive_and_must_be_arrays() {
        let path = std::env::temp_dir().join(format!(
            "riffra-cli-notes-source-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            br#"[{"pitch":"C4","position":"1:1","duration":"1/8"}]"#,
        )
        .unwrap();

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "music",
            "note",
            "insert",
            "--clip-id",
            "midi-clip:1",
            "--notes-file",
            path.to_str().unwrap(),
        ])
        .unwrap();
        assert_eq!(
            cli.request().unwrap().params["notes"],
            json!([{"pitch":"C4","position":"1:1","duration":"1/8"}])
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
            "[]",
            "--notes-file",
            path.to_str().unwrap(),
        ])
        .unwrap();
        assert!(cli.request().is_err());

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
            "{}",
        ])
        .unwrap();
        assert!(cli.request().is_err());

        let root = std::env::temp_dir().join(format!(
            "riffra-cli-structured-sources-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let events_file = root.join("events.json");
        let rhythm_file = root.join("rhythm.json");
        let phrase_file = root.join("phrase.json");
        std::fs::write(
            &events_file,
            br#"[{"start":"1:1","end":"2:1","chord":"Dm9"}]"#,
        )
        .unwrap();
        std::fs::write(
            &rhythm_file,
            br#"{"length":"1/2","steps":[{"offset":"0/1","duration":"1/8"}]}"#,
        )
        .unwrap();
        std::fs::write(
            &phrase_file,
            br#"{"pattern":{"length":"1/1","notes":[{"offset":"0/1","duration":"1/8","semitones":0}]},"placements":[{"position":"1:1","anchor":"C4","repeats":1}]}"#,
        )
        .unwrap();

        let cli = Cli::try_parse_from([
            "riffra",
            "music",
            "harmony",
            "insert",
            "--events-file",
            events_file.to_str().unwrap(),
        ])
        .unwrap();
        assert_eq!(
            cli.request().unwrap().params,
            json!({"events":[{"start":"1:1","end":"2:1","chord":"Dm9"}]})
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "music",
            "harmony",
            "realize",
            "--clip-id",
            "midi-clip:1",
            "--rhythm-file",
            rhythm_file.to_str().unwrap(),
        ])
        .unwrap();
        assert_eq!(
            cli.request().unwrap().params["rhythm"],
            json!({"length":"1/2","steps":[{"offset":"0/1","duration":"1/8"}]})
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "music",
            "phrase",
            "insert",
            "--clip-id",
            "midi-clip:1",
            "--phrase-file",
            phrase_file.to_str().unwrap(),
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert!(request.params.get("phrase").is_none());
        assert_eq!(request.params["pattern"]["length"], "1/1");
        assert_eq!(request.params["placements"][0]["anchor"], "C4");
        assert!(
            !request
                .params
                .to_string()
                .contains(phrase_file.to_string_lossy().as_ref())
        );

        assert!(Cli::try_parse_from(["riffra", "music", "harmony", "insert", "--stdin"]).is_err());
        assert!(
            Cli::try_parse_from([
                "riffra",
                "music",
                "harmony",
                "realize",
                "--clip-id",
                "midi-clip:1",
                "--stdin",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "riffra",
                "music",
                "phrase",
                "insert",
                "--clip-id",
                "midi-clip:1",
                "--stdin",
            ])
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);

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
        assert_eq!(
            cli.request().unwrap().params,
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
        assert_eq!(
            cli.request().unwrap().params,
            json!({"clipId":"clip:1","noteIds":["note:a","note:b"]})
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn validates_musical_positions_and_command_boundaries() {
        let cli = Cli::try_parse_from([
            "riffra",
            "marker",
            "add",
            "--name",
            "Chorus",
            "--position",
            "17:1",
        ])
        .unwrap();
        assert_eq!(
            cli.request().unwrap().params,
            json!({"name":"Chorus","position":"17:1"})
        );

        assert!(
            Cli::try_parse_from([
                "riffra", "marker", "add", "--name", "Chorus", "--tick", "960",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "riffra",
                "loop-range",
                "set",
                "--start-tick",
                "960",
                "--end-tick",
                "1920",
            ])
            .is_err()
        );

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "music",
            "harmony",
            "update",
            "--event-id",
            "harmony:A",
            "--patch-json",
            r#"{"eventId":"harmony:B","chord":"C"}"#,
        ])
        .unwrap();

        assert!(cli.request().is_err());

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

        let cli = Cli::try_parse_from([
            "riffra",
            "--data-root",
            "data",
            "loop-range",
            "set",
            "--enabled",
            "true",
            "--start",
            "1:1",
            "--end",
            "2:1",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(
            request.params,
            json!({"enabled":true,"start":"1:1","end":"2:1"})
        );
        assert!(
            Cli::try_parse_from([
                "riffra",
                "--data-root",
                "data",
                "loop-range",
                "set",
                "--enabled",
                "--start",
                "1:1",
                "--end",
                "2:1",
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

    #[test]
    fn session_apply_reads_control_commands_and_options() {
        let path = std::env::temp_dir().join(format!(
            "riffra-cli-session-apply-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(
            &path,
            br#"{"command":"track.add","params":{"name":"Lead","kind":"instrument"}}

{"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"5:1"}}"#,
        )
        .unwrap();
        let cli = Cli::try_parse_from([
            "riffra",
            "session",
            "apply",
            "--file",
            path.to_str().unwrap(),
            "--include-created-ids",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "session.apply");
        assert_eq!(request.params["operations"].as_array().unwrap().len(), 2);
        assert_eq!(request.params["operations"][0]["command"], "track.add");
        assert_eq!(request.params["includeCreatedIds"], true);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn phrase_preview_keeps_preview_options() {
        let cli = Cli::try_parse_from([
            "riffra",
            "music",
            "phrase",
            "preview",
            "--clip-id",
            "midi-clip:1",
            "--phrase-json",
            r#"{"pattern":{"length":"1/1","notes":[{"offset":"0/1","duration":"1/8","semitones":0}]},"placements":[{"position":"1:1","anchor":"C4","repeats":1}]} "#,
            "--include-notes",
        ])
        .unwrap();
        let request = cli.request().unwrap();
        assert_eq!(request.name, "music.phrase.preview");
        assert_eq!(request.params["includeNotes"], true);
        assert_eq!(request.params["clipId"], "midi-clip:1");
    }

    #[test]
    fn session_apply_rejects_request_envelope_fields_in_operations() {
        for (suffix, field) in [
            ("request-id", "requestId"),
            ("sequence", "expectedSequence"),
        ] {
            let path = std::env::temp_dir().join(format!(
                "riffra-cli-session-apply-{}-{suffix}.jsonl",
                std::process::id()
            ));
            std::fs::write(
                &path,
                format!(
                    r#"{{"command":"track.add","{field}":1,"params":{{"name":"Lead","kind":"instrument"}}}}"#
                ),
            )
            .unwrap();
            let cli = Cli::try_parse_from([
                "riffra",
                "session",
                "apply",
                "--file",
                path.to_str().unwrap(),
            ])
            .unwrap();

            let error = cli.request().unwrap_err();
            assert!(error.contains("unknown field"));
            let _ = std::fs::remove_file(path);
        }
    }
}
