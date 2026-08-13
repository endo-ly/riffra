//! Desktop adapters for Core Session operations and host runtime services.
//!
//! The operations use three consistency policies:
//!
//! - Sample-pad operations ([`create_sample_pad`], [`update_sample_pad`],
//!   [`remove_sample_pad`]) touch play state, view state, the Asset
//!   registry (existence check), and the Audio Runtime (pad configuration).
//!   Because the runtime and the persisted session must agree, each operation
//!   applies the new pad set to the runtime, persists the session, and restores
//!   the previous pad set when persistence fails.
//!
//! - Arrangement operations that change plugin topology prepare the proposed
//!   runtime graph before persisting the canonical Session. A failed candidate
//!   is rejected, and a persistence failure restores the previous graph. Other
//!   Arrangement operations commit first and submit a nonblocking projection.
//!
//! - Pure-session operations ([`import_session`] and [`restore_generation`])
//!   mutate the session and persist it without waiting for VST lifecycle work.
//!   Design navigation and workspace navigation are view state: they are
//!   returned as in-memory snapshots and send only a nonblocking desired
//!   runtime mode, so navigation never enters the durable Session commit path.
//!
//! Core owns editing rules, canonical state, history, and conflict decisions.
//! This layer resolves files and plugins, invokes native services, delegates
//! production changes to Core, and compensates host resources when an external
//! operation fails.

mod arrangement;
mod midi_import;
mod presentation;
mod rack;
mod recording;
mod runtime;
mod sample_pad;

pub(crate) use arrangement::*;
pub(crate) use midi_import::*;
pub(crate) use presentation::*;
pub(crate) use rack::*;
pub(crate) use recording::*;
pub(crate) use runtime::*;
pub(crate) use sample_pad::*;

#[cfg(test)]
use rack::commit_plugin_arrangement;

use std::collections::HashMap;
use std::{fs, path::Path};

use crate::asset;
use crate::model::{AudioStatus, SessionAudioPair};
use crate::plugin_catalog;
use crate::presentation::{DesignTool, DesktopViewState, Workspace};
use crate::runtime::ports::RuntimeDriver;
use crate::storage::SessionStore;
use riffra_core::{
    AssetId, AssetKind, AudioClipMove, AudioClipPatch, AudioTakeVariant, AutomationParameter,
    AutomationPoint, CreativeSession, MidiClipMove, MidiClipPatch, MidiEvent, MidiEventKind,
    MidiInputRoute, MidiNote, ProjectTimebase, SamplePad, TimelineTick, TrackKind,
};

pub(crate) use crate::session::commit::{
    commit_core_application, commit_recording_session, import_session, restore_generation,
};
pub(crate) use crate::session::context::{SessionContext, current_session, lock_error};
pub(crate) use crate::session::transport::{
    SamplePadRestoreOutcome, audio_command_succeeded, go_to_start_timeline, play_timeline,
    prepare_arrangement_candidate, resolve_native_pads, restore_sample_pads,
    runtime_snapshot_for_recording, seek_timeline, stop_timeline, switch_workspace,
    sync_arrangement, sync_arrangement_runtime,
};
use riffra_core::application::{
    AudioAssetClipPlacement, MarkerPatch, MidiAssetClipPlacement, MidiNotePatch, MidiNoteUpdate,
    SamplePadPatch, SessionSettingsPatch,
};
use riffra_core::domain::TrackPatch;
#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{
        AudioClip, DeviceKind, RackDevice, RecordingPassRecord, RecordingTakeRecord,
        TakeAudioSource, Track,
    };
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Barrier, Mutex, OnceLock, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    struct BarrierCommitDriver {
        commit_started: Arc<Barrier>,
        release_commit: Arc<Barrier>,
        commit_gate_used: AtomicBool,
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        generation: AtomicU64,
    }

    impl BarrierCommitDriver {
        fn new() -> Self {
            Self {
                commit_started: Arc::new(Barrier::new(2)),
                release_commit: Arc::new(Barrier::new(2)),
                commit_gate_used: AtomicBool::new(false),
                loaded: Mutex::new(Vec::new()),
                pending: Mutex::new(None),
                generation: AtomicU64::new(1),
            }
        }
    }

    impl crate::runtime::ports::ProjectionDriver for BarrierCommitDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: Value,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            *self.pending.lock().unwrap() = Some(snapshot["revision"].as_u64().unwrap());
            Ok(())
        }

        fn commit_timeline_snapshot(
            &self,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            if self
                .commit_gate_used
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.commit_started.wait();
                self.release_commit.wait();
            }
            let revision = self.pending.lock().unwrap().take().ok_or_else(|| {
                crate::runtime::error::RuntimeError::NativeRejected(
                    "No prepared timeline snapshot is available.".into(),
                )
            })?;
            self.loaded.lock().unwrap().push(revision);
            Ok(())
        }

        fn discard_timeline_snapshot(
            &self,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            self.pending.lock().unwrap().take();
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            self.generation.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl crate::runtime::ports::TransportDriver for BarrierCommitDriver {
        fn play_timeline(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            Ok(())
        }

        fn stop_timeline_nonblocking(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            self.stop_timeline()
        }
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
            snapshot: Value,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            if self.fail_prepare.swap(false, Ordering::AcqRel) {
                return Err(crate::runtime::error::RuntimeError::NativeRejected(
                    "Candidate graph was rejected.".into(),
                ));
            }
            *self.pending.lock().unwrap() = Some(snapshot["revision"].as_u64().unwrap());
            Ok(())
        }

        fn commit_timeline_snapshot(
            &self,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            if let Some(hook) = self.commit_hook.lock().unwrap().take() {
                hook();
            }
            let revision = self.pending.lock().unwrap().take().ok_or_else(|| {
                crate::runtime::error::RuntimeError::NativeRejected(
                    "No prepared timeline snapshot is available.".into(),
                )
            })?;
            self.loaded.lock().unwrap().push(revision);
            Ok(())
        }

        fn discard_timeline_snapshot(
            &self,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            self.pending.lock().unwrap().take();
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            self.generation.load(Ordering::Relaxed)
        }
    }

    impl crate::runtime::ports::TransportDriver for CandidateRuntimeDriver {
        fn play_timeline(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            Ok(())
        }

        fn stop_timeline_nonblocking(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            self.stop_timeline()
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

    fn test_view_state() -> &'static Mutex<crate::presentation::DesktopViewState> {
        static VIEW_STATE: OnceLock<Mutex<crate::presentation::DesktopViewState>> = OnceLock::new();
        VIEW_STATE.get_or_init(|| Mutex::new(Default::default()))
    }

    fn candidate_context<'a>(
        root: &'a Path,
        runtime: &'a crate::runtime::RuntimeReconciler<CandidateRuntimeDriver>,
        audio: &'a crate::native_audio::AudioSupervisor,
        core: &'a riffra_core::AppCore<crate::native_audio::AudioSupervisor>,
    ) -> SessionContext<'a, CandidateRuntimeDriver> {
        SessionContext {
            core,
            view_state: test_view_state(),
            audio,
            runtime,
            data_root: root,
            safe_mode: false,
        }
    }

    fn prepared_plugin_candidate<D: RuntimeDriver>(
        context: &SessionContext<'_, D>,
    ) -> riffra_core::PreparedSession {
        let store = SessionStore::new(context.data_root);
        context
            .core
            .application(&store)
            .prepare_track_effect(
                "track:plugin",
                "Candidate".into(),
                r"C:\plugins\Candidate.vst3".into(),
            )
            .unwrap()
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
            crate::storage::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = plugin_base_session();
        let driver = Arc::new(CandidateRuntimeDriver::new(true));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio.clone(), false, false);
        let context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_plugin_arrangement(&context, candidate);

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
            crate::storage::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = plugin_base_session();
        let driver = Arc::new(CandidateRuntimeDriver::new(true));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio.clone(), false, false);
        let context = candidate_context(&root, &runtime, &audio, &core);
        let candidate = prepared_plugin_candidate(&context);
        assert!(commit_plugin_arrangement(&context, candidate).is_err());
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
            crate::storage::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = plugin_base_session();
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
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
            let store = crate::storage::SessionStore::new(&hook_root);
            hook_core
                .application(&store)
                .add_marker(TimelineTick(7), "concurrent".into())
                .unwrap();
        }));
        let context = candidate_context(&root, &runtime, &audio, core.as_ref());

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_plugin_arrangement(&context, candidate);

        // Assert
        assert!(result.is_err());
        let current = core.snapshot().unwrap().session;
        assert_eq!(current.arrangement.revision, 1);
        assert!(current.arrangement.tracks[0].rack.devices.is_empty());
        assert_eq!(driver.loaded.lock().unwrap().last(), Some(&1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_persistence_failure_restores_the_previous_graph() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-persistence-{}",
            crate::storage::now_ms()
        ));
        std::fs::write(&root, b"not a directory").unwrap();
        let session = plugin_base_session();
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio.clone(), false, false);
        let context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let candidate = prepared_plugin_candidate(&context);
        let result = commit_plugin_arrangement(&context, candidate);

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
    fn update_track_returns_while_runtime_commit_is_blocked() {
        // Arrange
        let root =
            std::env::temp_dir().join(format!("riffra-barrier-{}", crate::storage::now_ms()));
        let session = {
            let mut session = CreativeSession::new(1);
            session
                .arrangement
                .tracks
                .push(Track::audio("track:a".into(), "Audio".into()));
            session
                .arrangement
                .tracks
                .push(Track::audio("track:b".into(), "Audio".into()));
            session
        };
        let store = crate::storage::SessionStore::new(&root);
        store.ensure_layout().unwrap();
        let driver = Arc::new(BarrierCommitDriver::new());
        let runtime =
            Arc::new(crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap());
        let audio = Arc::new(crate::native_audio::AudioSupervisor::offline("test"));
        let core = Arc::new(riffra_core::AppCore::new(
            root.clone(),
            session,
            audio.as_ref().clone(),
            false,
            false,
        ));
        let context = SessionContext {
            core: core.as_ref(),
            view_state: test_view_state(),
            audio: audio.as_ref(),
            runtime: runtime.as_ref(),
            data_root: &root,
            safe_mode: false,
        };

        update_track(
            &context,
            "track:a",
            TrackPatch {
                muted: Some(true),
                ..Default::default()
            },
        )
        .expect("initial update_track must succeed");
        driver.commit_started.wait();

        let (update_result_tx, update_result_rx) = mpsc::channel();
        let update_context = {
            let runtime = Arc::clone(&runtime);
            let audio = Arc::clone(&audio);
            let core = Arc::clone(&core);
            let root = root.clone();
            thread::spawn(move || {
                let context = SessionContext {
                    core: core.as_ref(),
                    view_state: test_view_state(),
                    audio: audio.as_ref(),
                    runtime: runtime.as_ref(),
                    data_root: &root,
                    safe_mode: false,
                };
                let result = update_track(
                    &context,
                    "track:b",
                    TrackPatch {
                        muted: Some(true),
                        ..Default::default()
                    },
                );
                update_result_tx.send(result).unwrap();
            })
        };
        let update_result = update_result_rx.recv_timeout(Duration::from_secs(1));

        // Act
        driver.release_commit.wait();
        update_context.join().unwrap();

        // Assert
        update_result
            .expect("update_track must return while commit is blocked")
            .expect("update_track must succeed while commit is blocked");
        let expected_revision = core.snapshot().unwrap().session.arrangement.revision;
        wait_until(Duration::from_secs(1), || {
            driver.loaded.lock().unwrap().last() == Some(&expected_revision)
        });
        assert_eq!(
            driver.loaded.lock().unwrap().last(),
            Some(&expected_revision)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_editor_state_survives_canonical_session_round_trip() {
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-state-round-trip-{}",
            crate::storage::now_ms()
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
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio, false, true);
        let store = crate::storage::SessionStore::new(&root);
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

    #[test]
    fn take_variant_is_applied_only_to_the_selected_clip() {
        let raw_id = riffra_core::mint_asset_id();
        let processed_id = riffra_core::mint_asset_id();
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:audio".into(), "Audio".into()));
        session.arrangement.takes.push(RecordingTakeRecord {
            id: "take:1".into(),
            session_id: "recording:1".into(),
            pass_id: "pass:1".into(),
            track_id: "track:audio".into(),
            start_tick: TimelineTick(0),
            duration_ticks: 960,
            source_start_sample: 0,
            source_end_sample: 1_000,
            raw_audio: Some(TakeAudioSource {
                asset_id: raw_id.clone(),
                source_start_sample: 0,
                source_end_sample: 1_000,
                tail_end_sample: 1_000,
                sample_rate: 48_000,
            }),
            processed_audio: Some(TakeAudioSource {
                asset_id: processed_id.clone(),
                source_start_sample: 128,
                source_end_sample: 1_256,
                tail_end_sample: 1_256,
                sample_rate: 48_000,
            }),
            midi_asset_id: None,
        });
        for id in ["clip:a", "clip:b"] {
            let mut clip = AudioClip::full_source(
                id.into(),
                id.into(),
                "track:audio".into(),
                raw_id.clone(),
                TimelineTick(0),
                48_000,
                1_000,
            );
            clip.recording_take_id = Some("take:1".into());
            session.arrangement.audio_clips.push(clip);
        }
        session
            .arrangement
            .recording_passes
            .push(RecordingPassRecord {
                id: "pass:1".into(),
                session_id: "recording:1".into(),
                ordinal: 1,
                start_tick: TimelineTick(0),
                duration_ticks: 960,
                partial_start: false,
                partial_end: false,
                track_take_ids: vec!["take:1".into()],
            });
        session
            .arrangement
            .recording_sessions
            .push(riffra_core::RecordingSessionRecord {
                id: "recording:1".into(),
                start_tick: TimelineTick(0),
                track_slots: vec![riffra_core::RecordingSessionTrackSlot {
                    track_id: "track:audio".into(),
                    active_take_id: "take:1".into(),
                    timeline_clip_id: "clip:a".into(),
                }],
                pass_ids: vec!["pass:1".into()],
            });
        let root =
            std::env::temp_dir().join(format!("riffra-take-variant-{}", crate::storage::now_ms()));
        struct MemoryStorage;
        impl riffra_core::SessionStorage for MemoryStorage {
            fn save(&self, _session: &CreativeSession) -> Result<(), riffra_core::PortError> {
                Ok(())
            }
        }
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio, false, true);
        let store = MemoryStorage;
        let changed = core
            .application(&store)
            .set_audio_clip_take_variant("clip:a", AudioTakeVariant::Processed)
            .unwrap();

        let selected = &changed.arrangement.audio_clips[0];
        let untouched = &changed.arrangement.audio_clips[1];
        assert_eq!(selected.asset_id, processed_id);
        assert_eq!(
            selected.source_range,
            riffra_core::FrameRange {
                start: 128,
                end: 1_128
            }
        );
        assert_eq!(selected.timeline_duration.frames, 1_000);
        assert_eq!(untouched.asset_id, raw_id);
        assert_eq!(untouched.take_variant, AudioTakeVariant::Raw);
        let _ = std::fs::remove_dir_all(root);
    }
}
