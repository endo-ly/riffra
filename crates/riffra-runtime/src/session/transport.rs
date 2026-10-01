//! Host adapters from Core session and transport decisions to the runtime.

use crate::RuntimeDriver;
use crate::execution::project_session;
use crate::session::context::SessionContext;
use crate::session::error::AdapterError;
use riffra_core::CreativeSession;
use std::sync::Arc;
use std::time::Duration;

const ARRANGEMENT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(60);

pub fn sync_arrangement_runtime<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<crate::RuntimeProjectionStatus, String> {
    let canonical = context.project.read().canonical.clone();
    context
        .runtime
        .apply_and_wait(
            Arc::new(project_session(
                context.data_root,
                context.built_in_instruments,
                &canonical.project_id,
                &canonical.session,
            )),
            riffra_core::ProjectionKey {
                sequence: canonical.sequence,
                session_revision: canonical.session.arrangement.revision,
            },
            ARRANGEMENT_RUNTIME_TIMEOUT,
        )
        .map_err(|error| error.to_string())
}

/// Prepares a proposed Arrangement graph before its Session becomes
/// canonical. The expected sequence prevents a candidate built from a stale
/// Session from becoming the active Runtime projection.
pub fn prepare_arrangement_candidate<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    candidate: &CreativeSession,
    expected_sequence: u64,
) -> Result<crate::RuntimeProjectionStatus, AdapterError> {
    let current = context.project.read().canonical.clone();
    if current.project_id != context.snapshot.project_id {
        return Err(AdapterError::ProjectConflict {
            expected_project_id: context.snapshot.project_id.clone(),
            current_project_id: current.project_id,
        });
    }
    if current.sequence != expected_sequence {
        return Err(AdapterError::Conflict {
            expected_sequence,
            current_sequence: current.sequence,
        });
    }
    context
        .runtime
        .apply_candidate_and_wait(
            Arc::new(project_session(
                context.data_root,
                context.built_in_instruments,
                &context.snapshot.project_id,
                candidate,
            )),
            riffra_core::ProjectionKey {
                sequence: expected_sequence.saturating_add(1),
                session_revision: candidate.arrangement.revision,
            },
            ARRANGEMENT_RUNTIME_TIMEOUT,
        )
        .map_err(|error| AdapterError::runtime(error.to_string()))
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
            br#"{"sourceRelease":"vtest","presets":[]}"#,
        )
        .unwrap();
        let catalog = crate::instrument::BuiltInInstrumentCatalog::load(&resource_root).unwrap();
        let projection = project_session(&resource_root, &catalog, "project", &session);

        assert_eq!(
            projection.diagnostics.missing_device_ids,
            ["device:missing"]
        );
        assert!(projection.snapshot.graph.tracks[0].instrument.is_none());
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
