//! Rack and plugin runtime adapters.

use super::*;

fn repair_previous_arrangement<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    original_error: String,
) -> String {
    if !context.runtime.reset_for_repair() {
        return format!(
            "Arrangement Runtime rejected the instrument or device candidate and could not be reset for the canonical Session: {original_error}"
        );
    }
    match sync_arrangement_runtime(context) {
        Ok(_) => format!(
            "Arrangement Runtime rejected the instrument or device candidate; the canonical Session was restored: {original_error}"
        ),
        Err(restore_error) => format!(
            "Arrangement Runtime rejected the instrument or device candidate and the previous graph could not be restored ({restore_error}): {original_error}"
        ),
    }
}
/// Validates a plugin-bearing candidate against the real Arrangement Runtime
/// before persisting it. A failed candidate never becomes part of the
/// canonical Session, and a persistence failure repairs the previous graph.
pub(super) fn commit_device_arrangement<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    prepared: riffra_core::PreparedSession,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if let Err(error) =
        prepare_arrangement_candidate(context, prepared.session(), prepared.sequence())
    {
        return Err(match error {
            AdapterError::Conflict {
                expected_sequence,
                current_sequence,
            } => AdapterError::Conflict {
                expected_sequence,
                current_sequence,
            },
            AdapterError::ProjectConflict {
                expected_project_id,
                current_project_id,
            } => AdapterError::ProjectConflict {
                expected_project_id,
                current_project_id,
            },
            AdapterError::RuntimeUnavailable(message) => {
                AdapterError::runtime(repair_previous_arrangement(context, message))
            }
            AdapterError::CommandFailed(message) => {
                AdapterError::command(repair_previous_arrangement(context, message))
            }
        });
    }
    let _project_commit_guard = context
        .project_commit
        .as_ref()
        .map(|project_commit| {
            project_commit
                .command_gate
                .lock()
                .map_err(|_| AdapterError::command("Host command gate was poisoned"))
        })
        .transpose()?;
    if let Some(project_commit) = context.project_commit.as_ref() {
        let current_project_id = project_commit
            .project_store
            .active_project_id()
            .map_err(|error| AdapterError::command(error.to_string()))?;
        if current_project_id != project_commit.expected_project_id {
            return Err(AdapterError::ProjectConflict {
                expected_project_id: project_commit.expected_project_id.clone(),
                current_project_id,
            });
        }
    }
    if let Err(error) = commit_core_application(context, |core, store| {
        core.application(store).commit_prepared(prepared)
    }) {
        return Err(match error {
            AdapterError::Conflict {
                expected_sequence,
                current_sequence,
            } => {
                let _ = repair_previous_arrangement(context, error.to_string());
                AdapterError::Conflict {
                    expected_sequence,
                    current_sequence,
                }
            }
            AdapterError::ProjectConflict {
                expected_project_id,
                current_project_id,
            } => AdapterError::ProjectConflict {
                expected_project_id,
                current_project_id,
            },
            AdapterError::RuntimeUnavailable(message) => {
                AdapterError::runtime(repair_previous_arrangement(context, message))
            }
            AdapterError::CommandFailed(message) => {
                AdapterError::command(repair_previous_arrangement(context, message))
            }
        });
    }
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn set_track_audio_input(
    context: &SessionContext<'_>,
    track_id: &str,
    channel_index: Option<u32>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_audio_input(track_id, channel_index)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn set_track_midi_input(
    context: &SessionContext<'_>,
    track_id: &str,
    route: MidiInputRoute,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_midi_input(track_id, route)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn set_track_vst3_instrument<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    track_id: &str,
    path: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    set_track_vst3_instrument_with_expected_sequence(context, track_id, path, None)
}

pub(crate) fn set_track_vst3_instrument_with_expected_sequence<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    track_id: &str,
    path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if context.safe_mode {
        return Err(AdapterError::runtime(
            "Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to connect instruments.",
        ));
    }
    let (name, validated_path) = plugins::validated_plugin(context.data_root, Path::new(path))?;
    let snapshot = current_session(context)?;
    let id = snapshot
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .and_then(|track| track.instrument.as_ref())
        .map(|instrument| instrument.id.clone())
        .unwrap_or_else(|| format!("device:instrument:{track_id}"));
    let instrument =
        riffra_core::TrackInstrument::vst3(id, name, validated_path.to_string_lossy().into_owned())
            .map_err(AdapterError::command)?;
    let prepared = context
        .core
        .application(&context.storage)
        .prepare_track_instrument(track_id, instrument)
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    commit_device_arrangement(context, prepared)
}

/// Assigns a built-in instrument resolved from the immutable Host catalog.
pub fn set_track_builtin_instrument<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    track_id: &str,
    preset_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    set_track_builtin_instrument_with_expected_sequence(context, track_id, preset_id, None)
}

pub(crate) fn set_track_builtin_instrument_with_expected_sequence<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    track_id: &str,
    preset_id: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    let definition = context
        .built_in_instruments
        .resolve(preset_id)
        .map_err(AdapterError::command)?;
    let snapshot = current_session(context)?;
    let id = snapshot
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .and_then(|track| track.instrument.as_ref())
        .map(|instrument| instrument.id.clone())
        .unwrap_or_else(|| format!("device:instrument:{track_id}"));
    let instrument = riffra_core::TrackInstrument::built_in(
        id,
        definition.summary.name.clone(),
        preset_id.to_owned(),
        definition.definition_json.clone(),
    )
    .map_err(AdapterError::command)?;
    let prepared = context
        .core
        .application(&context.storage)
        .prepare_track_instrument(track_id, instrument)
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    if context.safe_mode {
        commit_core_application(context, |core, store| {
            core.application(store).commit_prepared(prepared)
        })?;
        arrangement_mutation_without_projection(context)
    } else {
        commit_device_arrangement(context, prepared)
    }
}

pub fn clear_track_instrument(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store).set_track_instrument(track_id, None)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn add_track_effect(
    context: &SessionContext<'_>,
    track_id: &str,
    path: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    add_track_effect_with_expected_sequence(context, track_id, path, None)
}

pub(crate) fn add_track_effect_with_expected_sequence(
    context: &SessionContext<'_>,
    track_id: &str,
    path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if context.safe_mode {
        return Err(AdapterError::runtime(
            "Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to connect effects.",
        ));
    }
    let (name, validated_path) = plugins::validated_plugin(context.data_root, Path::new(path))?;
    let prepared = context
        .core
        .application(&context.storage)
        .prepare_track_effect(
            track_id,
            name,
            validated_path.to_string_lossy().into_owned(),
        )
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    commit_device_arrangement(context, prepared)
}

pub fn remove_track_effect(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .remove_track_effect(track_id, device_id)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn reorder_track_effects(
    context: &SessionContext<'_>,
    track_id: &str,
    ordered_device_ids: &[String],
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .reorder_track_effects(track_id, ordered_device_ids.to_owned())
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn set_track_device_bypassed(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    bypassed: bool,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    let session = current_session(context)?;
    let device = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .and_then(|track| {
            track
                .instrument
                .as_ref()
                .filter(|device| device.id == device_id)
                .map(|device| device.bypassed)
                .or_else(|| {
                    track
                        .rack
                        .devices
                        .iter()
                        .find(|device| device.id == device_id)
                        .map(|device| device.bypassed)
                })
        })
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))?;
    let previous = device;
    context
        .audio
        .set_track_device_bypassed(track_id, device_id, bypassed)?;
    let result = commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_device_bypassed(track_id, device_id, bypassed)
    });
    match result {
        Ok(_) => crate::session::adapter::arrangement_mutation_without_projection(context),
        Err(error) => {
            let _ = context
                .audio
                .set_track_device_bypassed(track_id, device_id, previous);
            Err(error)
        }
    }
}

pub fn set_track_device_parameter(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    parameter_index: u32,
    value: f32,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if !value.is_finite() {
        return Err("Track Device parameter value must be finite.".into());
    }
    let value = value.clamp(0.0, 1.0);
    let session = current_session(context)?;
    let track = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| AdapterError::command(format!("Track is not registered: {track_id}")))?;
    let index = usize::try_from(parameter_index)
        .map_err(|_| "Track Device parameter index is invalid.".to_string())?;
    let previous = if let Some(instrument) = track
        .instrument
        .as_ref()
        .filter(|instrument| instrument.id == device_id)
    {
        let vst3 = instrument.as_vst3().ok_or_else(|| {
            AdapterError::command("Built-in instruments do not expose parameters.")
        })?;
        vst3.parameter_values.get(index).copied().unwrap_or(0.0)
    } else {
        track
            .rack
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .ok_or_else(|| {
                AdapterError::command(format!("Track Device is not registered: {device_id}"))
            })?
            .parameter_values
            .get(index)
            .copied()
            .unwrap_or(0.0)
    };
    context
        .audio
        .set_track_device_parameter(track_id, device_id, parameter_index, value)?;
    let result = commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_device_parameter(track_id, device_id, index, value)
    });
    match result {
        Ok(_) => crate::session::adapter::arrangement_mutation_without_projection(context),
        Err(error) => {
            let _ = context.audio.set_track_device_parameter(
                track_id,
                device_id,
                parameter_index,
                previous,
            );
            Err(error)
        }
    }
}

pub fn open_track_plugin_editor(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
) -> Result<(), AdapterError> {
    let session = current_session(context)?;
    let registered = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .is_some_and(|track| {
            track.instrument.as_ref().is_some_and(|instrument| {
                instrument.id == device_id && instrument.as_vst3().is_some()
            }) || track
                .rack
                .devices
                .iter()
                .any(|device| device.id == device_id)
        });
    if !registered {
        return Err(format!("Track Device is not registered: {device_id}").into());
    }
    if session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .and_then(|track| track.instrument.as_ref())
        .is_some_and(|instrument| instrument.id == device_id && instrument.as_vst3().is_none())
    {
        return Err("Built-in instruments do not provide a plugin editor.".into());
    }
    drop(session);
    let project_id = context
        .storage
        .project_id()
        .map_err(|error| AdapterError::runtime(error.to_string()))?;
    context
        .audio
        .open_track_plugin_editor(&project_id, track_id, device_id)
        .map_err(|error| AdapterError::runtime(error.to_string()))
}

/// Persists state captured from the native Track Plugin Editor into the
/// canonical Session. The editor already owns the playback instance and the
/// Native Runtime mirrors the state into the live instance, so this operation
/// deliberately does not rebuild or reapply the plugin graph.
pub fn persist_track_plugin_state(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if parameter_values.iter().any(|value| !value.is_finite()) {
        return Err("Track Plugin Editor returned a non-finite parameter value.".into());
    }
    commit_core_application(context, |core, store| {
        core.application(store).persist_track_plugin_state(
            track_id,
            device_id,
            parameter_values,
            state_data,
            bypassed,
        )
    })?;
    crate::session::adapter::arrangement_mutation_without_projection(context)
}

/// Persists one editor-originated parameter without routing it back through
/// Native. The playback instance has already changed and the live instance
/// receives the same value through its block-boundary queue.
pub fn persist_track_plugin_parameter(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    parameter_index: i32,
    value: f32,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if parameter_index < 0 || !value.is_finite() {
        return Err("Track Plugin Editor returned an invalid parameter change.".into());
    }
    let index = usize::try_from(parameter_index)
        .map_err(|_| "Track Plugin Editor returned an invalid parameter index.".to_string())?;
    commit_core_application(context, |core, store| {
        core.application(store)
            .persist_track_plugin_parameter(track_id, device_id, index, value)
    })?;
    crate::session::adapter::arrangement_mutation_without_projection(context)
}

/// Rewrites every canonical Asset reference pointed to by `asset_id` to the
/// user's new file and persists the updated session. The Asset's
/// `content_location` is also updated so future operations resolve to the new
/// path.
pub fn relink_missing_dependency(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    new_path: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    let session = current_session(context)?;
    if !session
        .arrangement
        .audio_clips
        .iter()
        .any(|clip| clip.asset_id == asset_id)
    {
        return Err(format!("Asset is not referenced by the project: {asset_id}").into());
    }
    let new_path = Path::new(new_path);
    if !new_path.is_file() {
        return Err(format!("Replacement asset does not exist: {}", new_path.display()).into());
    }
    let name = new_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("audio");
    let new_asset_id = asset::register(
        context.data_root,
        AssetKind::Audio,
        name,
        &new_path.to_string_lossy(),
        Some(riffra_core::Provenance::imported()),
    )?;
    commit_core_application(context, |core, store| {
        core.application(store)
            .replace_asset_references(&asset_id, new_asset_id)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

/// Marks a missing plugin device as a disabled placeholder so it no longer
/// surfaces as a missing dependency. The session is persisted through the
/// canonical commit.
pub fn disable_missing_plugin(
    context: &SessionContext<'_>,
    device_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store).disable_missing_plugin(device_id)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

/// Replaces an unresolved Track Device in place so its chain position and id
/// remain stable while the plugin binary and plugin state are refreshed.
pub fn replace_missing_track_plugin(
    context: &SessionContext<'_>,
    device_id: &str,
    new_path: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    replace_missing_track_plugin_with_expected_sequence(context, device_id, new_path, None)
}

pub(crate) fn replace_missing_track_plugin_with_expected_sequence(
    context: &SessionContext<'_>,
    device_id: &str,
    new_path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    let path = Path::new(new_path.trim());
    if !path.exists() {
        return Err("Replacement VST3 path does not exist.".into());
    }
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Plugin")
        .to_owned();
    let prepared = context
        .core
        .application(&context.storage)
        .prepare_track_plugin_replacement(device_id, name, path.to_string_lossy().into_owned())
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    commit_device_arrangement(context, prepared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{CreativeSession, DeviceKind, RackDevice, TimelineTick, Track};
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    struct CandidateRuntimeDriver {
        fail_prepare: AtomicBool,
        generation: AtomicU64,
        pending: Mutex<Option<u64>>,
        loaded: Mutex<Vec<u64>>,
        commit_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    }

    impl CandidateRuntimeDriver {
        fn new(fail_prepare: bool) -> Self {
            Self {
                fail_prepare: AtomicBool::new(fail_prepare),
                generation: AtomicU64::new(1),
                pending: Mutex::new(None),
                loaded: Mutex::new(Vec::new()),
                commit_hook: Mutex::new(None),
            }
        }

        fn set_commit_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) {
            *self.commit_hook.lock().unwrap() = Some(hook);
        }
    }

    impl crate::ProjectionDriver for CandidateRuntimeDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: Value,
            _timeout: Duration,
        ) -> Result<(), crate::RuntimeError> {
            if self.fail_prepare.swap(false, Ordering::AcqRel) {
                return Err(crate::RuntimeError::NativeRejected(
                    "Candidate graph was rejected.".into(),
                ));
            }
            *self.pending.lock().unwrap() = Some(snapshot["revision"].as_u64().unwrap());
            Ok(())
        }

        fn commit_timeline_snapshot(&self, _timeout: Duration) -> Result<(), crate::RuntimeError> {
            if let Some(hook) = self.commit_hook.lock().unwrap().take() {
                hook();
            }
            let revision = self.pending.lock().unwrap().take().ok_or_else(|| {
                crate::RuntimeError::NativeRejected(
                    "No prepared timeline snapshot is available.".into(),
                )
            })?;
            self.loaded.lock().unwrap().push(revision);
            Ok(())
        }

        fn discard_timeline_snapshot(&self, _timeout: Duration) -> Result<(), crate::RuntimeError> {
            self.pending.lock().unwrap().take();
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            self.generation.load(Ordering::Relaxed)
        }
    }

    impl crate::TransportDriver for CandidateRuntimeDriver {
        fn play_timeline(&self) -> Result<(), crate::RuntimeError> {
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), crate::RuntimeError> {
            Ok(())
        }
    }

    fn plugin_base_session() -> CreativeSession {
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:plugin".into(), "Plugin Track".into()));
        session
    }

    fn built_in_base_session() -> CreativeSession {
        let mut session = CreativeSession::new(1);
        session.arrangement.tracks.push(Track::instrument(
            "track:instrument".into(),
            "Instrument Track".into(),
        ));
        session
    }

    fn built_in_catalog(root: &Path) -> crate::instrument::BuiltInInstrumentCatalog {
        let directory = root.join("01-clean-sub-bass");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("definition.json"),
            r#"{"metadata":{"name":"Clean Sub Bass","description":"Test preset"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("manifest.json"),
            br#"{"sourceRelease":"vtest","presets":["01-clean-sub-bass"]}"#,
        )
        .unwrap();
        crate::instrument::BuiltInInstrumentCatalog::load(root).unwrap()
    }

    fn candidate_context_with_catalog<'a>(
        root: &'a Path,
        runtime: &'a crate::RuntimeReconciler<CandidateRuntimeDriver>,
        audio: &'a crate::AudioSupervisor,
        core: &'a riffra_core::AppCore<crate::AudioSupervisor>,
        built_in_instruments: &'a crate::instrument::BuiltInInstrumentCatalog,
    ) -> SessionContext<'a, CandidateRuntimeDriver> {
        let storage = riffra_host::SessionStore::new(root, "01900000-0000-7000-8000-000000000001");
        SessionContext {
            core,
            audio,
            runtime,
            storage,
            data_root: root,
            built_in_instruments,
            safe_mode: false,
            events: &crate::NoopHostEventSink,
            project_commit: None,
        }
    }

    fn candidate_context<'a>(
        root: &'a Path,
        runtime: &'a crate::RuntimeReconciler<CandidateRuntimeDriver>,
        audio: &'a crate::AudioSupervisor,
        core: &'a riffra_core::AppCore<crate::AudioSupervisor>,
    ) -> SessionContext<'a, CandidateRuntimeDriver> {
        candidate_context_with_catalog(
            root,
            runtime,
            audio,
            core,
            crate::test_support::empty_built_in_catalog(),
        )
    }

    fn prepared_plugin_candidate<D: RuntimeDriver>(
        context: &SessionContext<'_, D>,
    ) -> riffra_core::PreparedSession {
        context
            .core
            .application(&context.storage)
            .prepare_track_effect(
                "track:plugin",
                "Candidate".into(),
                r"C:\plugins\Candidate.vst3".into(),
            )
            .unwrap()
    }

    #[test]
    fn built_in_candidate_commits_after_runtime_prepare() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-built-in-candidate-committed-{}",
            riffra_host::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = built_in_catalog(&root);
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(
            root.clone(),
            built_in_base_session(),
            audio.clone(),
            false,
            false,
        );
        let context = candidate_context_with_catalog(&root, &runtime, &audio, &core, &catalog);

        // Act
        let result =
            set_track_builtin_instrument(&context, "track:instrument", "01-clean-sub-bass");

        // Assert
        assert!(result.is_ok());
        let snapshot = core.snapshot().unwrap();
        let instrument = snapshot.session.arrangement.tracks[0]
            .instrument
            .as_ref()
            .unwrap();
        assert_eq!(instrument.name, "Clean Sub Bass");
        assert_eq!(instrument.built_in_preset_id(), Some("01-clean-sub-bass"));
        assert_eq!(
            instrument.as_internal().unwrap().0,
            r#"{"metadata":{"name":"Clean Sub Bass","description":"Test preset"}}"#
        );
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), [1]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejected_built_in_candidate_leaves_the_canonical_session_unchanged() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-built-in-candidate-rejected-{}",
            riffra_host::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = built_in_catalog(&root);
        let driver = Arc::new(CandidateRuntimeDriver::new(true));
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(
            root.clone(),
            built_in_base_session(),
            audio.clone(),
            false,
            false,
        );
        let context = candidate_context_with_catalog(&root, &runtime, &audio, &core, &catalog);

        // Act
        let result =
            set_track_builtin_instrument(&context, "track:instrument", "01-clean-sub-bass");

        // Assert
        assert!(result.is_err());
        assert!(
            core.snapshot().unwrap().session.arrangement.tracks[0]
                .instrument
                .is_none()
        );
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), [0]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_mode_commits_a_built_in_assignment_without_runtime_projection() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-built-in-candidate-safe-mode-{}",
            riffra_host::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = built_in_catalog(&root);
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(
            root.clone(),
            built_in_base_session(),
            audio.clone(),
            false,
            false,
        );
        let mut context = candidate_context_with_catalog(&root, &runtime, &audio, &core, &catalog);
        context.safe_mode = true;

        // Act
        let result =
            set_track_builtin_instrument(&context, "track:instrument", "01-clean-sub-bass");

        // Assert
        assert!(result.is_ok());
        assert!(
            core.snapshot().unwrap().session.arrangement.tracks[0]
                .instrument
                .is_some()
        );
        assert!(driver.loaded.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate() {
                return;
            }
            if Instant::now() >= deadline {
                panic!("condition was not met within {timeout:?}");
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn rejected_plugin_candidate_restores_the_canonical_runtime() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-rejected-{}",
            riffra_host::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = plugin_base_session();
        let driver = Arc::new(CandidateRuntimeDriver::new(true));
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio.clone(), false, false);
        let context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_device_arrangement(&context, candidate);

        // Assert
        assert!(result.is_err());
        assert!(
            core.snapshot().unwrap().session.arrangement.tracks[0]
                .rack
                .devices
                .is_empty()
        );
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), [0]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejected_plugin_candidate_is_not_requeued_after_runtime_restart() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-restart-{}",
            riffra_host::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = plugin_base_session();
        let driver = Arc::new(CandidateRuntimeDriver::new(true));
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio.clone(), false, false);
        let context = candidate_context(&root, &runtime, &audio, &core);
        let candidate = prepared_plugin_candidate(&context);
        assert!(commit_device_arrangement(&context, candidate).is_err());
        let loaded_before_restart = driver.loaded.lock().unwrap().len();
        driver.generation.store(2, Ordering::Release);

        // Act
        let requeued = runtime.requeue_after_runtime_restart(2);

        // Assert
        assert!(requeued);
        wait_until(Duration::from_secs(1), || {
            driver.loaded.lock().unwrap().len() > loaded_before_restart
        });
        assert_eq!(driver.loaded.lock().unwrap().last(), Some(&0));
        assert!(!driver.loaded.lock().unwrap().contains(&1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn candidate_sequence_conflict_restores_the_newer_canonical_session() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-conflict-{}",
            riffra_host::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = plugin_base_session();
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = Arc::new(riffra_core::AppCore::new(
            root.clone(),
            session,
            audio.clone(),
            false,
            false,
        ));
        let hook_core = Arc::clone(&core);
        let hook_root = root.clone();
        driver.set_commit_hook(Arc::new(move || {
            let store =
                riffra_host::SessionStore::new(&hook_root, "01900000-0000-7000-8000-000000000001");
            hook_core
                .application(&store)
                .add_marker(TimelineTick(7), "concurrent".into())
                .unwrap();
        }));
        let context = candidate_context(&root, &runtime, &audio, core.as_ref());

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_device_arrangement(&context, candidate);

        // Assert
        assert!(matches!(
            result,
            Err(AdapterError::Conflict {
                expected_sequence: 0,
                current_sequence: 1,
            })
        ));
        let current = core.snapshot().unwrap().session;
        assert_eq!(current.arrangement.revision, 1);
        assert!(current.arrangement.tracks[0].rack.devices.is_empty());
        assert_eq!(driver.loaded.lock().unwrap().last(), Some(&1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stale_plugin_candidate_is_rejected_before_runtime_prepare() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-stale-{}",
            riffra_host::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(
            root.clone(),
            plugin_base_session(),
            audio.clone(),
            false,
            false,
        );
        let context = candidate_context(&root, &runtime, &audio, &core);
        let candidate = prepared_plugin_candidate(&context);
        let store = riffra_host::SessionStore::new(&root, "01900000-0000-7000-8000-000000000001");
        core.application(&store)
            .add_marker(TimelineTick(7), "concurrent".into())
            .unwrap();

        // Act
        let result = commit_device_arrangement(&context, candidate);

        // Assert
        assert!(matches!(
            result,
            Err(AdapterError::Conflict {
                expected_sequence: 0,
                current_sequence: 1,
            })
        ));
        assert!(driver.loaded.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_persistence_failure_restores_the_previous_graph() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-persistence-{}",
            riffra_host::now_ms()
        ));
        std::fs::write(&root, b"not a directory").unwrap();
        let session = plugin_base_session();
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio.clone(), false, false);
        let context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_device_arrangement(&context, candidate);

        // Assert
        assert!(result.is_err());
        assert!(
            core.snapshot().unwrap().session.arrangement.tracks[0]
                .rack
                .devices
                .is_empty()
        );
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), [1, 0]);
        let _ = std::fs::remove_file(&root);
    }

    #[test]
    fn plugin_editor_state_survives_canonical_session_round_trip() {
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-state-round-trip-{}",
            riffra_host::now_ms()
        ));
        let mut session = CreativeSession::new(1);
        let mut track = Track::audio("track:guitar".into(), "Guitar".into());
        track.rack.devices.push(RackDevice {
            id: "device:amp".into(),
            name: "Amp".into(),
            kind: DeviceKind::Plugin,
            path: Some(r"C:\plugins\Amp.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        });
        session.arrangement.tracks.push(track);
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio, false, true);
        let store = riffra_host::SessionStore::new(&root, "01900000-0000-7000-8000-000000000001");
        store.ensure_layout().unwrap();
        let saved = core
            .application(&store)
            .persist_track_plugin_state(
                "track:guitar",
                "device:amp",
                vec![0.25, 0.75],
                Some("opaque-state".into()),
                true,
            )
            .unwrap();
        let restored =
            riffra_core::deserialize_session(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let device = &restored.arrangement.tracks[0].rack.devices[0];
        assert_eq!(device.parameter_values, [0.25, 0.75]);
        assert_eq!(device.state_data.as_deref(), Some("opaque-state"));
        assert!(device.bypassed);
        let _ = std::fs::remove_dir_all(root);
    }
}
