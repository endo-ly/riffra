use crate::AppState;
use crate::asset;
use crate::model::ArrangementMutationResult;
use crate::render::RenderOptions;
use crate::session::adapter::{self, AdapterError};
use riffra_control::{CommandResult, ControlRequest, ControlResponse, ErrorCode, ProtocolError};
use riffra_core::application::{MidiNoteInput, MidiNotePatch, MidiNoteUpdate};
use riffra_core::{
    AssetId, AudioClipMove, AudioClipPatch, AutomationParameter, AutomationPoint, FrameRange,
    MidiClipMove, MidiClipPatch, MidiInputRoute, ProjectTimebase, TimelineTick, TrackKind,
    TrackPatch,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug)]
struct RouteResult {
    result_type: &'static str,
    value: Value,
}

#[derive(Debug)]
enum RouteError {
    InvalidRequest(String),
    Conflict {
        expected_sequence: u64,
        current_sequence: u64,
    },
    CommandFailed(String),
    RuntimeUnavailable(String),
}

impl RouteError {
    fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidRequest(message.into())
    }

    fn command(message: impl Into<String>) -> Self {
        Self::CommandFailed(message.into())
    }

    fn runtime(message: impl Into<String>) -> Self {
        Self::RuntimeUnavailable(message.into())
    }

    fn conflict(expected_sequence: u64, current_sequence: u64) -> Self {
        Self::Conflict {
            expected_sequence,
            current_sequence,
        }
    }

    fn protocol_error(self) -> ProtocolError {
        match self {
            Self::InvalidRequest(message) => ProtocolError::new(ErrorCode::InvalidRequest, message),
            Self::Conflict {
                expected_sequence,
                current_sequence,
            } => ProtocolError::conflict(expected_sequence, current_sequence),
            Self::CommandFailed(message) => ProtocolError::new(ErrorCode::CommandFailed, message),
            Self::RuntimeUnavailable(message) => {
                ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
            }
        }
    }
}

impl From<AdapterError> for RouteError {
    fn from(error: AdapterError) -> Self {
        match error {
            AdapterError::Conflict {
                expected_sequence,
                current_sequence,
            } => Self::conflict(expected_sequence, current_sequence),
            AdapterError::RuntimeUnavailable(message) => Self::runtime(message),
            AdapterError::CommandFailed(message) => Self::command(message),
        }
    }
}

/// Dispatches one validated request through the same Desktop adapters used by
/// the Tauri boundary.
pub(crate) fn dispatch(state: &AppState, request: ControlRequest) -> ControlResponse {
    let request_id = request.request_id.clone();
    if let Err(error) = request.validate() {
        return ControlResponse::failure(request_id, None, error);
    }

    let gate = if requires_command_gate(&request.command) {
        match state.command_gate.lock() {
            Ok(gate) => Some(gate),
            Err(error) => {
                return ControlResponse::failure(
                    request_id,
                    None,
                    ProtocolError::new(
                        ErrorCode::CommandFailed,
                        format!("command gate failed: {error}"),
                    ),
                );
            }
        }
    } else {
        None
    };
    let current = match state.core.snapshot() {
        Ok(current) => current,
        Err(error) => {
            return ControlResponse::failure(
                request_id,
                None,
                ProtocolError::new(ErrorCode::CommandFailed, error.to_string()),
            );
        }
    };
    if let Some(expected_sequence) = request.expected_sequence
        && expected_sequence != current.sequence
    {
        return ControlResponse::failure(
            request_id,
            Some(current.sequence),
            ProtocolError::conflict(expected_sequence, current.sequence),
        );
    }

    let expected_sequence = request.expected_sequence;
    let result = route_command(
        state,
        &current.session,
        request.control_command(),
        expected_sequence,
    );
    drop(gate);
    match result {
        Ok(result) => match state.core.snapshot() {
            Ok(canonical) => ControlResponse::success(
                request_id,
                canonical.sequence,
                CommandResult {
                    result_type: result.result_type.into(),
                    value: result.value,
                },
            ),
            Err(error) => ControlResponse::failure(
                request_id,
                None,
                ProtocolError::new(ErrorCode::CommandFailed, error.to_string()),
            ),
        },
        Err(error) => {
            let current_sequence = state.core.snapshot().ok().map(|state| state.sequence);
            ControlResponse::failure(request_id, current_sequence, error.protocol_error())
        }
    }
}

fn requires_command_gate(command: &str) -> bool {
    !matches!(
        command,
        "runtime.projection.get"
            | "runtime.projection.retry"
            | "transport.play"
            | "transport.stop"
            | "transport.go-to-start"
            | "transport.seek"
            | "audio.status"
            | "midi.send"
            | "midi.panic"
            | "plugin.catalog.list"
            | "instrument.set"
            | "effect.add"
            | "missing.list"
            | "missing.replace-plugin"
            | "render.start"
            | "job.get"
            | "job.cancel"
    )
}

fn route_command(
    state: &AppState,
    session: &riffra_core::CreativeSession,
    request: riffra_control::ControlCommand,
    expected_sequence: Option<u64>,
) -> Result<RouteResult, RouteError> {
    let context = adapter::SessionContext {
        core: &state.core,
        audio: state.core.audio(),
        runtime: &state.runtime,
        data_root: state.core.data_root(),
        safe_mode: state.core.safe_mode(),
        app_handle: Some(&state.app_handle),
    };
    let params = request.params;
    match request.name.as_str() {
        "session.get" => serialized("session", session),
        "session.settings.update" => {
            mutation(adapter::update_session_settings(&context, decode(params)?))
        }
        "history.get" => serialized(
            "history",
            &state
                .core
                .canonical_state()
                .map_err(|error| RouteError::command(error.to_string()))?
                .history,
        ),
        "track.list" => serialized("tracks", &session.arrangement.tracks),
        "track.add" => {
            let params: TrackAddParams = decode(params)?;
            mutation(adapter::add_track(
                &context,
                params.name,
                parse_track_kind(&params.kind)?,
            ))
        }
        "track.update" => {
            let params: TrackUpdateParams = decode(params)?;
            mutation(adapter::update_track(
                &context,
                &params.track_id,
                params.patch,
            ))
        }
        "track.remove" => {
            let params: TrackIdParams = decode(params)?;
            mutation(adapter::remove_track(&context, &params.track_id))
        }
        "track.duplicate" => {
            let params: TrackIdParams = decode(params)?;
            mutation(adapter::duplicate_track(&context, &params.track_id))
        }
        "track.reorder" => {
            let params: ReorderParams = decode(params)?;
            mutation(adapter::reorder_track(
                &context,
                &params.track_id,
                params.target_index,
            ))
        }
        "track.audio-input.set" => {
            let params: AudioInputParams = decode(params)?;
            mutation(adapter::set_track_audio_input(
                &context,
                &params.track_id,
                Some(params.channel_index),
            ))
        }
        "track.audio-input.clear" => {
            let params: TrackIdParams = decode(params)?;
            mutation(adapter::set_track_audio_input(
                &context,
                &params.track_id,
                None,
            ))
        }
        "track.midi-input.set" => {
            let params: MidiInputParams = decode(params)?;
            mutation(adapter::set_track_midi_input(
                &context,
                &params.track_id,
                MidiInputRoute {
                    device_id: params.device_id,
                    channel: params.channel,
                },
            ))
        }
        "track.midi-input.clear" => {
            let params: TrackIdParams = decode(params)?;
            mutation(adapter::set_track_midi_input(
                &context,
                &params.track_id,
                MidiInputRoute::default(),
            ))
        }
        "audio-clip.list" => serialized("audioClips", &session.arrangement.audio_clips),
        "audio-clip.add-asset" => {
            let params: AudioAddParams = decode(params)?;
            mutation(adapter::add_audio_clip(
                &context,
                parse_asset_id(&params.asset_id)?,
                params.name,
                params.start_tick.map(TimelineTick),
                params.track_id,
            ))
        }
        "audio-clip.update" => {
            let params: ClipPatchParams<AudioClipPatch> = decode(params)?;
            mutation(adapter::update_audio_clip(
                &context,
                &params.clip_id,
                params.patch,
            ))
        }
        "audio-clip.move" => {
            let params: MovesParams<AudioClipMove> = decode(params)?;
            mutation(adapter::move_audio_clips(&context, params.moves))
        }
        "audio-clip.trim" => {
            let params: AudioTrimParams = decode(params)?;
            mutation(adapter::trim_audio_clip(
                &context,
                &params.clip_id,
                TimelineTick(params.start_tick),
                params.source_range,
            ))
        }
        "audio-clip.split" => {
            let params: SplitParams = decode(params)?;
            mutation(adapter::split_audio_clip(
                &context,
                &params.clip_id,
                TimelineTick(params.split_tick),
            ))
        }
        "audio-clip.duplicate" => {
            let params: ClipIdParams = decode(params)?;
            mutation(adapter::duplicate_audio_clip(&context, &params.clip_id))
        }
        "audio-clip.crossfade" => {
            let params: CrossfadeParams = decode(params)?;
            mutation(adapter::crossfade_audio_clips(
                &context,
                &params.first_clip_id,
                &params.second_clip_id,
            ))
        }
        "midi-clip.list" => serialized("midiClips", &session.arrangement.midi_clips),
        "midi-clip.create" => {
            let params: MidiClipCreateParams = decode(params)?;
            mutation(adapter::create_midi_clip(
                &context,
                &params.track_id,
                TimelineTick(params.start_tick),
                params.duration_ticks,
                params.name,
            ))
        }
        "midi-clip.add-asset" => {
            let params: MidiAddParams = decode(params)?;
            mutation(adapter::add_midi_clip(
                &context,
                parse_asset_id(&params.asset_id)?,
                params.name,
                params.start_tick.map(TimelineTick),
                params.track_id,
            ))
        }
        "midi-clip.update" => {
            let params: ClipPatchParams<MidiClipPatch> = decode(params)?;
            mutation(adapter::update_midi_clip(
                &context,
                &params.clip_id,
                params.patch,
            ))
        }
        "midi-clip.move" => {
            let params: MovesParams<MidiClipMove> = decode(params)?;
            mutation(adapter::move_midi_clips(&context, params.moves))
        }
        "midi-clip.trim" => {
            let params: MidiTrimParams = decode(params)?;
            mutation(adapter::trim_midi_clip(
                &context,
                &params.clip_id,
                TimelineTick(params.start_tick),
                params.duration_ticks,
            ))
        }
        "midi-clip.split" => {
            let params: SplitParams = decode(params)?;
            mutation(adapter::split_midi_clip(
                &context,
                &params.clip_id,
                TimelineTick(params.split_tick),
            ))
        }
        "midi-clip.duplicate" => {
            let params: ClipIdParams = decode(params)?;
            mutation(adapter::duplicate_midi_clip(&context, &params.clip_id))
        }
        "midi-note.add" => {
            let params: MidiNoteAddParams = decode(params)?;
            mutation(adapter::add_midi_note(
                &context,
                &params.clip_id,
                TimelineTick(params.start_tick),
                params.pitch,
                params.duration_ticks,
                params.velocity,
                params.channel,
            ))
        }
        "midi-note.insert" => {
            let params: MidiNoteInsertParams = decode(params)?;
            mutation(adapter::insert_midi_notes(
                &context,
                &params.clip_id,
                params.notes,
            ))
        }
        "midi-note.update" => {
            let params: MidiNoteUpdateParams = decode(params)?;
            mutation(adapter::update_midi_note(
                &context,
                &params.clip_id,
                &params.note_id,
                params.patch,
            ))
        }
        "midi-note.update-many" => {
            let params: MidiNoteUpdatesParams = decode(params)?;
            mutation(adapter::update_midi_notes(
                &context,
                &params.clip_id,
                params.updates,
            ))
        }
        "midi-note.remove" => {
            let params: MidiNoteIdParams = decode(params)?;
            mutation(adapter::remove_midi_note(
                &context,
                &params.clip_id,
                &params.note_id,
            ))
        }
        "midi-note.remove-many" => {
            let params: MidiNoteIdsParams = decode(params)?;
            mutation(adapter::remove_midi_notes(
                &context,
                &params.clip_id,
                &params.note_ids,
            ))
        }
        "midi-note.quantize" => {
            let params: MidiNoteQuantizeParams = decode(params)?;
            mutation(adapter::quantize_midi_notes(
                &context,
                &params.clip_id,
                &params.note_ids,
                params.grid_ticks,
            ))
        }
        "midi-note.transform" => {
            let params: MidiNoteTransformParams = decode(params)?;
            mutation(adapter::transform_midi_notes(
                &context,
                &params.clip_id,
                params.note_ids,
                params.transpose_semitones,
                params.velocity_offset,
            ))
        }
        "midi-note.duplicate" => {
            let params: MidiNoteDuplicateParams = decode(params)?;
            mutation(adapter::duplicate_midi_notes(
                &context,
                &params.clip_id,
                &params.note_ids,
                params.offset_ticks,
            ))
        }
        "clip.remove" => {
            let params: ClipRemoveParams = decode(params)?;
            mutation(adapter::remove_timeline_clips(
                &context,
                &params.audio_clip_ids,
                &params.midi_clip_ids,
            ))
        }
        "clip.paste" => {
            let params: ClipPasteParams = decode(params)?;
            mutation(adapter::paste_timeline_clips(
                &context,
                &params.audio_clip_ids,
                &params.midi_clip_ids,
                TimelineTick(params.start_tick),
            ))
        }
        "marker.add" => {
            let params: MarkerAddParams = decode(params)?;
            mutation(adapter::add_marker(
                &context,
                TimelineTick(params.tick),
                params.name,
            ))
        }
        "marker.update" => {
            let params: MarkerUpdateParams = decode(params)?;
            mutation(adapter::update_marker(
                &context,
                &params.marker_id,
                params.name,
                params.tick.map(TimelineTick),
            ))
        }
        "marker.remove" => {
            let params: MarkerIdParams = decode(params)?;
            mutation(adapter::remove_marker(&context, &params.marker_id))
        }
        "timebase.update" => {
            let params: TimebaseParams = decode(params)?;
            mutation(adapter::update_timebase(&context, params.timebase))
        }
        "loop-range.set" => {
            let params: RangeParams = decode(params)?;
            mutation(adapter::update_loop_range(
                &context,
                params.enabled,
                TimelineTick(params.start_tick),
                TimelineTick(params.end_tick),
            ))
        }
        "punch-range.set" => {
            let params: RangeParams = decode(params)?;
            mutation(adapter::update_punch_range(
                &context,
                params.enabled,
                TimelineTick(params.start_tick),
                TimelineTick(params.end_tick),
            ))
        }
        "automation.set" => {
            let params: AutomationParams = decode(params)?;
            mutation(adapter::set_track_automation(
                &context,
                &params.track_id,
                parse_automation_parameter(&params.parameter)?,
                params.points,
            ))
        }
        "automation.clear" => {
            let params: AutomationClearParams = decode(params)?;
            mutation(adapter::set_track_automation(
                &context,
                &params.track_id,
                parse_automation_parameter(&params.parameter)?,
                Vec::new(),
            ))
        }
        "asset.import-midi" => {
            let params: AssetImportParams = decode(params)?;
            let asset_id = asset::application::import_midi_asset(
                state.core.data_root(),
                &params.path.to_string_lossy(),
                params.name.as_deref(),
            )
            .map_err(command)?;
            serialized("assetId", &asset_id)
        }
        "project.export" => {
            let export =
                crate::projects::export(state.core.data_root(), session, crate::storage::now_ms())
                    .map_err(command)?;
            serialized("projectExport", &export)
        }
        "project.import" => {
            let params: ProjectImportParams = decode(params)?;
            mutation(adapter::import_session(&context, &params.path))
        }
        "instrument.clear" => {
            let params: TrackIdParams = decode(params)?;
            mutation(adapter::clear_track_instrument(&context, &params.track_id))
        }
        "effect.remove" => {
            let params: EffectRemoveParams = decode(params)?;
            mutation(adapter::remove_track_effect(
                &context,
                &params.track_id,
                &params.device_id,
            ))
        }
        "effect.reorder" => {
            let params: EffectReorderParams = decode(params)?;
            mutation(adapter::reorder_track_effects(
                &context,
                &params.track_id,
                &params.device_ids,
            ))
        }
        "device.bypass" => {
            let params: DeviceBypassParams = decode(params)?;
            runtime_mutation(adapter::set_track_device_bypassed(
                &context,
                &params.track_id,
                &params.device_id,
                params.bypassed,
            ))
        }
        "undo" => mutation(adapter::undo(&context)),
        "redo" => mutation(adapter::redo(&context)),
        "runtime.projection.get" => serialized("runtimeProjection", &state.runtime.status()),
        "runtime.projection.retry" => {
            adapter::sync_arrangement_runtime(&context).map_err(RouteError::runtime)?;
            serialized("runtimeProjection", &state.runtime.status())
        }
        "render.start" => {
            let params: RenderParams = decode(params)?;
            let (id, status) = state.jobs.start(crate::jobs::JobKind::Render);
            let registry = state.jobs.clone();
            let render_worker = state.render_worker.clone();
            let data_root = state.core.data_root().to_path_buf();
            let session = session.clone();
            let options = params.options.unwrap_or_default();
            std::mem::drop(tauri::async_runtime::spawn_blocking(move || {
                let Some(cancelled) = registry.cancellation_flag(&id) else {
                    return;
                };
                if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                    registry.mark_cancelled(&id);
                    return;
                }
                registry.set_running(&id, "Rendering the canonical timeline.");
                let result = crate::render::render_timeline_with_cancellation(
                    &render_worker,
                    &data_root,
                    &session,
                    crate::storage::now_ms(),
                    options,
                    cancelled.as_ref(),
                );
                if registry.is_cancelled(&id) {
                    registry.mark_cancelled(&id);
                    return;
                }
                match result {
                    Ok(result) => match crate::jobs::serialize_result(&result) {
                        Ok(value) => registry.complete(&id, value, "Timeline render completed."),
                        Err(error) => crate::jobs::fail(&registry, &data_root, &id, error),
                    },
                    Err(error) => crate::jobs::fail(&registry, &data_root, &id, error),
                }
            }));
            let status = crate::jobs::to_background_status(status).map_err(RouteError::command)?;
            serialized("backgroundJob", &status)
        }
        "job.get" => {
            let params: JobIdParams = decode(params)?;
            let status = state
                .jobs
                .status(&params.id)
                .map(crate::jobs::to_background_status)
                .transpose()
                .map_err(RouteError::command)?;
            serialized("backgroundJob", &status)
        }
        "job.cancel" => {
            let params: JobIdParams = decode(params)?;
            let status = state
                .jobs
                .cancel(&params.id)
                .map(crate::jobs::to_background_status)
                .transpose()
                .map_err(RouteError::command)?;
            serialized("backgroundJob", &status)
        }
        "transport.play" => {
            let params: TransportParams = decode(params)?;
            adapter::play_timeline(&context, params.transport_sequence)
                .map_err(RouteError::runtime)?;
            empty()
        }
        "transport.stop" => {
            let params: TransportParams = decode(params)?;
            adapter::stop_timeline(&context, params.transport_sequence)
                .map_err(RouteError::runtime)?;
            empty()
        }
        "transport.go-to-start" => {
            let params: TransportParams = decode(params)?;
            adapter::go_to_start_timeline(&context, params.transport_sequence)
                .map_err(RouteError::runtime)?;
            empty()
        }
        "transport.seek" => {
            let params: SeekParams = decode(params)?;
            adapter::seek_timeline(&context, TimelineTick(params.tick))
                .map_err(RouteError::runtime)?;
            empty()
        }
        "audio.status" => serialized(
            "audioStatus",
            &state
                .core
                .audio()
                .refresh_status()
                .map_err(RouteError::runtime)?,
        ),
        "midi.send" => {
            let params: MidiSendParams = decode(params)?;
            state
                .core
                .audio()
                .send_track_midi(&params.track_id, &params.bytes)
                .map_err(|error| RouteError::runtime(error.to_string()))?;
            empty()
        }
        "midi.panic" => {
            let params: TrackIdParams = decode(params)?;
            state
                .core
                .audio()
                .panic_track_midi(&params.track_id)
                .map_err(|error| RouteError::runtime(error.to_string()))?;
            empty()
        }
        "plugin.catalog.list" => {
            let catalog = crate::plugin_catalog::load(state.core.data_root())
                .map_err(|error| RouteError::command(error.to_string()))?;
            serialized("pluginCatalog", &catalog)
        }
        "instrument.set" => {
            let params: PluginPathParams = decode(params)?;
            runtime_mutation(adapter::set_track_instrument_with_expected_sequence(
                &context,
                &params.track_id,
                &params.plugin_path,
                expected_sequence,
            ))
        }
        "effect.add" => {
            let params: PluginPathParams = decode(params)?;
            runtime_mutation(adapter::add_track_effect_with_expected_sequence(
                &context,
                &params.track_id,
                &params.plugin_path,
                expected_sequence,
            ))
        }
        "device.parameter.set" => {
            let params: DeviceParameterParams = decode(params)?;
            runtime_mutation(adapter::set_track_device_parameter(
                &context,
                &params.track_id,
                &params.device_id,
                params.parameter_index,
                params.value,
            ))
        }
        "missing.list" => serialized(
            "missingDependencies",
            &crate::missing::collect_missing(state.core.data_root(), session),
        ),
        "missing.relink" => {
            let params: MissingRelinkParams = decode(params)?;
            mutation(adapter::relink_missing_dependency(
                &context,
                parse_asset_id(&params.asset_id)?,
                &params.new_path,
            ))
        }
        "missing.disable-plugin" => {
            let params: DeviceIdParams = decode(params)?;
            mutation(adapter::disable_missing_plugin(&context, &params.device_id))
        }
        "missing.replace-plugin" => {
            let params: MissingPluginReplaceParams = decode(params)?;
            runtime_mutation(
                adapter::replace_missing_track_plugin_with_expected_sequence(
                    &context,
                    &params.device_id,
                    &params.new_path,
                    expected_sequence,
                ),
            )
        }
        _ => Err(RouteError::invalid(format!(
            "unknown command: {}",
            request.name
        ))),
    }
}

fn serialized<T: serde::Serialize>(
    result_type: &'static str,
    value: &T,
) -> Result<RouteResult, RouteError> {
    Ok(RouteResult {
        result_type,
        value: serde_json::to_value(value)
            .map_err(|error| RouteError::command(error.to_string()))?,
    })
}

fn empty() -> Result<RouteResult, RouteError> {
    serialized("ok", &())
}

fn mutation(
    result: Result<ArrangementMutationResult, AdapterError>,
) -> Result<RouteResult, RouteError> {
    let result = result.map_err(RouteError::from)?;
    serialized("canonicalState", &result.canonical)
}

fn runtime_mutation(
    result: Result<ArrangementMutationResult, AdapterError>,
) -> Result<RouteResult, RouteError> {
    result
        .map_err(RouteError::from)
        .and_then(|result| serialized("canonicalState", &result.canonical))
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, RouteError> {
    serde_json::from_value(value)
        .map_err(|error| RouteError::invalid(format!("invalid command parameters: {error}")))
}

fn parse_asset_id(value: &str) -> Result<AssetId, RouteError> {
    AssetId::from_normalized(value)
        .map_err(|error| RouteError::invalid(format!("Asset id is invalid: {error}")))
}

fn parse_track_kind(value: &str) -> Result<TrackKind, RouteError> {
    match value {
        "audio" => Ok(TrackKind::Audio),
        "instrument" => Ok(TrackKind::Instrument),
        _ => Err(RouteError::invalid(
            "track kind must be audio or instrument",
        )),
    }
}

fn parse_automation_parameter(value: &str) -> Result<AutomationParameter, RouteError> {
    match value {
        "volume" => Ok(AutomationParameter::Volume),
        "pan" => Ok(AutomationParameter::Pan),
        _ => Err(RouteError::invalid(
            "automation parameter must be volume or pan",
        )),
    }
}

fn command(error: String) -> RouteError {
    RouteError::CommandFailed(error)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackAddParams {
    name: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackUpdateParams {
    track_id: String,
    #[serde(flatten)]
    patch: TrackPatch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackIdParams {
    track_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderParams {
    track_id: String,
    target_index: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioInputParams {
    track_id: String,
    channel_index: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiInputParams {
    track_id: String,
    device_id: Option<String>,
    channel: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipPatchParams<T> {
    clip_id: String,
    patch: T,
}

#[derive(Debug, Deserialize)]
struct MovesParams<T> {
    moves: Vec<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioAddParams {
    asset_id: String,
    name: String,
    start_tick: Option<u64>,
    track_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioTrimParams {
    clip_id: String,
    start_tick: u64,
    source_range: FrameRange,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplitParams {
    clip_id: String,
    split_tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipIdParams {
    clip_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrossfadeParams {
    first_clip_id: String,
    second_clip_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiClipCreateParams {
    track_id: String,
    start_tick: u64,
    duration_ticks: u64,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiAddParams {
    asset_id: String,
    name: String,
    start_tick: Option<u64>,
    track_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiTrimParams {
    clip_id: String,
    start_tick: u64,
    duration_ticks: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteAddParams {
    clip_id: String,
    pitch: u8,
    start_tick: u64,
    duration_ticks: u64,
    velocity: u8,
    channel: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteInsertParams {
    clip_id: String,
    notes: Vec<MidiNoteInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteUpdateParams {
    clip_id: String,
    note_id: String,
    patch: MidiNotePatch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteUpdatesParams {
    clip_id: String,
    updates: Vec<MidiNoteUpdate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteIdParams {
    clip_id: String,
    note_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteIdsParams {
    clip_id: String,
    note_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteQuantizeParams {
    clip_id: String,
    note_ids: Vec<String>,
    grid_ticks: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteTransformParams {
    clip_id: String,
    note_ids: Vec<String>,
    transpose_semitones: i16,
    velocity_offset: i16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteDuplicateParams {
    clip_id: String,
    note_ids: Vec<String>,
    offset_ticks: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipRemoveParams {
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipPasteParams {
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    start_tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerAddParams {
    name: String,
    tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerUpdateParams {
    marker_id: String,
    name: Option<String>,
    tick: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerIdParams {
    marker_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimebaseParams {
    #[serde(flatten)]
    timebase: ProjectTimebase,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RangeParams {
    enabled: bool,
    start_tick: u64,
    end_tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomationParams {
    track_id: String,
    parameter: String,
    points: Vec<AutomationPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomationClearParams {
    track_id: String,
    parameter: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetImportParams {
    path: PathBuf,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectImportParams {
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectRemoveParams {
    track_id: String,
    device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectReorderParams {
    track_id: String,
    device_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceBypassParams {
    track_id: String,
    device_id: String,
    bypassed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransportParams {
    transport_sequence: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeekParams {
    tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiSendParams {
    track_id: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginPathParams {
    track_id: String,
    plugin_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceParameterParams {
    track_id: String,
    device_id: String,
    parameter_index: u32,
    value: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceIdParams {
    device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MissingRelinkParams {
    asset_id: String,
    new_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MissingPluginReplaceParams {
    device_id: String,
    new_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderParams {
    #[serde(default)]
    options: Option<RenderOptions>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobIdParams {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ArrangementProjectionOutcome;
    use riffra_core::{CanonicalState, CreativeSession, HistoryState};

    fn mutation_result() -> ArrangementMutationResult {
        ArrangementMutationResult {
            canonical: CanonicalState {
                session: CreativeSession::new(1),
                sequence: 7,
                history: HistoryState {
                    can_undo: true,
                    can_redo: false,
                },
            },
            projection: ArrangementProjectionOutcome::NotRequired,
        }
    }

    #[test]
    fn canonical_mutations_return_the_complete_canonical_state() {
        let route = mutation(Ok(mutation_result())).unwrap();

        assert_eq!(route.result_type, "canonicalState");
        assert_eq!(route.value["sequence"], 7);
        assert!(route.value["session"].is_object());
        assert!(route.value.get("projection").is_none());
    }

    #[test]
    fn adapter_errors_keep_their_protocol_classification() {
        let conflict = RouteError::from(AdapterError::Conflict {
            expected_sequence: 4,
            current_sequence: 5,
        })
        .protocol_error();
        assert_eq!(conflict.code, ErrorCode::Conflict);
        assert_eq!(conflict.details.unwrap()["expectedSequence"], 4);

        let runtime = runtime_mutation(Err(AdapterError::RuntimeUnavailable(
            "audio sidecar unavailable".into(),
        )))
        .unwrap_err()
        .protocol_error();
        assert_eq!(runtime.code, ErrorCode::RuntimeUnavailable);

        let command = runtime_mutation(Err(AdapterError::CommandFailed(
            "track device is not registered".into(),
        )))
        .unwrap_err()
        .protocol_error();
        assert_eq!(command.code, ErrorCode::CommandFailed);
    }

    #[test]
    fn missing_split_tick_is_an_invalid_request() {
        let error = decode::<SplitParams>(serde_json::json!({ "clipId": "clip:1" }))
            .unwrap_err()
            .protocol_error();

        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }
}
