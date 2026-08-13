//! Desktop adapters from Core session and transport decisions to the runtime.

use crate::asset;
use crate::model::{AudioState, AudioStatus};
use crate::native_audio::NativeSamplePad;
use crate::runtime::RuntimeReconciler;
use crate::runtime::ports::RuntimeDriver;
use crate::session::context::SessionContext;
use riffra_core::{
    CreativeSession, DeviceKind, PortError, RuntimeProjection, RuntimeProjectionRequest, SamplePad,
    TimelineTick,
};
use std::path::Path;
use std::time::Duration;

const ARRANGEMENT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) fn audio_command_succeeded(status: &AudioStatus) -> bool {
    status.state != AudioState::Faulted && status.state != AudioState::Offline
}

/// Reports whether persisted Sample Pad mappings were restored or safely
/// disabled because the active device could not accept their buffers.
#[derive(Debug)]
pub(crate) enum SamplePadRestoreOutcome {
    Restored(AudioStatus),
    Disabled {
        status: AudioStatus,
        warning: String,
    },
}

impl SamplePadRestoreOutcome {
    pub(crate) fn into_status(self) -> AudioStatus {
        match self {
            Self::Restored(status) => status,
            Self::Disabled {
                mut status,
                warning,
            } => {
                status.message = append_status_message(&status.message, &warning);
                status
            }
        }
    }
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

struct DesktopRuntimeProjection<'a, D: RuntimeDriver> {
    data_root: &'a Path,
    runtime: &'a RuntimeReconciler<D>,
    wait_for_activation: bool,
}

impl<D: RuntimeDriver> RuntimeProjection for DesktopRuntimeProjection<'_, D> {
    fn project(&self, request: RuntimeProjectionRequest) -> Result<(), PortError> {
        let key = riffra_core::ProjectionKey {
            sequence: request.sequence(),
            session_revision: request.session().arrangement.revision,
        };
        let snapshot = runtime_timeline_snapshot(self.data_root, request.session());
        if self.wait_for_activation {
            self.runtime
                .apply_and_wait(snapshot, key, ARRANGEMENT_RUNTIME_TIMEOUT)
                .map(|_| ())
                .map_err(|error| PortError::Runtime(error.to_string()))
        } else {
            self.runtime.submit_nonblocking(snapshot, key);
            Ok(())
        }
    }
}

/// Enqueues the latest canonical Session for the Audio Runtime without
/// blocking on the reconcile cycle. The Runtime applies the latest projection
/// as soon as an in-flight cycle completes; workflows that need the graph
/// active before returning use [`sync_arrangement_runtime`] instead.
pub(crate) fn sync_arrangement<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<crate::model::RuntimeProjectionStatus, String> {
    let projection = DesktopRuntimeProjection {
        data_root: context.data_root,
        runtime: context.runtime,
        wait_for_activation: false,
    };
    let store = crate::storage::SessionStore::new(context.data_root);
    context
        .core
        .application(&store)
        .project_current(&projection)
        .map_err(|error| error.to_string())?;
    Ok(context.runtime.status())
}

pub fn sync_arrangement_runtime<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<crate::model::RuntimeProjectionStatus, String> {
    let projection = DesktopRuntimeProjection {
        data_root: context.data_root,
        runtime: context.runtime,
        wait_for_activation: true,
    };
    let store = crate::storage::SessionStore::new(context.data_root);
    context
        .core
        .application(&store)
        .project_current(&projection)
        .map_err(|error| error.to_string())?;
    Ok(context.runtime.status())
}

/// Prepares a proposed Arrangement graph before its Session becomes
/// canonical. The expected sequence prevents a candidate built from a stale
/// Session from becoming the active Runtime projection.
pub(crate) fn prepare_arrangement_candidate<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    candidate: &CreativeSession,
    expected_sequence: u64,
) -> Result<crate::model::RuntimeProjectionStatus, String> {
    let current = context.core.snapshot().map_err(|error| error.to_string())?;
    if current.sequence != expected_sequence {
        return Err("Canonical Session changed while the VST candidate was being built.".into());
    }
    context
        .runtime
        .apply_candidate_and_wait(
            runtime_timeline_snapshot(context.data_root, candidate),
            riffra_core::ProjectionKey {
                sequence: expected_sequence.saturating_add(1),
                session_revision: candidate.arrangement.revision,
            },
            ARRANGEMENT_RUNTIME_TIMEOUT,
        )
        .map_err(String::from)
}

/// Rebuilds the persisted Sample Pad mapping after the isolated Audio Runtime
/// has been replaced. This does not mutate canonical state.
pub(crate) fn restore_sample_pads(
    context: &SessionContext<'_>,
) -> Result<SamplePadRestoreOutcome, String> {
    if context.safe_mode {
        return context
            .audio
            .refresh_status()
            .map(SamplePadRestoreOutcome::Restored)
            .map_err(String::from);
    }
    let session = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session;
    let native_pads = match resolve_native_pads(
        context.data_root,
        &session.play_state.sample_instrument.pads,
    ) {
        Ok(native_pads) => native_pads,
        Err(error) => return disable_sample_pads_after_failure(context, error),
    };
    let status = match context.audio.configure_sample_pads(&native_pads) {
        Ok(status) => status,
        Err(error) => return disable_sample_pads_after_failure(context, error.to_string()),
    };
    if !audio_command_succeeded(&status) {
        return disable_sample_pads_after_failure(
            context,
            format!(
                "Runtime rejected Sample Pad restoration: {}",
                status.message
            ),
        );
    }
    Ok(SamplePadRestoreOutcome::Restored(status))
}

fn disable_sample_pads_after_failure(
    context: &SessionContext<'_>,
    reason: String,
) -> Result<SamplePadRestoreOutcome, String> {
    match context.audio.configure_sample_pads(&[]) {
        Ok(status) if audio_command_succeeded(&status) => Ok(SamplePadRestoreOutcome::Disabled {
            status,
            warning: format!(
                "{reason}; Sample Pads were disabled because their buffers could not be restored"
            ),
        }),
        Ok(status) => Err(format!(
            "{reason}; Sample Pads could not be disabled: {}",
            status.message
        )),
        Err(error) => Err(format!(
            "{reason}; Sample Pads could not be disabled: {error}"
        )),
    }
}

fn append_status_message(current: &str, addition: &str) -> String {
    if current.is_empty() {
        addition.into()
    } else {
        format!("{current} {addition}")
    }
}

pub fn play_timeline(context: &SessionContext<'_>, transport_sequence: u64) -> Result<(), String> {
    // Playback is the boundary where an eventually-consistent projection is
    // no longer sufficient. Register the Play intent before waiting for the
    // graph so a concurrent Stop can cancel the pending start.
    let projection = context.core.snapshot().map_err(|error| error.to_string())?;
    context.runtime.apply_and_play(
        transport_sequence,
        runtime_timeline_snapshot(context.data_root, &projection.session),
        riffra_core::ProjectionKey {
            sequence: projection.sequence,
            session_revision: projection.session.arrangement.revision,
        },
        std::time::Duration::from_secs(30),
    )?;
    Ok(())
}

pub fn stop_timeline(context: &SessionContext<'_>, transport_sequence: u64) -> Result<(), String> {
    context
        .runtime
        .stop(transport_sequence)
        .map(|_| ())
        .map_err(String::from)
}

pub fn go_to_start_timeline(
    context: &SessionContext<'_>,
    transport_sequence: u64,
) -> Result<(), String> {
    context
        .runtime
        .stop_and_seek_to_start(transport_sequence, || {
            context
                .audio
                .seek_timeline(0)
                .map_err(crate::runtime::error::RuntimeError::from)
        })
        .map_err(String::from)
}

pub fn seek_timeline(context: &SessionContext<'_>, tick: TimelineTick) -> Result<(), String> {
    context.audio.seek_timeline(tick.0).map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RecordingStatus;
    use riffra_core::{RackDevice, Track};

    #[test]
    fn disabled_sample_pad_outcome_keeps_the_warning_in_the_status() {
        // Arrange
        let outcome = SamplePadRestoreOutcome::Disabled {
            status: AudioStatus {
                state: AudioState::Muted,
                driver: None,
                input_device: None,
                input_channel: None,
                input_channels: Vec::new(),
                output_device: None,
                output_channels: Vec::new(),
                sample_rate: None,
                buffer_size: None,
                round_trip_ms: None,
                timeline_tick: None,
                recording: RecordingStatus::default(),
                midi_inputs: Vec::new(),
                midi_outputs: Vec::new(),
                midi_input_active: false,
                midi_messages: 0,
                last_midi_note: None,
                midi_pad_mappings: 0,
                midi_pad_triggers: 0,
                input_peak: 0.0,
                output_peak: 0.0,
                invalid_samples: 0,
                feedback_suspected: false,
                message: "Audio remains muted while the graph is rebuilt.".into(),
            },
            warning: "Sample Pads were disabled because their buffers could not be restored".into(),
        };

        // Act
        let status = outcome.into_status();

        // Assert
        assert_eq!(status.state, AudioState::Muted);
        assert!(status.message.contains("Audio remains muted"));
        assert!(status.message.contains("Sample Pads were disabled"));
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
