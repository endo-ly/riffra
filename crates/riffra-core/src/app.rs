use crate::errors::ApplicationError;
use crate::history::History;
use crate::ports::{PortError, RuntimeProjection, RuntimeProjectionRequest, SessionStorage};
use crate::session::CreativeSession;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use ts_rs::TS;

/// A consistent canonical session snapshot and its Core projection sequence.
#[derive(Clone, Debug)]
pub struct CanonicalSnapshot {
    /// Canonical production state at the capture boundary.
    pub session: CreativeSession,
    /// Sequence assigned by the Core commit boundary.
    pub sequence: u64,
}

/// A read-only handle for integrations that outlive one command invocation.
#[derive(Clone)]
pub struct CanonicalSessionHandle {
    session: Arc<Mutex<CreativeSession>>,
}

impl CanonicalSessionHandle {
    /// Captures the current canonical production state.
    pub fn snapshot(&self) -> Result<CreativeSession, ApplicationError> {
        self.session
            .lock()
            .map_err(|_| ApplicationError::StateLock)
            .map(|session| session.clone())
    }
}

/// Read-only history capabilities exposed to a host UI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistoryState {
    /// Whether an undo operation can be performed.
    pub can_undo: bool,
    /// Whether a redo operation can be performed.
    pub can_redo: bool,
}

/// Platform-independent application state shared by every Riffra host.
pub struct AppCore<A> {
    data_root: PathBuf,
    session: Arc<Mutex<CreativeSession>>,
    audio: A,
    recovered_from_generation: bool,
    safe_mode: bool,
    operation_gate: Mutex<()>,
    projection_version: AtomicU64,
    history: Mutex<History>,
}

impl<A> AppCore<A> {
    /// Creates application state from an already-loaded canonical session.
    pub fn new(
        data_root: PathBuf,
        session: CreativeSession,
        audio: A,
        recovered_from_generation: bool,
        safe_mode: bool,
    ) -> Self {
        Self {
            data_root,
            session: Arc::new(Mutex::new(session)),
            audio,
            recovered_from_generation,
            safe_mode,
            operation_gate: Mutex::new(()),
            projection_version: AtomicU64::new(0),
            history: Mutex::new(History::default()),
        }
    }

    /// Creates Core state around an existing canonical session handle.
    ///
    /// Hosts use this when the session storage handle is already shared with
    /// another adapter that observes the same canonical state.
    pub fn from_shared_session(
        data_root: PathBuf,
        session: Arc<Mutex<CreativeSession>>,
        audio: A,
        recovered_from_generation: bool,
        safe_mode: bool,
    ) -> Self {
        Self {
            data_root,
            session,
            audio,
            recovered_from_generation,
            safe_mode,
            operation_gate: Mutex::new(()),
            projection_version: AtomicU64::new(0),
            history: Mutex::new(History::default()),
        }
    }

    /// Returns the root used for durable application data.
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// Returns the shared canonical session handle for host integrations that
    /// must observe state while outliving a single command.
    pub fn shared_session(&self) -> CanonicalSessionHandle {
        CanonicalSessionHandle {
            session: Arc::clone(&self.session),
        }
    }

    /// Returns the host-provided live audio service.
    pub fn audio(&self) -> &A {
        &self.audio
    }

    /// Reports whether startup restored a recovery generation.
    pub fn recovered_from_generation(&self) -> bool {
        self.recovered_from_generation
    }

    /// Reports whether external devices and plugins are isolated.
    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    /// Captures the canonical session and projection sequence as one pair.
    pub fn snapshot(&self) -> Result<CanonicalSnapshot, ApplicationError> {
        loop {
            let before = self.projection_version.load(Ordering::Acquire);
            if !before.is_multiple_of(2) {
                std::thread::yield_now();
                continue;
            }
            let session = self
                .session
                .lock()
                .map_err(|_| ApplicationError::StateLock)?
                .clone();
            let after = self.projection_version.load(Ordering::Acquire);
            if before == after && after.is_multiple_of(2) {
                return Ok(CanonicalSnapshot {
                    session,
                    sequence: after / 2,
                });
            }
            std::thread::yield_now();
        }
    }

    /// Commits one user-intent mutation through validation, persistence, the
    /// canonical state exchange, and Core-owned history.
    pub fn commit<S, F>(&self, storage: &S, edit: F) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
        F: FnOnce(&mut CreativeSession) -> Result<(), ApplicationError>,
    {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        let current = self
            .session
            .lock()
            .map_err(|_| ApplicationError::StateLock)?
            .clone();
        let mut candidate = current.clone();
        edit(&mut candidate)?;
        self.commit_candidate_locked(storage, current, candidate)
    }

    pub(crate) fn commit_at_sequence<S, F>(
        &self,
        storage: &S,
        expected_sequence: u64,
        edit: F,
    ) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
        F: FnOnce(&mut CreativeSession) -> Result<(), ApplicationError>,
    {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        let current = self
            .session
            .lock()
            .map_err(|_| ApplicationError::StateLock)?
            .clone();
        let current_sequence = self.projection_version.load(Ordering::Acquire) / 2;
        if current_sequence != expected_sequence {
            return Err(ApplicationError::InvalidCommand(
                "canonical session changed while the operation was being prepared".into(),
            ));
        }
        let mut candidate = current.clone();
        edit(&mut candidate)?;
        self.commit_candidate_locked(storage, current, candidate)
    }

    /// Commits a production edit and submits the resulting canonical snapshot
    /// to a host runtime projection Port.
    ///
    /// The canonical commit remains durable when the runtime rejects the
    /// projection; callers can retry the projection from the returned Core
    /// snapshot without losing production data.
    pub fn commit_with_projection<S, P, F>(
        &self,
        storage: &S,
        projection: &P,
        edit: F,
    ) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
        P: RuntimeProjection + ?Sized,
        F: FnOnce(&mut CreativeSession) -> Result<(), ApplicationError>,
    {
        let committed = self.commit(storage, edit)?;
        self.project_current(projection)?;
        Ok(committed)
    }

    /// Submits the current canonical snapshot to a host runtime projection
    /// Port without changing production state.
    pub fn project_current<P>(&self, projection: &P) -> Result<(), ApplicationError>
    where
        P: RuntimeProjection + ?Sized,
    {
        let snapshot = self.snapshot()?;
        projection
            .project(RuntimeProjectionRequest::new(
                snapshot.session,
                snapshot.sequence,
            ))
            .map_err(application_error_from_port)
    }

    /// Commits a prepared candidate. This is useful for a long-running host
    /// operation that merges only its owned fields onto the latest snapshot.
    pub(crate) fn commit_candidate<S>(
        &self,
        storage: &S,
        candidate: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
    {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        let current = self
            .session
            .lock()
            .map_err(|_| ApplicationError::StateLock)?
            .clone();
        self.commit_candidate_locked(storage, current, candidate)
    }

    /// Commits a candidate merged with the latest canonical state while the
    /// operation boundary is held, preventing stale long-running results from
    /// erasing edits made after the operation started.
    pub(crate) fn commit_merged<S, F>(
        &self,
        storage: &S,
        base: &CreativeSession,
        candidate: CreativeSession,
        merge: F,
    ) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
        F: FnOnce(&CreativeSession, &CreativeSession, CreativeSession) -> CreativeSession,
    {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        let current = self
            .session
            .lock()
            .map_err(|_| ApplicationError::StateLock)?
            .clone();
        let merged = merge(&current, base, candidate);
        self.commit_candidate_locked(storage, current, merged)
    }

    /// Undoes the latest committed user edit and persists the restored state.
    pub fn undo<S>(&self, storage: &S) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
    {
        self.restore_history_entry(storage, true)
    }

    /// Redoes the latest undone user edit and persists the restored state.
    pub fn redo<S>(&self, storage: &S) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
    {
        self.restore_history_entry(storage, false)
    }

    /// Returns the current Core-owned history capabilities.
    pub fn history_state(&self) -> Result<HistoryState, ApplicationError> {
        let history = self
            .history
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        Ok(HistoryState {
            can_undo: history.can_undo(),
            can_redo: history.can_redo(),
        })
    }

    fn commit_candidate_locked<S>(
        &self,
        storage: &S,
        current: CreativeSession,
        mut candidate: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
    {
        candidate = candidate
            .validate_and_normalize()
            .map_err(ApplicationError::InvalidSession)?;
        if candidate == current {
            return Ok(current);
        }
        candidate.updated_at_ms =
            next_update_timestamp(current.updated_at_ms, candidate.updated_at_ms);
        storage
            .save(&candidate)
            .map_err(application_error_from_port)?;

        self.begin_exchange();
        if let Ok(mut session) = self.session.lock() {
            *session = candidate.clone();
        } else {
            self.end_exchange();
            return Err(ApplicationError::StateLock);
        }
        self.end_exchange();
        self.history
            .lock()
            .map_err(|_| ApplicationError::StateLock)?
            .record(current);
        Ok(candidate)
    }

    fn restore_history_entry<S>(
        &self,
        storage: &S,
        undo: bool,
    ) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
    {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        let current = self
            .session
            .lock()
            .map_err(|_| ApplicationError::StateLock)?
            .clone();
        let history_entry = {
            let mut history = self
                .history
                .lock()
                .map_err(|_| ApplicationError::StateLock)?;
            if undo {
                history.take_undo()
            } else {
                history.take_redo()
            }
        }
        .ok_or(ApplicationError::HistoryEmpty)?;
        let mut target = match history_entry.clone().validate_and_normalize() {
            Ok(target) => target,
            Err(error) => {
                let mut history = self
                    .history
                    .lock()
                    .map_err(|_| ApplicationError::StateLock)?;
                if undo {
                    history.push_undo(history_entry);
                } else {
                    history.push_redo(history_entry);
                }
                return Err(ApplicationError::InvalidSession(error));
            }
        };
        target.updated_at_ms = next_update_timestamp(current.updated_at_ms, target.updated_at_ms);
        if let Err(error) = storage.save(&target) {
            let mut history = self
                .history
                .lock()
                .map_err(|_| ApplicationError::StateLock)?;
            if undo {
                history.push_undo(history_entry);
            } else {
                history.push_redo(history_entry);
            }
            return Err(application_error_from_port(error));
        }
        self.begin_exchange();
        if let Ok(mut session) = self.session.lock() {
            *session = target.clone();
        } else {
            self.end_exchange();
            return Err(ApplicationError::StateLock);
        }
        self.end_exchange();
        let mut history = self
            .history
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        if undo {
            history.push_redo(current);
        } else {
            history.push_undo(current);
        }
        Ok(target)
    }

    fn begin_exchange(&self) {
        let previous = self.projection_version.fetch_add(1, Ordering::AcqRel);
        debug_assert!(previous.is_multiple_of(2));
    }

    fn end_exchange(&self) {
        self.projection_version.fetch_add(1, Ordering::Release);
    }
}

fn next_update_timestamp(previous: u64, candidate: u64) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(previous);
    now.max(candidate).max(previous.saturating_add(1))
}

fn application_error_from_port(error: PortError) -> ApplicationError {
    match error {
        PortError::Storage(message) => ApplicationError::Storage(message),
        PortError::Runtime(message) => ApplicationError::Runtime(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::mint_asset_id;
    use crate::ports::{PortError, RuntimeProjection, RuntimeProjectionRequest, SessionStorage};
    use crate::rack::{DeviceKind, RackDevice};
    use crate::session::{AudioClip, AudioClipMove, MidiClip, SamplePad, TimelineTick, TrackKind};
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStorage {
        sessions: Mutex<Vec<CreativeSession>>,
    }

    impl SessionStorage for MemoryStorage {
        fn save(&self, session: &CreativeSession) -> Result<(), PortError> {
            self.sessions.lock().unwrap().push(session.clone());
            Ok(())
        }
    }

    struct FailingStorage;

    impl SessionStorage for FailingStorage {
        fn save(&self, _session: &CreativeSession) -> Result<(), PortError> {
            Err(PortError::Storage("disk full".into()))
        }
    }

    #[derive(Default)]
    struct MemoryProjection {
        requests: Mutex<Vec<RuntimeProjectionRequest>>,
    }

    impl RuntimeProjection for MemoryProjection {
        fn project(&self, request: RuntimeProjectionRequest) -> Result<(), PortError> {
            self.requests.lock().unwrap().push(request);
            Ok(())
        }
    }

    struct NoopAudio;

    struct FailingProjection;

    impl RuntimeProjection for FailingProjection {
        fn project(&self, _request: RuntimeProjectionRequest) -> Result<(), PortError> {
            Err(PortError::Runtime("runtime unavailable".into()))
        }
    }

    #[test]
    fn commit_serializes_edit_persistence_and_canonical_exchange() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );

        let committed = core
            .application(&storage)
            .add_track("Main", TrackKind::Audio)
            .unwrap();

        assert_eq!(committed.arrangement.tracks.len(), 1);
        assert_eq!(storage.sessions.lock().unwrap().len(), 1);
        assert!(core.history_state().unwrap().can_undo);
    }

    #[test]
    fn application_facade_commits_domain_arrangement_operations() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let application = core.application(&storage);
        let with_track = application.add_track("Audio", TrackKind::Audio).unwrap();
        let track_id = with_track.arrangement.tracks[0].id.clone();
        let asset_id = mint_asset_id();
        let clip = AudioClip::full_source(
            "clip:1".into(),
            "Take".into(),
            track_id.clone(),
            asset_id,
            TimelineTick(0),
            48_000,
            48_000,
        );

        let with_clip = application.add_audio_clip(clip, |_| true).unwrap();
        let moved = application
            .move_audio_clips(vec![AudioClipMove {
                clip_id: "clip:1".into(),
                start_tick: TimelineTick(960),
                track_id: track_id.clone(),
            }])
            .unwrap();

        assert_eq!(with_clip.arrangement.audio_clips.len(), 1);
        assert_eq!(
            moved.arrangement.audio_clips[0].start_tick,
            TimelineTick(960)
        );
        assert_eq!(storage.sessions.lock().unwrap().len(), 3);

        let duplicated = application.duplicate_track(&track_id).unwrap();
        assert_eq!(duplicated.arrangement.tracks.len(), 2);
        assert_eq!(duplicated.arrangement.audio_clips.len(), 2);

        let marked = application
            .add_marker(TimelineTick(1_920), "  Chorus  ".into())
            .unwrap();
        assert_eq!(marked.arrangement.markers[0].name, "Chorus");

        let settings = application
            .update_session_settings(crate::application::SessionSettingsPatch {
                master_db: Some(-6.0),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(settings.settings.master_db, -6.0);
    }

    #[test]
    fn session_settings_are_normalized_at_the_application_boundary() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );

        let committed = core
            .application(&storage)
            .update_session_settings(crate::application::SessionSettingsPatch {
                project_name: Some(Some("  Project  ".into())),
                count_in_beats: Some(99),
                note: Some("note".into()),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(committed.project_name.as_deref(), Some("Project"));
        assert_eq!(committed.settings.count_in_beats, 8);
        assert_eq!(committed.settings.note, "note");
    }

    #[test]
    fn application_facade_owns_midi_note_edits() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let application = core.application(&storage);
        let track = application
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.arrangement.tracks[0].id.clone();
        application
            .add_midi_clip(MidiClip {
                id: "midi:1".into(),
                name: "Pattern".into(),
                track_id,
                asset_id: None,
                start_tick: TimelineTick(0),
                duration_ticks: 1_920,
                notes: Vec::new(),
                events: Vec::new(),
                muted: false,
                loop_enabled: false,
                recording_take_id: None,
            })
            .unwrap();

        let with_note = application
            .add_midi_note("midi:1", TimelineTick(0), 60, 480, 100, 1)
            .unwrap();
        let note_id = with_note.arrangement.midi_clips[0].notes[0].id.clone();
        let updated = application
            .update_midi_notes(
                "midi:1",
                vec![crate::application::MidiNoteUpdate {
                    note_id: note_id.clone(),
                    patch: crate::application::MidiNotePatch {
                        note: Some(61),
                        ..Default::default()
                    },
                }],
            )
            .unwrap();
        assert_eq!(updated.arrangement.midi_clips[0].notes[0].note, 61);

        let duplicated = application
            .duplicate_midi_notes("midi:1", vec![note_id.clone()], 480)
            .unwrap();
        assert_eq!(duplicated.arrangement.midi_clips[0].notes.len(), 2);
        let removed = application.remove_midi_note("midi:1", &note_id).unwrap();
        assert_eq!(removed.arrangement.midi_clips[0].notes.len(), 1);
    }

    #[test]
    fn application_facade_owns_sample_pad_and_plugin_state_edits() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let application = core.application(&storage);
        let asset_id = mint_asset_id();
        let pad = SamplePad {
            id: "pad:one".into(),
            name: "One".into(),
            asset_id,
            start_ms: 0,
            end_ms: 100,
            midi_key: 36,
            gain_db: 0.0,
            loop_enabled: false,
        };
        let added = application.add_sample_pad(pad).unwrap();
        assert_eq!(added.play_state.sample_instrument.pads.len(), 1);
        let updated = application
            .update_sample_pad(
                "pad:one",
                crate::application::SamplePadPatch {
                    start_ms: Some(50),
                    end_ms: Some(25),
                    gain_db: Some(3.0),
                    loop_enabled: Some(true),
                },
            )
            .unwrap();
        let updated_pad = &updated.play_state.sample_instrument.pads[0];
        assert_eq!((updated_pad.start_ms, updated_pad.end_ms), (50, 51));
        assert_eq!(updated_pad.gain_db, 3.0);
        assert!(updated_pad.loop_enabled);
        assert_eq!(
            application
                .remove_sample_pad("pad:one")
                .unwrap()
                .play_state
                .sample_instrument
                .pads
                .len(),
            0
        );

        let track = application
            .add_track("Synth", TrackKind::Instrument)
            .unwrap();
        let track_id = track.arrangement.tracks[0].id.clone();
        let device = RackDevice {
            id: "device:synth".into(),
            name: "Synth".into(),
            kind: DeviceKind::Plugin,
            path: Some("Synth.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        };
        application
            .set_track_instrument(&track_id, Some(device))
            .unwrap();
        let with_state = application
            .persist_track_plugin_state(
                &track_id,
                "device:synth",
                vec![0.25],
                Some("state".into()),
                true,
            )
            .unwrap();
        let instrument = with_state.arrangement.tracks[0]
            .instrument
            .as_ref()
            .unwrap();
        assert_eq!(instrument.parameter_values, [0.25]);
        assert_eq!(instrument.state_data.as_deref(), Some("state"));
        assert!(instrument.bypassed);
        let disabled = application.disable_missing_plugin("device:synth").unwrap();
        assert!(
            disabled.arrangement.tracks[0]
                .instrument
                .as_ref()
                .unwrap()
                .disabled_placeholder
        );

        let replacement = RackDevice {
            path: Some("Other.vst3".into()),
            disabled_placeholder: false,
            ..disabled.arrangement.tracks[0].instrument.clone().unwrap()
        };
        let replaced = application
            .replace_track_plugin("device:synth", replacement)
            .unwrap();
        assert_eq!(
            replaced.arrangement.tracks[0]
                .instrument
                .as_ref()
                .unwrap()
                .path
                .as_deref(),
            Some("Other.vst3")
        );
    }

    #[test]
    fn undo_and_redo_are_core_owned_and_persisted() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        core.application(&storage)
            .add_track("Main", TrackKind::Audio)
            .unwrap();

        let undone = core.undo(&storage).unwrap();
        assert!(undone.arrangement.tracks.is_empty());
        assert!(core.history_state().unwrap().can_redo);

        let redone = core.redo(&storage).unwrap();
        assert_eq!(redone.arrangement.tracks[0].name, "Main");
        assert_eq!(storage.sessions.lock().unwrap().len(), 3);
    }

    #[test]
    fn stale_merge_uses_latest_canonical_state() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let base = core.snapshot().unwrap().session;
        core.application(&storage)
            .add_track("Current", TrackKind::Audio)
            .unwrap();
        let mut stale = base.clone();
        stale.project_name = Some("stale result".into());
        let merged = core
            .commit_merged(&storage, &base, stale, |current, _, candidate| {
                let mut result = current.clone();
                result.project_name = candidate.project_name;
                result
            })
            .unwrap();

        assert_eq!(merged.arrangement.tracks[0].name, "Current");
        assert_eq!(merged.project_name.as_deref(), Some("stale result"));
    }

    #[test]
    fn canonical_snapshot_keeps_sequence_with_session() {
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let snapshot = core.snapshot().unwrap();
        assert_eq!(snapshot.sequence, 0);
        assert_eq!(snapshot.session.session_id, "scratch-1");
    }

    #[test]
    fn persistence_failure_leaves_canonical_state_and_history_unchanged() {
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let before = core.snapshot().unwrap();

        let error = core
            .application(&FailingStorage)
            .add_track("Main", TrackKind::Audio)
            .unwrap_err();

        assert_eq!(error, ApplicationError::Storage("disk full".into()));
        let after = core.snapshot().unwrap();
        assert_eq!(after.session, before.session);
        assert_eq!(after.sequence, before.sequence);
        assert_eq!(core.history_state().unwrap(), HistoryState::default());
    }

    #[test]
    fn committed_snapshot_is_sent_to_the_runtime_projection_port() {
        let storage = MemoryStorage::default();
        let projection = MemoryProjection::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );

        core.commit_with_projection(&storage, &projection, |session| {
            session.project_name = Some("Projected".into());
            Ok(())
        })
        .unwrap();

        let requests = projection.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].session().project_name.as_deref(),
            Some("Projected")
        );
        assert_eq!(requests[0].sequence(), 1);
    }

    #[test]
    fn runtime_projection_failure_does_not_rollback_a_durable_commit() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );

        let error = core
            .commit_with_projection(&storage, &FailingProjection, |session| {
                session.project_name = Some("Durable".into());
                Ok(())
            })
            .unwrap_err();

        assert_eq!(
            error,
            ApplicationError::Runtime("runtime unavailable".into())
        );
        assert_eq!(
            core.snapshot().unwrap().session.project_name.as_deref(),
            Some("Durable")
        );
        assert!(core.history_state().unwrap().can_undo);
        assert_eq!(storage.sessions.lock().unwrap().len(), 1);
    }
}
