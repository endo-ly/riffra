use crate::application::history::History;
use crate::domain::CreativeSession;
use crate::errors::ApplicationError;
use crate::ports::{PortError, RuntimeProjection, RuntimeProjectionRequest, SessionStorage};
use serde::{Deserialize, Serialize};
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

/// Canonical production state and the history capabilities at one revision.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalState {
    /// Canonical production state at the revision boundary.
    pub session: CreativeSession,
    /// Process-local canonical commit revision.
    pub sequence: u64,
    /// Undo/Redo capabilities for this revision.
    pub history: HistoryState,
}

/// A Core-produced candidate that can be inspected by an external runtime and
/// later committed without recreating the production edit in an adapter.
pub struct PreparedSession {
    session: CreativeSession,
    expected_sequence: u64,
}

impl PreparedSession {
    /// Returns the exact candidate that an external runtime should validate.
    pub fn session(&self) -> &CreativeSession {
        &self.session
    }

    /// Returns the canonical sequence from which this candidate was derived.
    pub fn sequence(&self) -> u64 {
        self.expected_sequence
    }

    /// Rebinds the candidate to a caller-provided optimistic-concurrency
    /// revision before an external runtime validates it.
    pub fn with_expected_sequence(mut self, expected_sequence: u64) -> Self {
        self.expected_sequence = expected_sequence;
        self
    }
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
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
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

    /// Replaces the canonical session while switching Project containers.
    ///
    /// Activation advances the canonical sequence and clears all history. It
    /// deliberately does not persist the session because the ProjectStore has
    /// already completed the Project-scoped save before activation.
    ///
    /// # Errors
    /// Returns an error when the supplied session is invalid or Core state is
    /// unavailable.
    pub fn activate_session(
        &self,
        session: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        let session = session
            .validate_and_normalize()
            .map_err(ApplicationError::InvalidSession)?;
        self.begin_exchange();
        if let Ok(mut canonical) = self.session.lock() {
            *canonical = session.clone();
        } else {
            self.end_exchange();
            return Err(ApplicationError::StateLock);
        }
        self.end_exchange();
        self.history
            .lock()
            .map_err(|_| ApplicationError::StateLock)?
            .clear();
        Ok(session)
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

    /// Captures the canonical session, revision, and history capabilities as
    /// one consistent state for host synchronization.
    pub fn canonical_state(&self) -> Result<CanonicalState, ApplicationError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        let sequence = self.projection_version.load(Ordering::Acquire) / 2;
        let session = self
            .session
            .lock()
            .map_err(|_| ApplicationError::StateLock)?
            .clone();
        let history = self
            .history
            .lock()
            .map_err(|_| ApplicationError::StateLock)?;
        Ok(CanonicalState {
            session,
            sequence,
            history: HistoryState {
                can_undo: history.can_undo(),
                can_redo: history.can_redo(),
            },
        })
    }

    /// Commits one user-intent mutation through validation, persistence, the
    /// canonical state exchange, and Core-owned history.
    pub(crate) fn commit<S, F>(
        &self,
        storage: &S,
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
        let mut candidate = current.clone();
        edit(&mut candidate)?;
        self.commit_candidate_locked(storage, current, candidate)
    }

    pub(crate) fn prepare<F>(&self, edit: F) -> Result<PreparedSession, ApplicationError>
    where
        F: FnOnce(&mut CreativeSession) -> Result<(), ApplicationError>,
    {
        let snapshot = self.snapshot()?;
        let mut candidate = snapshot.session;
        edit(&mut candidate)?;
        candidate = candidate
            .validate_and_normalize()
            .map_err(ApplicationError::InvalidSession)?;
        Ok(PreparedSession {
            session: candidate,
            expected_sequence: snapshot.sequence,
        })
    }

    pub(crate) fn commit_prepared<S>(
        &self,
        storage: &S,
        prepared: PreparedSession,
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
        let current_sequence = self.projection_version.load(Ordering::Acquire) / 2;
        if current_sequence != prepared.expected_sequence {
            return Err(ApplicationError::Conflict {
                expected_sequence: prepared.expected_sequence,
                current_sequence,
            });
        }
        self.commit_candidate_locked(storage, current, prepared.session)
    }

    /// Submits the current canonical snapshot to a host runtime projection
    /// Port without changing production state.
    pub(crate) fn project_current<P>(&self, projection: &P) -> Result<(), ApplicationError>
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
    pub(crate) fn undo<S>(&self, storage: &S) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
    {
        self.restore_history_entry(storage, true)
    }

    /// Redoes the latest undone user edit and persists the restored state.
    pub(crate) fn redo<S>(&self, storage: &S) -> Result<CreativeSession, ApplicationError>
    where
        S: SessionStorage + ?Sized,
    {
        self.restore_history_entry(storage, false)
    }

    /// Returns the current Core-owned history capabilities.
    pub(crate) fn history_state(&self) -> Result<HistoryState, ApplicationError> {
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
    use crate::domain::asset::mint_asset_id;
    use crate::domain::{
        AudioClip, AudioClipMove, DeviceKind, FrameRange, MidiClip, MidiNote, RackDevice,
        TimelineTick, TrackKind,
    };
    use crate::ports::{PortError, RuntimeProjection, RuntimeProjectionRequest, SessionStorage};
    use std::sync::{Arc, Barrier, Mutex};

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
    fn commit_persists_and_records_history() {
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
    fn add_track_normalizes_the_canonical_name() {
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
            .add_track(format!("  {}  ", "a".repeat(100)), TrackKind::Audio)
            .unwrap();

        assert_eq!(committed.arrangement.tracks[0].name, "a".repeat(80));
    }

    #[test]
    fn adding_an_audio_asset_creates_the_missing_audio_track() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let asset_id = mint_asset_id();

        let committed = core
            .application(&storage)
            .add_audio_asset_clip(
                crate::application::AudioAssetClipPlacement {
                    asset_id,
                    name: "Take".into(),
                    start_tick: None,
                    track_id: None,
                    sample_rate: 48_000,
                    source_frames: 48_000,
                },
                |_| true,
            )
            .unwrap();

        assert_eq!(committed.arrangement.tracks.len(), 1);
        assert_eq!(committed.arrangement.tracks[0].kind, TrackKind::Audio);
        assert_eq!(committed.arrangement.audio_clips.len(), 1);
        assert_eq!(
            committed.arrangement.audio_clips[0].track_id,
            committed.arrangement.tracks[0].id
        );
    }

    #[test]
    fn adding_a_midi_asset_creates_the_track_and_replaces_transient_ids() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let asset_id = mint_asset_id();

        let committed = core
            .application(&storage)
            .add_midi_asset_clip(crate::application::MidiAssetClipPlacement {
                asset_id,
                name: "Pattern".into(),
                start_tick: None,
                track_id: None,
                duration_ticks: 960,
                notes: vec![MidiNote {
                    id: "adapter:temporary".into(),
                    note: 60,
                    start_tick: TimelineTick(0),
                    duration_ticks: 480,
                    velocity: 100,
                    channel: 1,
                }],
                events: Vec::new(),
            })
            .unwrap();

        assert_eq!(committed.arrangement.tracks[0].kind, TrackKind::Instrument);
        assert_eq!(committed.arrangement.midi_clips.len(), 1);
        assert_ne!(
            committed.arrangement.midi_clips[0].notes[0].id,
            "adapter:temporary"
        );
    }

    #[test]
    fn creating_an_empty_midi_clip_is_core_owned_and_requires_an_instrument_track() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let application = core.application(&storage);
        let audio = application.add_track("Audio", TrackKind::Audio).unwrap();
        let audio_id = audio.arrangement.tracks[0].id.clone();

        let error = application
            .create_midi_clip(&audio_id, TimelineTick(0), 960, None)
            .unwrap_err();
        assert!(error.to_string().contains("requires an Instrument Track"));

        let instrument = application
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let instrument_id = instrument.arrangement.tracks[1].id.clone();
        let committed = application
            .create_midi_clip(
                &instrument_id,
                TimelineTick(480),
                0,
                Some("  Lead  ".into()),
            )
            .unwrap();
        let clip = &committed.arrangement.midi_clips[0];

        assert!(clip.id.starts_with("midi-clip:"));
        assert_eq!(clip.name, "Lead");
        assert_eq!(clip.track_id, instrument_id);
        assert_eq!(clip.start_tick, TimelineTick(480));
        assert_eq!(clip.duration_ticks, 1);
        assert!(clip.asset_id.is_none());
        assert!(clip.notes.is_empty());
        assert!(clip.events.is_empty());
    }

    #[test]
    fn batch_midi_note_insert_and_remove_each_have_one_undoable_commit() {
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
            .create_midi_clip(&track_id, TimelineTick(0), 1_920, None)
            .unwrap();
        let before_insert_saves = storage.sessions.lock().unwrap().len();

        let inserted = application
            .insert_midi_notes(
                "midi-clip:missing",
                vec![crate::application::MidiNoteInput {
                    pitch: 60,
                    start_tick: TimelineTick(0),
                    duration_ticks: 480,
                    velocity: 96,
                    channel: 1,
                }],
            )
            .unwrap_err();
        assert!(inserted.to_string().contains("not found"));

        let clip_id = core.snapshot().unwrap().session.arrangement.midi_clips[0]
            .id
            .clone();
        let inserted = application
            .insert_midi_notes(
                &clip_id,
                vec![
                    crate::application::MidiNoteInput {
                        pitch: 60,
                        start_tick: TimelineTick(0),
                        duration_ticks: 480,
                        velocity: 96,
                        channel: 1,
                    },
                    crate::application::MidiNoteInput {
                        pitch: 64,
                        start_tick: TimelineTick(480),
                        duration_ticks: 0,
                        velocity: 100,
                        channel: 1,
                    },
                ],
            )
            .unwrap();
        assert_eq!(
            storage.sessions.lock().unwrap().len(),
            before_insert_saves + 1
        );
        assert_eq!(inserted.arrangement.midi_clips[0].notes.len(), 2);
        assert_eq!(
            inserted.arrangement.midi_clips[0].notes[1].duration_ticks,
            1
        );
        assert_ne!(
            inserted.arrangement.midi_clips[0].notes[0].id,
            inserted.arrangement.midi_clips[0].notes[1].id
        );

        let existing_note_id = inserted.arrangement.midi_clips[0].notes[0].id.clone();
        let empty_selection = application
            .duplicate_midi_notes(&clip_id, Vec::new(), 1_920)
            .unwrap_err();
        assert!(empty_selection.to_string().contains("no midi notes"));

        let missing_note = application
            .duplicate_midi_notes(
                &clip_id,
                vec![existing_note_id.clone(), "note:missing".into()],
                1_920,
            )
            .unwrap_err();
        assert!(missing_note.to_string().contains("not found"));

        let duplicate_selection = application
            .duplicate_midi_notes(
                &clip_id,
                vec![existing_note_id.clone(), existing_note_id],
                1_920,
            )
            .unwrap_err();
        assert!(duplicate_selection.to_string().contains("duplicate"));

        let note_ids = inserted.arrangement.midi_clips[0]
            .notes
            .iter()
            .map(|note| note.id.clone())
            .collect::<Vec<_>>();
        let undone_insert = core.undo(&storage).unwrap();
        assert!(undone_insert.arrangement.midi_clips[0].notes.is_empty());
        let redone_insert = core.redo(&storage).unwrap();
        assert_eq!(redone_insert.arrangement.midi_clips[0].notes.len(), 2);

        let before_remove_saves = storage.sessions.lock().unwrap().len();
        let removed = application.remove_midi_notes(&clip_id, note_ids).unwrap();
        assert!(removed.arrangement.midi_clips[0].notes.is_empty());
        assert_eq!(
            storage.sessions.lock().unwrap().len(),
            before_remove_saves + 1
        );
        let undone_remove = core.undo(&storage).unwrap();
        assert_eq!(undone_remove.arrangement.midi_clips[0].notes.len(), 2);
        let redone_remove = core.redo(&storage).unwrap();
        assert!(redone_remove.arrangement.midi_clips[0].notes.is_empty());
    }

    #[test]
    fn midi_note_duplicate_and_paste_extend_the_clip_as_one_edit() {
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
        let created = application
            .create_midi_clip(&track_id, TimelineTick(0), 1_920, None)
            .unwrap();
        let clip_id = created.arrangement.midi_clips[0].id.clone();
        let inserted = application
            .insert_midi_notes(
                &clip_id,
                vec![crate::application::MidiNoteInput {
                    pitch: 60,
                    start_tick: TimelineTick(0),
                    duration_ticks: 1_920,
                    velocity: 96,
                    channel: 1,
                }],
            )
            .unwrap();
        let note_id = inserted.arrangement.midi_clips[0].notes[0].id.clone();

        let duplicated = application
            .duplicate_midi_notes(&clip_id, vec![note_id], 1_920)
            .unwrap();
        let duplicated_clip = &duplicated.arrangement.midi_clips[0];
        assert_eq!(duplicated_clip.duration_ticks, 3_840);
        assert_eq!(duplicated_clip.notes.len(), 2);
        assert_eq!(duplicated_clip.notes[1].start_tick, TimelineTick(1_920));
        assert_eq!(duplicated_clip.notes[1].duration_ticks, 1_920);

        let undone_duplicate = core.undo(&storage).unwrap();
        let undone_clip = &undone_duplicate.arrangement.midi_clips[0];
        assert_eq!(undone_clip.duration_ticks, 1_920);
        assert_eq!(undone_clip.notes.len(), 1);

        let pasted = application
            .insert_midi_notes(
                &clip_id,
                vec![crate::application::MidiNoteInput {
                    pitch: 64,
                    start_tick: TimelineTick(1_920),
                    duration_ticks: 480,
                    velocity: 100,
                    channel: 1,
                }],
            )
            .unwrap();
        let pasted_clip = &pasted.arrangement.midi_clips[0];
        assert_eq!(pasted_clip.duration_ticks, 2_400);
        assert_eq!(pasted_clip.notes.len(), 2);

        let undone_paste = core.undo(&storage).unwrap();
        assert_eq!(undone_paste.arrangement.midi_clips[0].duration_ticks, 1_920);
        assert_eq!(undone_paste.arrangement.midi_clips[0].notes.len(), 1);
    }

    #[test]
    fn prepared_plugin_commit_uses_the_runtime_validated_candidate() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        let application = core.application(&storage);
        let with_track = application
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = with_track.arrangement.tracks[0].id.clone();
        let prepared = application
            .prepare_track_instrument(&track_id, "Synth".into(), "Synth.vst3".into())
            .unwrap();
        let validated_arrangement = prepared.session().arrangement.clone();

        let committed = application.commit_prepared(prepared).unwrap();

        assert_eq!(committed.arrangement, validated_arrangement);
    }

    #[test]
    fn core_executes_a_complete_daw_edit_history_and_save_flow() {
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

        application
            .trim_audio_clip(
                "clip:1",
                TimelineTick(960),
                FrameRange {
                    start: 0,
                    end: 24_000,
                },
                48_000,
            )
            .unwrap();
        application
            .split_audio_clip("clip:1", TimelineTick(1_440))
            .unwrap();

        let with_midi_track = application
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let midi_track_id = with_midi_track.arrangement.tracks[1].id.clone();
        application
            .add_midi_clip(MidiClip {
                id: "midi:1".into(),
                name: "Pattern".into(),
                track_id: midi_track_id,
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

        application
            .add_track_effect(
                &track_id,
                RackDevice {
                    id: "device:gain".into(),
                    name: "Gain".into(),
                    kind: DeviceKind::Utility,
                    path: None,
                    bypassed: false,
                    gain_db: 0.0,
                    parameter_values: Vec::new(),
                    state_data: None,
                    disabled_placeholder: false,
                },
            )
            .unwrap();

        let undone = core.undo(&storage).unwrap();
        let redone = core.redo(&storage).unwrap();

        assert_eq!(with_clip.arrangement.audio_clips.len(), 1);
        assert_eq!(
            moved.arrangement.audio_clips[0].start_tick,
            TimelineTick(960)
        );
        assert!(undone.arrangement.tracks[0].rack.devices.is_empty());
        assert_eq!(redone.arrangement.tracks[0].rack.devices.len(), 1);
        assert_eq!(redone.arrangement.audio_clips.len(), 2);
        assert_eq!(redone.arrangement.midi_clips.len(), 1);
        assert!(storage.sessions.lock().unwrap().len() >= 10);

        let duplicated = application.duplicate_track(&track_id).unwrap();
        assert_eq!(duplicated.arrangement.tracks.len(), 3);
        assert_eq!(duplicated.arrangement.audio_clips.len(), 4);

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
    fn application_facade_owns_plugin_state_edits() {
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
    fn project_activation_swaps_canonical_state_without_persisting_or_retaining_history() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );
        core.application(&storage)
            .add_track("Old project", TrackKind::Audio)
            .unwrap();
        let saved_before_activation = storage.sessions.lock().unwrap().len();

        let mut next = CreativeSession::new(2);
        next.project_name = Some("Next project".into());
        let activated = core.activate_session(next).unwrap();

        assert_eq!(activated.project_name.as_deref(), Some("Next project"));
        assert_eq!(core.canonical_state().unwrap().sequence, 2);
        assert_eq!(
            core.canonical_state().unwrap().session.session_id,
            "scratch-2"
        );
        assert_eq!(core.history_state().unwrap(), HistoryState::default());
        assert_eq!(
            storage.sessions.lock().unwrap().len(),
            saved_before_activation
        );
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
    fn long_running_operation_merges_after_a_newer_commit_without_losing_it() {
        let storage = Arc::new(MemoryStorage::default());
        let core = Arc::new(AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        ));
        let base = core.snapshot().unwrap().session;
        let mut candidate = base.clone();
        candidate.project_name = Some("operation a".into());
        let release = Arc::new(Barrier::new(2));
        let operation = {
            let core = Arc::clone(&core);
            let storage = Arc::clone(&storage);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                release.wait();
                core.commit_merged(&*storage, &base, candidate, |current, _, candidate| {
                    let mut merged = current.clone();
                    merged.project_name = candidate.project_name;
                    merged
                })
                .unwrap()
            })
        };

        core.application(&*storage)
            .add_track("operation b", TrackKind::Audio)
            .unwrap();
        release.wait();
        let committed = operation.join().unwrap();

        assert_eq!(committed.project_name.as_deref(), Some("operation a"));
        assert_eq!(committed.arrangement.tracks[0].name, "operation b");
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
    fn canonical_state_keeps_history_with_the_same_revision() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            NoopAudio,
            false,
            false,
        );

        let initial = core.canonical_state().unwrap();
        assert_eq!(initial.sequence, 0);
        assert_eq!(initial.history, HistoryState::default());

        core.application(&storage)
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let committed = core.canonical_state().unwrap();
        assert_eq!(committed.sequence, 1);
        assert!(committed.history.can_undo);
        assert_eq!(committed.session.arrangement.tracks.len(), 1);

        core.undo(&storage).unwrap();
        let undone = core.canonical_state().unwrap();
        assert_eq!(undone.sequence, 2);
        assert!(!undone.history.can_undo);
        assert!(undone.history.can_redo);
    }

    #[test]
    fn stale_prepared_commit_returns_typed_conflict_without_mutating_state() {
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
        let prepared = application
            .prepare_track_instrument(&track_id, "Synth".into(), "Synth.vst3".into())
            .unwrap();
        application
            .update_session_settings(crate::application::SessionSettingsPatch {
                project_name: Some(Some("Newer".into())),
                ..Default::default()
            })
            .unwrap();
        let current = core.canonical_state().unwrap();
        let saved = storage.sessions.lock().unwrap().len();

        let error = application.commit_prepared(prepared).unwrap_err();

        assert_eq!(
            error,
            ApplicationError::Conflict {
                expected_sequence: 1,
                current_sequence: 2,
            }
        );
        assert_eq!(core.canonical_state().unwrap(), current);
        assert_eq!(storage.sessions.lock().unwrap().len(), saved);
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

        let application = core.application(&storage);
        application
            .update_session_settings(crate::application::SessionSettingsPatch {
                project_name: Some(Some("Projected".into())),
                ..Default::default()
            })
            .unwrap();
        application.project_current(&projection).unwrap();

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

        let application = core.application(&storage);
        application
            .update_session_settings(crate::application::SessionSettingsPatch {
                project_name: Some(Some("Durable".into())),
                ..Default::default()
            })
            .unwrap();
        let error = application.project_current(&FailingProjection).unwrap_err();

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

    #[test]
    fn add_track_leaves_coloring_to_the_ui() {
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

        assert_eq!(committed.arrangement.tracks[0].color, None);
    }

    #[test]
    fn transform_midi_notes_touches_only_the_selected_notes() {
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
        application
            .add_midi_note("midi:1", TimelineTick(0), 60, 480, 100, 1)
            .unwrap();
        let with_second = application
            .add_midi_note("midi:1", TimelineTick(480), 64, 240, 40, 1)
            .unwrap();
        let ids: Vec<String> = with_second.arrangement.midi_clips[0]
            .notes
            .iter()
            .map(|note| note.id.clone())
            .collect();

        let transformed = application
            .transform_midi_notes("midi:1", vec![ids[0].clone()], 2, -10)
            .unwrap();

        let notes = &transformed.arrangement.midi_clips[0].notes;
        assert_eq!(
            (notes[0].note, notes[0].velocity),
            (62, 90),
            "the selected note is transposed and offset"
        );
        assert_eq!(
            (notes[1].note, notes[1].velocity),
            (64, 40),
            "the unselected note keeps its pitch and velocity"
        );
    }

    #[test]
    fn transform_midi_notes_without_ids_transforms_every_note() {
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
        application
            .add_midi_note("midi:1", TimelineTick(0), 60, 480, 100, 1)
            .unwrap();
        application
            .add_midi_note("midi:1", TimelineTick(480), 64, 240, 40, 1)
            .unwrap();

        let transformed = application
            .transform_midi_notes("midi:1", Vec::new(), -3, 5)
            .unwrap();

        let notes = &transformed.arrangement.midi_clips[0].notes;
        assert_eq!((notes[0].note, notes[0].velocity), (57, 105));
        assert_eq!((notes[1].note, notes[1].velocity), (61, 45));
    }

    #[test]
    fn transform_midi_notes_clamps_pitch_and_velocity() {
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
        application
            .add_midi_note("midi:1", TimelineTick(0), 120, 480, 20, 1)
            .unwrap();

        let transformed = application
            .transform_midi_notes("midi:1", Vec::new(), 30, -200)
            .unwrap();

        let notes = &transformed.arrangement.midi_clips[0].notes;
        assert_eq!(notes[0].note, 127);
        assert_eq!(notes[0].velocity, 0);
    }

    #[test]
    fn transform_midi_notes_rejects_unknown_note_ids() {
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
        application
            .add_midi_note("midi:1", TimelineTick(0), 60, 480, 100, 1)
            .unwrap();

        let error = application
            .transform_midi_notes("midi:1", vec!["midi-note:missing".into()], 1, 0)
            .unwrap_err();

        assert_eq!(
            error,
            ApplicationError::InvalidCommand(
                "invalid clip: midi note 'midi-note:missing' is not registered".into()
            )
        );
    }
}
