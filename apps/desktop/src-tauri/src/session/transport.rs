//! Session-to-Runtime Transport application operations.

use crate::asset;
use crate::model::{AudioState, AudioStatus};
use crate::native_audio::NativeSamplePad;
use crate::rack::DeviceKind;
use crate::runtime::ports::RuntimeDriver;
use crate::session::context::{SessionContext, lock_error};
use crate::session::{CreativeSession, SamplePad, TimelineTick, Workspace};
use std::path::Path;

pub(crate) fn audio_command_succeeded(status: &AudioStatus) -> bool {
    status.state != AudioState::Faulted && status.state != AudioState::Offline
}

/// Resolves the session's pad set into the runtime's native pad shape, failing
/// on any invalid slice or unresolved asset. Shared by the create workflow and
/// the direct `configure_sample_pads` command.
pub fn resolve_native_pads(
    data_root: &Path,
    pads: &[SamplePad],
) -> Result<Vec<NativeSamplePad>, String> {
    if pads.len() > 128 {
        return Err("A sample instrument cannot contain more than 128 pads.".into());
    }
    let mut native_pads = Vec::with_capacity(pads.len());
    for pad in pads {
        if pad.end_ms <= pad.start_ms {
            return Err(format!("Sample pad '{}' has an invalid slice.", pad.name));
        }
        let content_location = asset::resolve_content_location(data_root, &pad.asset_id)
            .ok_or_else(|| format!("Sample pad '{}' references an unresolved asset.", pad.name))?;
        native_pads.push(NativeSamplePad {
            id: pad.id.clone(),
            name: pad.name.clone(),
            asset_path: content_location,
            start_ms: pad.start_ms,
            end_ms: pad.end_ms,
            midi_key: pad.midi_key,
            gain_db: pad.gain_db,
            loop_enabled: pad.loop_enabled,
        });
    }
    Ok(native_pads)
}

pub(crate) fn runtime_timeline_snapshot(
    data_root: &Path,
    session: &CreativeSession,
) -> serde_json::Value {
    let arrangement = &session.arrangement;
    let mut unavailable_clip_ids = Vec::new();
    let mut missing_device_ids = Vec::new();
    let tracks = arrangement
        .tracks
        .iter()
        .map(|track| {
            let mut runtime_instrument = track.instrument.clone();
            let mut runtime_rack = track.rack.clone();
            for device in runtime_instrument
                .iter_mut()
                .chain(runtime_rack.devices.iter_mut())
                .filter(|device| device.kind == DeviceKind::Plugin)
            {
                if !device.disabled_placeholder
                    && device
                        .path
                        .as_deref()
                        .is_none_or(|path| !Path::new(path).exists())
                {
                    missing_device_ids.push(device.id.clone());
                    // Keep the canonical unresolved Device intact, but project a
                    // bypassed placeholder into the Runtime Graph so one missing
                    // plugin never prevents the rest of the Arrangement playing.
                    device.disabled_placeholder = true;
                }
            }
            let audio_clips = arrangement
                .audio_clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
                .filter_map(|clip| {
                    let Some(path) = asset::resolve_content_location(data_root, &clip.asset_id)
                    else {
                        unavailable_clip_ids.push(clip.id.clone());
                        return None;
                    };
                    Some(serde_json::json!({
                        "clipId": clip.id,
                        "path": path,
                        "sourceSampleRate": clip.source_sample_rate,
                        "sourceStartFrame": clip.source_range.start,
                        "sourceEndFrame": clip.source_range.end,
                        "durationFrames": clip.timeline_duration.frames,
                        "durationSampleRate": clip.timeline_duration.sample_rate,
                        "startTick": clip.start_tick.0,
                        "fadeInFrames": clip.fade_in.frames,
                        "fadeOutFrames": clip.fade_out.frames,
                        "gainDb": clip.gain_db,
                        "pan": clip.pan,
                        "loopEnabled": clip.loop_enabled,
                        "muted": clip.muted,
                    }))
                })
                .collect::<Vec<_>>();
            let midi_clips = arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
                .collect::<Vec<_>>();
            let automation = arrangement
                .automation_lanes
                .iter()
                .filter(|lane| lane.track_id == track.id)
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": track.id,
                "name": track.name,
                "kind": track.kind,
                "gainDb": track.gain_db,
                "pan": track.pan,
                "muted": track.muted,
                "solo": track.solo,
                "armed": track.armed,
                "monitoring": track.monitoring,
                "audioInput": track.audio_input,
                "midiInput": track.midi_input,
                "instrument": runtime_instrument,
                "rack": runtime_rack,
                "audioClips": audio_clips,
                "midiClips": midi_clips,
                "automation": automation,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "revision": arrangement.revision,
        "timebase": arrangement.timebase,
        "loopRange": arrangement.loop_range,
        "punchRange": arrangement.punch_range,
        "metronomeEnabled": session.settings.metronome_enabled,
        "tracks": tracks,
        "unavailableClipIds": unavailable_clip_ids,
        "missingDeviceIds": missing_device_ids,
    })
}

pub(crate) fn runtime_snapshot_for_recording(
    data_root: &Path,
    session: &CreativeSession,
) -> serde_json::Value {
    runtime_timeline_snapshot(data_root, session)
}

pub(crate) fn submit_canonical_projection<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    projection: crate::session::actor::CanonicalProjection,
) -> Result<crate::model::RuntimeProjectionStatus, String> {
    Ok(context.runtime.submit(
        runtime_timeline_snapshot(context.data_root, &projection.session),
        crate::runtime::model::ProjectionKey {
            sequence: projection.sequence,
            session_revision: projection.session.arrangement.revision,
        },
    ))
}

pub(crate) fn submit_canonical_projection_nonblocking<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    projection: crate::session::actor::CanonicalProjection,
) -> Result<crate::model::RuntimeProjectionStatus, String> {
    Ok(context.runtime.submit_nonblocking(
        runtime_timeline_snapshot(context.data_root, &projection.session),
        crate::runtime::model::ProjectionKey {
            sequence: projection.sequence,
            session_revision: projection.session.arrangement.revision,
        },
    ))
}

/// Enqueues the canonical Session captured under the Actor for the Audio
/// Runtime without blocking on the reconcile cycle. Session commands already
/// own the Actor for their whole synchronous operation, so this path must use
/// the non-reentrant, guard-aware capture method. The Runtime applies the
/// latest projection as soon as an in-flight cycle completes; workflows that
/// need the graph active before returning (playback, recording) use
/// [`sync_arrangement_runtime`] instead.
pub(crate) fn sync_arrangement<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<crate::model::RuntimeProjectionStatus, String> {
    let projection = context
        .session_actor
        .capture_projection_while_held(context.session)?;
    submit_canonical_projection_nonblocking(context, projection)
}

pub fn sync_arrangement_runtime(
    context: &SessionContext<'_>,
) -> Result<crate::model::RuntimeProjectionStatus, String> {
    let projection = context.session_actor.capture_projection(context.session)?;
    submit_canonical_projection(context, projection)
}

/// Rebuilds the persisted Sample Pad mapping after the isolated Audio Runtime
/// has been replaced. This does not mutate canonical state.
pub fn restore_sample_pads(context: &SessionContext<'_>) -> Result<AudioStatus, String> {
    if context.safe_mode {
        return context.audio.refresh_status();
    }
    let session = context.session.lock().map_err(lock_error)?.clone();
    let native_pads = resolve_native_pads(
        context.data_root,
        &session.play_state.sample_instrument.pads,
    )?;
    let status = context.audio.configure_sample_pads(&native_pads)?;
    if !audio_command_succeeded(&status) {
        return Err(format!(
            "Runtime rejected Sample Pad restoration: {}",
            status.message
        ));
    }
    Ok(status)
}

pub fn play_timeline(context: &SessionContext<'_>, transport_sequence: u64) -> Result<(), String> {
    // Playback is the boundary where an eventually-consistent projection is
    // no longer sufficient. Register the Play intent before waiting for the
    // graph so a concurrent Stop can cancel the pending start.
    let projection = context.session_actor.capture_projection(context.session)?;
    let played = context.runtime.apply_and_play_if(
        transport_sequence,
        runtime_timeline_snapshot(context.data_root, &projection.session),
        crate::runtime::model::ProjectionKey {
            sequence: projection.sequence,
            session_revision: projection.session.arrangement.revision,
        },
        std::time::Duration::from_secs(30),
        || {
            context
                .session
                .lock()
                .map(|session| session.workspace == Workspace::Arrange)
                .unwrap_or(false)
        },
    )?;
    if !played {
        return Err("Arrange playback was cancelled because the workspace changed.".into());
    }
    Ok(())
}

pub fn stop_timeline(context: &SessionContext<'_>, transport_sequence: u64) -> Result<(), String> {
    context.runtime.stop(transport_sequence).map(|_| ())
}

pub fn go_to_start_timeline(
    context: &SessionContext<'_>,
    transport_sequence: u64,
) -> Result<(), String> {
    context
        .runtime
        .stop_and_seek_to_start(transport_sequence, || context.audio.seek_timeline(0))
}

pub fn seek_timeline(context: &SessionContext<'_>, tick: TimelineTick) -> Result<(), String> {
    context.audio.seek_timeline(tick.0)
}

/// Returns the active workspace snapshot and updates the desired audio mode.
///
/// Workspace navigation is UI state, not production content. Persisting it as
/// a full Session commit made every tab click copy the current recovery
/// generation, serialize the whole arrangement, fsync, and wait behind any
/// unrelated edit. The frontend applies this snapshot optimistically; the
/// in-memory Session also retains the latest workspace so the next real edit
/// persists it, but navigation itself creates no recovery generation. A
/// stalled audio process is deliberately unable to block this operation
/// because mode delivery is best-effort and nonblocking.
pub fn switch_workspace(
    context: &SessionContext<'_>,
    workspace: Workspace,
    transport_sequence: u64,
) -> Result<CreativeSession, String> {
    let session = {
        let mut current = context.session.lock().map_err(lock_error)?;
        current.workspace = workspace;
        current.clone()
    };
    if workspace != Workspace::Arrange
        && let Err(error) = context.runtime.stop_nonblocking(transport_sequence)
    {
        tracing::warn!(
            error = ?error,
            workspace = ?workspace,
            "Workspace snapshot returned, but stale Arrange transport could not be stopped."
        );
    }
    // This is deliberately not a SessionActor commit: workspace is view state
    // and has no effect on the arrangement projection sequence. Keeping the
    // latest value in memory lets the next real production edit persist it,
    // while avoiding a full JSON/recovery-generation/fsync cycle per click.
    let mode = workspace_processing_mode(workspace);
    if let Err(error) = context.audio.set_processing_mode_nonblocking(mode) {
        tracing::warn!(
            error = ?error,
            workspace = ?workspace,
            "Workspace snapshot returned, but the audio processing mode could not be sent; recovery will retry the desired mode."
        );
    }
    Ok(session)
}

fn workspace_processing_mode(workspace: Workspace) -> &'static str {
    match workspace {
        Workspace::Play => "play",
        Workspace::Arrange => "arrange",
        Workspace::Home | Workspace::Design => "passive",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rack::RackDevice;
    use crate::session::Track;
    use crate::session::actor::SessionActor;
    use std::sync::{Arc, Mutex};

    #[test]
    fn workspace_navigation_updates_memory_without_creating_a_session_save() {
        let root = std::env::temp_dir().join(format!(
            "riffra-workspace-navigation-{}",
            crate::storage::now_ms()
        ));
        let session = Mutex::new(CreativeSession::new(1));
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let runtime_driver = Arc::new(audio.clone());
        let runtime =
            Arc::new(crate::runtime::RuntimeReconciler::new(runtime_driver, None).unwrap());
        let actor = SessionActor::default();
        let context = SessionContext {
            audio: &audio,
            runtime: runtime.as_ref(),
            session_actor: &actor,
            data_root: &root,
            session: &session,
            safe_mode: false,
        };

        let next = switch_workspace(&context, Workspace::Arrange, 1).unwrap();

        assert_eq!(next.workspace, Workspace::Arrange);
        assert_eq!(session.lock().unwrap().workspace, Workspace::Arrange);
        assert!(!root.join("scratch").join("current.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_track_plugin_is_projected_as_a_runtime_placeholder() {
        let mut session = CreativeSession::new(1);
        let mut track = Track::instrument("track:synth".into(), "Synth".into());
        track.instrument = Some(RackDevice {
            id: "device:missing".into(),
            name: "Missing Synth".into(),
            kind: DeviceKind::Plugin,
            path: Some(r"C:\missing\Synth.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        });
        session.arrangement.tracks.push(track);

        let snapshot = runtime_timeline_snapshot(Path::new("."), &session);

        assert_eq!(
            snapshot["missingDeviceIds"],
            serde_json::json!(["device:missing"])
        );
        assert_eq!(
            snapshot["tracks"][0]["instrument"]["disabledPlaceholder"],
            true
        );
        assert!(
            !session.arrangement.tracks[0]
                .instrument
                .as_ref()
                .unwrap()
                .disabled_placeholder
        );
    }
}
