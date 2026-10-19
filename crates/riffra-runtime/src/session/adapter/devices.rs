//! Track device and plugin runtime adapters.

use super::*;

fn plugin_device_role(
    session: &riffra_core::CreativeSession,
    device_id: &str,
) -> Option<crate::api::output::PluginRole> {
    session
        .arrangement
        .tracks
        .iter()
        .find_map(|track| {
            if track
                .instrument
                .as_ref()
                .is_some_and(|instrument| instrument.id == device_id)
            {
                return Some(crate::api::output::PluginRole::Instrument);
            }

            track
                .effects
                .iter()
                .any(|device| device.id == device_id)
                .then_some(crate::api::output::PluginRole::Effect)
        })
        .or_else(|| {
            session
                .arrangement
                .mixer
                .buses
                .iter()
                .flat_map(|bus| &bus.effects)
                .chain(&session.arrangement.mixer.master_effects)
                .any(|effect| effect.id == device_id)
                .then_some(crate::api::output::PluginRole::Effect)
        })
}

fn repair_previous_arrangement<D: RuntimeDriver>(
    context: &mut SessionContext<'_, D>,
    original_error: String,
) -> String {
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
    context: &mut SessionContext<'_, D>,
    prepared: riffra_core::PreparedSession,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    commit_device_arrangement_with_created_ids(context, prepared, Default::default())
}

pub(super) fn commit_device_arrangement_with_created_ids<D: RuntimeDriver>(
    context: &mut SessionContext<'_, D>,
    prepared: riffra_core::PreparedSession,
    created_entity_ids: std::collections::BTreeMap<String, Vec<String>>,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
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
            AdapterError::Native(error) => AdapterError::Native({
                let descriptor = error.descriptor();
                crate::NativeAudioError::structured(
                    descriptor.kind,
                    repair_previous_arrangement(context, descriptor.message),
                    descriptor.operation.as_deref().unwrap_or_default(),
                    descriptor.details,
                )
            }),
            AdapterError::CommandFailed(message) => {
                AdapterError::command(repair_previous_arrangement(context, message))
            }
        });
    }
    let committed = match context.commit(|mut app| app.commit_prepared(prepared)) {
        Ok(committed) => committed,
        Err(error) => {
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
                AdapterError::Native(error) => AdapterError::Native({
                    let descriptor = error.descriptor();
                    crate::NativeAudioError::structured(
                        descriptor.kind,
                        repair_previous_arrangement(context, descriptor.message),
                        descriptor.operation.as_deref().unwrap_or_default(),
                        descriptor.details,
                    )
                }),
                AdapterError::CommandFailed(message) => {
                    AdapterError::command(repair_previous_arrangement(context, message))
                }
            });
        }
    };
    let mut mutation = committed.0;
    mutation.created_entity_ids = created_entity_ids;
    Ok(mutation)
}

pub fn set_track_audio_input(
    context: &mut SessionContext<'_>,
    track_id: &str,
    channel_index: Option<u32>,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    let committed = context.commit(|mut app| app.set_track_audio_input(track_id, channel_index))?;
    Ok(committed.0)
}

pub fn set_track_midi_input(
    context: &mut SessionContext<'_>,
    track_id: &str,
    route: MidiInputRoute,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    let committed = context.commit(|mut app| app.set_track_midi_input(track_id, route))?;
    Ok(committed.0)
}

pub(crate) fn set_track_vst3_instrument_with_expected_sequence<D: RuntimeDriver>(
    context: &mut SessionContext<'_, D>,
    track_id: &str,
    path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    if context.safe_mode {
        return Err(AdapterError::runtime(
            "Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to connect instruments.",
        ));
    }
    let (name, validated_path) = plugins::validated_plugin(
        context.data_root,
        Path::new(path),
        crate::api::output::PluginRole::Instrument,
    )?;
    let snapshot = current_session(context)?;
    let existing_id = snapshot
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .and_then(|track| track.instrument.as_ref())
        .map(|instrument| instrument.id.clone());
    let creates_device = existing_id.is_none();
    let id = existing_id.unwrap_or_else(|| format!("device:instrument:{track_id}"));
    let instrument = riffra_core::TrackInstrument::vst3(
        id.clone(),
        name,
        validated_path.to_string_lossy().into_owned(),
    )
    .map_err(AdapterError::command)?;
    let prepared = context
        .prepare_core()
        .application(&context.storage)
        .prepare_track_instrument(track_id, instrument)
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    let created_entity_ids = if creates_device {
        std::collections::BTreeMap::from([("devices".into(), vec![id])])
    } else {
        Default::default()
    };
    commit_device_arrangement_with_created_ids(context, prepared, created_entity_ids)
}

pub fn clear_track_instrument(
    context: &mut SessionContext<'_>,
    track_id: &str,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    let committed = context.commit(|mut app| app.set_track_instrument(track_id, None))?;
    Ok(committed.0)
}

pub(crate) fn add_effect_with_expected_sequence(
    context: &mut SessionContext<'_>,
    owner: &riffra_core::EffectOwner,
    path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    if context.safe_mode {
        return Err(AdapterError::runtime(
            "Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to connect effects.",
        ));
    }
    let (name, validated_path) = plugins::validated_plugin(
        context.data_root,
        Path::new(path),
        crate::api::output::PluginRole::Effect,
    )?;
    let (prepared, device_id) = context
        .prepare_core()
        .application(&context.storage)
        .prepare_effect_with_created_id(owner, name, validated_path.to_string_lossy().into_owned())
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    commit_device_arrangement_with_created_ids(
        context,
        prepared,
        std::collections::BTreeMap::from([(String::from("devices"), vec![device_id])]),
    )
}

pub fn remove_effect(
    context: &mut SessionContext<'_>,
    owner: &riffra_core::EffectOwner,
    device_id: &str,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    let committed = context.commit(|mut app| app.remove_effect(owner, device_id))?;
    Ok(committed.0)
}

pub fn reorder_effects(
    context: &mut SessionContext<'_>,
    owner: &riffra_core::EffectOwner,
    ordered_device_ids: &[String],
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    let committed =
        context.commit(|mut app| app.reorder_effects(owner, ordered_device_ids.to_owned()))?;
    Ok(committed.0)
}

pub fn set_device_bypassed(
    context: &mut SessionContext<'_>,
    owner: &riffra_core::EffectOwner,
    device_id: &str,
    bypassed: bool,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    let session = current_session(context)?;
    let previous = session
        .arrangement
        .device(owner, device_id)
        .map_err(|error| AdapterError::command(error.to_string()))?
        .bypassed();
    context
        .audio
        .set_device_bypassed(owner, device_id, bypassed)?;
    let committed = context.commit(|mut app| app.set_device_bypassed(owner, device_id, bypassed));
    match committed {
        Ok((mutation, _)) => Ok(mutation),
        Err(error) => {
            let _ = context
                .audio
                .set_device_bypassed(owner, device_id, previous);
            Err(error)
        }
    }
}

/// Waits for the runtime projection matching the context snapshot before
/// touching a device on the native timeline, so the device read and write
/// cannot observe a graph that is being replaced. The wait runs without
/// holding a canonical lock, and states outside the projection lifecycle
/// (Safe Mode, a runtime that never projected this key) fall straight through.
fn wait_for_device_projection<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<(), AdapterError> {
    let key = riffra_core::ProjectionKey {
        sequence: context.snapshot.sequence,
        session_revision: context.snapshot.session.arrangement.revision,
    };
    match context
        .runtime
        .wait_for_native_projection(key)
        .map_err(|error| AdapterError::CommandFailed(error.to_string()))?
    {
        crate::runtime::ProjectionWait::Ready | crate::runtime::ProjectionWait::Missing => Ok(()),
    }
}

pub fn set_device_parameter(
    context: &mut SessionContext<'_>,
    owner: &riffra_core::EffectOwner,
    device_id: &str,
    parameter_index: u32,
    value: Option<f32>,
    display: Option<String>,
) -> Result<crate::api::output::DeviceParameterMutationResult, AdapterError> {
    if value.is_some() == display.is_some() || value.is_some_and(|value| !value.is_finite()) {
        return Err("select exactly one finite value or display text".into());
    }
    let value = value.map(|value| value.clamp(0.0, 1.0));
    {
        let session = current_session(context)?;
        let device = session
            .arrangement
            .device(owner, device_id)
            .map_err(|error| AdapterError::command(error.to_string()))?;
        if device.plugin().is_none() {
            return Err(AdapterError::command(
                "built-in instruments do not expose parameters",
            ));
        }
    }
    // The device write only commits after the native graph accepted the
    // parameter, so verify-then-commit order keeps canonical and native alive
    // without a post-commit reconciliation against an already mutated state.
    // The projection wait can block for a long time, so it runs without
    // holding the canonical write lock; holding it across the wait would
    // stall every other canonical writer. Afterwards the lock is taken back
    // and kept through the commit, so the native change, its recorded state,
    // and the canonical commit cannot interleave with another device writer.
    context.release_writer();
    wait_for_device_projection(context)?;
    context.reacquire_writer()?;
    let previous = context.audio.get_plugin_state(owner, device_id)?;
    let before = context.audio.list_device_parameters(owner, device_id)?;
    if usize::try_from(parameter_index).is_ok_and(|index| index >= previous.parameter_values.len())
    {
        return Err(AdapterError::command("device parameter index is invalid"));
    }
    let committed: Result<_, AdapterError> = (|| {
        context
            .audio
            .set_device_parameter(owner, device_id, parameter_index, value, display)?;
        let state = context.audio.get_plugin_state(owner, device_id)?;
        context.commit(|mut app| {
            app.persist_plugin_state(
                owner,
                device_id,
                state.parameter_values,
                state.state_data,
                state.bypassed,
            )
        })
    })();
    match committed {
        Ok((mutation, _)) => {
            let after = context.audio.list_device_parameters(owner, device_id)?;
            let (parameter, changed_parameters) =
                parameter_changes(&before, after, parameter_index)?;
            Ok(crate::api::output::DeviceParameterMutationResult {
                mutation,
                parameter,
                changed_parameters,
            })
        }
        Err(error) => {
            if let Err(rollback_error) = context.audio.set_plugin_state(owner, device_id, previous)
            {
                return Err(AdapterError::runtime(format!(
                    "{error}; plugin parameter rollback failed: {rollback_error}"
                )));
            }
            Err(error)
        }
    }
}

fn parameter_changes(
    before: &[crate::api::output::DeviceParameterInfo],
    mut after: Vec<crate::api::output::DeviceParameterInfo>,
    parameter_index: u32,
) -> Result<
    (
        crate::api::output::DeviceParameterInfo,
        Vec<crate::api::output::DeviceParameterInfo>,
    ),
    AdapterError,
> {
    let target = after
        .iter()
        .position(|parameter| parameter.index == parameter_index)
        .ok_or_else(|| AdapterError::command("device parameter missing after setting"))?;
    let parameter = after.remove(target);
    after.retain(|current| {
        before
            .iter()
            .find(|previous| previous.index == current.index)
            .is_none_or(|previous| {
                previous.name != current.name
                    || previous.display_value != current.display_value
                    || previous.value != current.value
            })
    });
    after.sort_by_key(|parameter| parameter.index);
    Ok((parameter, after))
}

pub fn open_plugin_editor(
    context: &mut SessionContext<'_>,
    owner: &riffra_core::EffectOwner,
    device_id: &str,
) -> Result<(), AdapterError> {
    let session = current_session(context)?;
    let device = session
        .arrangement
        .device(owner, device_id)
        .map_err(|error| AdapterError::command(error.to_string()))?;
    if device.plugin().is_none() {
        return Err("built-in instruments do not provide a plugin editor".into());
    }
    drop(session);
    let project_id = context
        .storage
        .project_id()
        .map_err(|error| AdapterError::runtime(error.to_string()))?;
    context
        .audio
        .open_plugin_editor(&project_id, owner, device_id)?;
    Ok(())
}

/// Rewrites every canonical Asset reference pointed to by `asset_id` to the
/// user's new file and persists the updated session. The Asset's
/// `content_location` is also updated so future operations resolve to the new
/// path.
pub fn relink_missing_dependency(
    context: &mut SessionContext<'_>,
    asset_id: AssetId,
    new_path: &str,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
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
    let committed =
        context.commit(|mut app| app.replace_asset_references(&asset_id, new_asset_id))?;
    Ok(committed.0)
}

/// Marks a missing plugin device as a disabled placeholder so it no longer
/// surfaces as a missing dependency. The session is persisted through the
/// canonical commit.
pub fn disable_missing_plugin(
    context: &mut SessionContext<'_>,
    device_id: &str,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    let committed = context.commit(|mut app| app.disable_missing_plugin(device_id))?;
    Ok(committed.0)
}

/// Replaces an unresolved Track Device in place so its chain position and id
/// remain stable while the plugin binary and plugin state are refreshed.
pub(crate) fn replace_missing_track_plugin_with_expected_sequence(
    context: &mut SessionContext<'_>,
    device_id: &str,
    new_path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    let path = Path::new(new_path.trim());
    let session = current_session(context).map_err(AdapterError::command)?;
    let role = plugin_device_role(&session, device_id).ok_or_else(|| {
        AdapterError::command(format!(
            "Track plugin device is not registered: {device_id}"
        ))
    })?;
    let (name, validated_path) = plugins::validated_plugin(context.data_root, path, role)?;
    let prepared = context
        .prepare_core()
        .application(&context.storage)
        .prepare_track_plugin_replacement(
            device_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{CreativeSession, EffectDevice, TimelineTick, Track};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn parameter_readback_reports_target_and_sorted_changes() {
        let parameter = |index| {
            serde_json::from_value::<crate::api::output::DeviceParameterInfo>(serde_json::json!({
                "parameterId":null,"index":index,"name":"Gain","value":0.5,
                "defaultValue":0.5,"automatable":true,"displayValue":"50%",
                "label":"","discrete":false,"stepCount":0,"choices":[]
            }))
            .unwrap()
        };
        let before: Vec<_> = (0..5).map(parameter).collect();
        let mut after = before.clone();
        after[0].value = 0.75;
        after[1].display_value = "new display".into();
        after[3].name = Some("new name".into());
        after[4].value = 0.25;
        after.reverse();

        let (target, changed) = parameter_changes(&before, after, 0).unwrap();

        assert_eq!(target.index, 0);
        assert_eq!(target.value, 0.75);
        assert_eq!(
            changed.iter().map(|p| p.index).collect::<Vec<_>>(),
            [1, 3, 4]
        );
        assert_eq!(changed[0].display_value, "new display");
        assert_eq!(changed[1].name.as_deref(), Some("new name"));
        assert_eq!(changed[2].value, 0.25);
    }

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

    impl crate::runtime::ports::ProjectionDriver for CandidateRuntimeDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: &crate::execution::TimelineSnapshot,
            _timeout: Duration,
        ) -> Result<(), crate::RuntimeError> {
            if self.fail_prepare.swap(false, Ordering::AcqRel) {
                return Err(crate::RuntimeError::NativeRejected(
                    "Candidate graph was rejected.".into(),
                ));
            }
            *self.pending.lock().unwrap() = Some(snapshot.revision);
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

    impl crate::runtime::ports::TransportDriver for CandidateRuntimeDriver {
        fn set_transport_starting(&self) -> Result<(), crate::RuntimeError> {
            Ok(())
        }

        fn play_timeline(&self) -> Result<(), crate::RuntimeError> {
            Ok(())
        }

        fn pause_timeline(&self) -> Result<(), crate::RuntimeError> {
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

    fn project(root: &Path, session: CreativeSession) -> crate::host::open_project::ProjectCell {
        crate::host::open_project::ProjectCell::new(crate::host::open_project::OpenProject {
            storage: riffra_host::SessionStore::new(root, "01900000-0000-7000-8000-000000000001"),
            core: riffra_core::AppCore::new(
                "01900000-0000-7000-8000-000000000001".into(),
                session,
                0,
            ),
            recovered_from_generation: false,
        })
    }

    fn candidate_context<'a>(
        root: &'a Path,
        runtime: &'a crate::RuntimeReconciler<CandidateRuntimeDriver>,
        audio: &'a crate::AudioSupervisor,
        project: &'a crate::host::open_project::ProjectCell,
    ) -> SessionContext<'a, CandidateRuntimeDriver> {
        let snapshot = project.read().canonical.clone();
        let storage = project.write().project().storage.clone();
        SessionContext {
            project,
            snapshot,
            writer: None,
            audio,
            runtime,
            storage,
            data_root: root,
            built_in_instruments: crate::test_support::empty_built_in_catalog(),
            safe_mode: false,
            publish: &|_writer, committed| crate::api::output::ArrangementMutationResult {
                canonical: committed.snapshot.canonical.clone(),
                projection: crate::api::output::ArrangementProjectionOutcome::NotRequired,
                created_entity_ids: Default::default(),
            },
        }
    }

    fn prepared_plugin_candidate<D: RuntimeDriver>(
        context: &SessionContext<'_, D>,
    ) -> riffra_core::PreparedSession {
        context
            .prepare_core()
            .application(&context.storage)
            .prepare_effect_with_created_id(
                &riffra_core::EffectOwner::Track {
                    id: "track:plugin".to_owned(),
                },
                "Candidate".into(),
                r"C:\plugins\Candidate.vst3".into(),
            )
            .unwrap()
            .0
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
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = project(&root, session);
        let mut context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_device_arrangement(&mut context, candidate);

        // Assert
        assert!(result.is_err());
        assert!(
            core.read().canonical.clone().session.arrangement.tracks[0]
                .effects
                .is_empty()
        );
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), [0]);
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
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = Arc::new(project(&root, session));
        let hook_core = Arc::clone(&core);
        driver.set_commit_hook(Arc::new(move || {
            let committed = hook_core
                .write()
                .commit(|mut app| {
                    app.add_marker_with_created_ids(TimelineTick(7), "concurrent".into())
                })
                .unwrap();
            assert_eq!(committed.snapshot.canonical.sequence, 1);
        }));
        let mut context = candidate_context(&root, &runtime, &audio, core.as_ref());

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_device_arrangement(&mut context, candidate);

        // Assert
        assert!(matches!(
            result,
            Err(AdapterError::Conflict {
                expected_sequence: 0,
                current_sequence: 1,
            })
        ));
        let current = core.read().canonical.clone().session;
        assert_eq!(current.arrangement.revision, 1);
        assert!(current.arrangement.tracks[0].effects.is_empty());
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
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = project(&root, plugin_base_session());
        let mut context = candidate_context(&root, &runtime, &audio, &core);
        let candidate = prepared_plugin_candidate(&context);
        let committed = core
            .write()
            .commit(|mut app| app.add_marker_with_created_ids(TimelineTick(7), "concurrent".into()))
            .unwrap();
        assert_eq!(committed.snapshot.canonical.sequence, 1);

        // Act
        let result = commit_device_arrangement(&mut context, candidate);

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
        let runtime = crate::RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        let audio = crate::AudioSupervisor::offline("test");
        let core = project(&root, session);
        let mut context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_device_arrangement(&mut context, candidate);

        // Assert
        assert!(result.is_err());
        assert!(
            core.read().canonical.clone().session.arrangement.tracks[0]
                .effects
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
        track.effects.push(
            EffectDevice::new(
                "device:amp".into(),
                "Amp".into(),
                r"C:\plugins\Amp.vst3".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks.push(track);
        let mut bus = riffra_core::MixBus {
            id: "bus:group".into(),
            name: "Group".into(),
            kind: riffra_core::MixBusKind::Group,
            gain_db: 0.0,
            pan: 0.0,
            pan_law: riffra_core::PanLaw::default(),
            muted: false,
            solo: false,
            effects: vec![],
        };
        bus.effects.push(
            EffectDevice::new("device:bus".into(), "Bus Effect".into(), "Bus.vst3".into()).unwrap(),
        );
        session.arrangement.mixer.buses.push(bus);
        session.arrangement.mixer.master_effects.push(
            EffectDevice::new(
                "device:master".into(),
                "Master Effect".into(),
                "Master.vst3".into(),
            )
            .unwrap(),
        );
        let core = project(&root, session);
        let store = riffra_host::SessionStore::new(&root, "01900000-0000-7000-8000-000000000001");
        store.ensure_layout().unwrap();
        let saved = core
            .write()
            .commit(|mut app| {
                app.persist_plugin_state(
                    &riffra_core::EffectOwner::Track {
                        id: ("track:guitar").to_owned(),
                    },
                    "device:amp",
                    vec![0.25, 0.75],
                    Some("opaque-state".into()),
                    true,
                )
            })
            .unwrap()
            .value;
        let restored = riffra_core::deserialize_session_document(
            &riffra_core::serialize_session_document(&saved).unwrap(),
        )
        .unwrap();
        assert_eq!(
            restored.arrangement.tracks[0].effects[0]
                .plugin
                .parameter_values,
            [0.25, 0.75]
        );
        for (owner, id) in [
            (
                riffra_core::EffectOwner::Bus {
                    id: "bus:group".into(),
                },
                "device:bus",
            ),
            (riffra_core::EffectOwner::Master, "device:master"),
        ] {
            let _ = core
                .write()
                .commit(|mut app| {
                    app.persist_plugin_state(
                        &owner,
                        id,
                        vec![0.25, 0.75],
                        Some("opaque-state".into()),
                        true,
                    )
                })
                .unwrap();
        }
        let restored = store.load_existing().unwrap().session;
        for device in [
            &restored.arrangement.tracks[0].effects[0],
            &restored.arrangement.mixer.buses[0].effects[0],
            &restored.arrangement.mixer.master_effects[0],
        ] {
            assert_eq!(device.plugin.parameter_values, [0.25, 0.75]);
            assert_eq!(device.plugin.state_data.as_deref(), Some("opaque-state"));
            assert!(device.bypassed);
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
