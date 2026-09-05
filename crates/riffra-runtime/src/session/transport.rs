//! Host adapters from Core session and transport decisions to the runtime.

use crate::session::context::SessionContext;
use crate::session::error::AdapterError;
use crate::{RuntimeDriver, RuntimeReconciler};
use riffra_core::{
    CreativeSession, PortError, RuntimeProjection, RuntimeProjectionRequest, TimelineTick,
};
use std::path::Path;
use std::time::Duration;

pub use crate::runtime_snapshot::runtime_timeline_snapshot;

const ARRANGEMENT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(60);

struct RuntimeProjectionAdapter<'a, D: RuntimeDriver> {
    data_root: &'a Path,
    built_in_instruments: &'a crate::instrument::BuiltInInstrumentCatalog,
    runtime: &'a RuntimeReconciler<D>,
}

impl<D: RuntimeDriver> RuntimeProjection for RuntimeProjectionAdapter<'_, D> {
    fn project(&self, request: RuntimeProjectionRequest) -> Result<(), PortError> {
        let key = riffra_core::ProjectionKey {
            sequence: request.sequence(),
            session_revision: request.session().arrangement.revision,
        };
        let snapshot =
            runtime_timeline_snapshot(self.data_root, self.built_in_instruments, request.session());
        self.runtime
            .apply_and_wait(snapshot, key, ARRANGEMENT_RUNTIME_TIMEOUT)
            .map(|_| ())
            .map_err(|error| PortError::Runtime(error.to_string()))
    }
}

pub fn sync_arrangement_runtime<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<crate::RuntimeProjectionStatus, String> {
    let projection = RuntimeProjectionAdapter {
        data_root: context.data_root,
        built_in_instruments: context.built_in_instruments,
        runtime: context.runtime,
    };
    context
        .core
        .application(&context.storage)
        .project_current(&projection)
        .map_err(|error| error.to_string())?;
    Ok(context.runtime.status())
}

/// Prepares a proposed Arrangement graph before its Session becomes
/// canonical. The expected sequence prevents a candidate built from a stale
/// Session from becoming the active Runtime projection.
pub fn prepare_arrangement_candidate<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    candidate: &CreativeSession,
    expected_sequence: u64,
) -> Result<crate::RuntimeProjectionStatus, AdapterError> {
    let current = context.core.snapshot()?;
    if current.sequence != expected_sequence {
        return Err(AdapterError::Conflict {
            expected_sequence,
            current_sequence: current.sequence,
        });
    }
    context
        .runtime
        .apply_candidate_and_wait(
            runtime_timeline_snapshot(context.data_root, context.built_in_instruments, candidate),
            riffra_core::ProjectionKey {
                sequence: expected_sequence.saturating_add(1),
                session_revision: candidate.arrangement.revision,
            },
            ARRANGEMENT_RUNTIME_TIMEOUT,
        )
        .map_err(|error| AdapterError::runtime(error.to_string()))
}

pub fn play_timeline(context: &SessionContext<'_>, transport_sequence: u64) -> Result<(), String> {
    // Playback is the boundary where an eventually-consistent projection is
    // no longer sufficient. Register the Play intent before waiting for the
    // graph so a concurrent Stop can cancel the pending start.
    let projection = context.core.snapshot().map_err(|error| error.to_string())?;
    context.runtime.apply_and_play(
        transport_sequence,
        runtime_timeline_snapshot(
            context.data_root,
            context.built_in_instruments,
            &projection.session,
        ),
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
                .map_err(crate::RuntimeError::from)
        })
        .map_err(String::from)
}

pub fn seek_timeline(context: &SessionContext<'_>, tick: TimelineTick) -> Result<(), String> {
    context.audio.seek_timeline(tick.0).map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{Track, TrackInstrument};

    #[test]
    fn missing_track_plugin_is_projected_as_a_runtime_placeholder() {
        let mut session = CreativeSession::new(1);
        let mut track = Track::instrument("track:synth".into(), "Synth".into());
        track.instrument = Some(
            TrackInstrument::vst3(
                "device:missing".into(),
                "Missing Synth".into(),
                r"C:\missing\Synth.vst3".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks.push(track);

        let resource_root =
            std::env::temp_dir().join(format!("riffra-transport-builtins-{}", std::process::id()));
        std::fs::create_dir_all(&resource_root).unwrap();
        std::fs::write(
            resource_root.join("manifest.json"),
            br#"{"sourceRevision":"test-revision","presets":[]}"#,
        )
        .unwrap();
        let catalog = crate::instrument::BuiltInInstrumentCatalog::load(&resource_root).unwrap();
        let snapshot = runtime_timeline_snapshot(&resource_root, &catalog, &session);

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
                .as_vst3()
                .unwrap()
                .disabled_placeholder
        );
        let _ = std::fs::remove_dir_all(resource_root);
    }
}
