use crate::application::history::History;
use crate::domain::CreativeSession;
use crate::errors::ApplicationError;
use crate::ports::SessionStorage;
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
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
    /// Identity of the project owning this revision.
    pub project_id: String,
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
    /// Builds a validated candidate from a consistent canonical snapshot.
    ///
    /// # Errors
    /// Returns an error when editing or session validation fails.
    pub fn from_snapshot(
        snapshot: &CanonicalSnapshot,
        edit: impl FnOnce(&mut CreativeSession) -> Result<(), ApplicationError>,
    ) -> Result<Self, ApplicationError> {
        let mut session = snapshot.session.clone();
        edit(&mut session)?;
        let session = session
            .validate_and_normalize()
            .map_err(ApplicationError::InvalidSession)?;
        Ok(Self::new(session, snapshot.sequence))
    }

    pub(crate) fn new(session: CreativeSession, expected_sequence: u64) -> Self {
        Self {
            session,
            expected_sequence,
        }
    }

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

/// Read-only history capabilities exposed to a host UI.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistoryState {
    /// Whether an undo operation can be performed.
    pub can_undo: bool,
    /// Whether a redo operation can be performed.
    pub can_redo: bool,
}

/// Canonical state and history for one project.
pub struct AppCore {
    project_id: String,
    snapshot: Arc<CanonicalSnapshot>,
    history: History,
}

impl AppCore {
    /// Creates canonical state for one project at the supplied sequence.
    pub fn new(project_id: String, session: CreativeSession, sequence: u64) -> Self {
        Self {
            project_id,
            snapshot: Arc::new(CanonicalSnapshot { session, sequence }),
            history: History::default(),
        }
    }

    /// Returns the identity of this project.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Captures the current committed session and sequence.
    pub fn snapshot(&self) -> Arc<CanonicalSnapshot> {
        Arc::clone(&self.snapshot)
    }

    /// Captures canonical state and history at the same revision.
    pub fn canonical_state(&self) -> CanonicalState {
        CanonicalState {
            project_id: self.project_id.clone(),
            session: self.snapshot.session.clone(),
            sequence: self.snapshot.sequence,
            history: self.history_state(),
        }
    }

    pub(crate) fn commit<S: SessionStorage + ?Sized>(
        &mut self,
        storage: &S,
        edit: impl FnOnce(&mut CreativeSession) -> Result<(), ApplicationError>,
    ) -> Result<CreativeSession, ApplicationError> {
        let mut candidate = self.snapshot.session.clone();
        edit(&mut candidate)?;
        self.commit_candidate(storage, candidate)
    }

    pub(crate) fn commit_prepared<S: SessionStorage + ?Sized>(
        &mut self,
        storage: &S,
        prepared: PreparedSession,
    ) -> Result<CreativeSession, ApplicationError> {
        if self.snapshot.sequence != prepared.expected_sequence {
            return Err(ApplicationError::Conflict {
                expected_sequence: prepared.expected_sequence,
                current_sequence: self.snapshot.sequence,
            });
        }
        self.commit_candidate(storage, prepared.session)
    }

    pub(crate) fn commit_candidate<S: SessionStorage + ?Sized>(
        &mut self,
        storage: &S,
        candidate: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        let mut candidate = candidate
            .validate_and_normalize()
            .map_err(ApplicationError::InvalidSession)?;
        if candidate == self.snapshot.session {
            return Ok(candidate);
        }
        candidate.updated_at_ms =
            next_update_timestamp(self.snapshot.session.updated_at_ms, candidate.updated_at_ms);
        storage
            .save(&candidate)
            .map_err(application_error_from_port)?;
        self.history.record(self.snapshot.session.clone());
        self.exchange(candidate.clone());
        Ok(candidate)
    }

    pub(crate) fn commit_merged<S: SessionStorage + ?Sized>(
        &mut self,
        storage: &S,
        base: &CreativeSession,
        candidate: CreativeSession,
        merge: impl FnOnce(&CreativeSession, &CreativeSession, CreativeSession) -> CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        let candidate = merge(&self.snapshot.session, base, candidate);
        self.commit_candidate(storage, candidate)
    }

    pub(crate) fn undo<S: SessionStorage + ?Sized>(
        &mut self,
        storage: &S,
    ) -> Result<CreativeSession, ApplicationError> {
        self.restore_history_entry(storage, true)
    }
    pub(crate) fn redo<S: SessionStorage + ?Sized>(
        &mut self,
        storage: &S,
    ) -> Result<CreativeSession, ApplicationError> {
        self.restore_history_entry(storage, false)
    }
    pub(crate) fn history_state(&self) -> HistoryState {
        HistoryState {
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
        }
    }

    fn restore_history_entry<S: SessionStorage + ?Sized>(
        &mut self,
        storage: &S,
        undo: bool,
    ) -> Result<CreativeSession, ApplicationError> {
        let entry = if undo {
            self.history.take_undo()
        } else {
            self.history.take_redo()
        }
        .ok_or(ApplicationError::HistoryEmpty)?;
        let result = entry
            .clone()
            .validate_and_normalize()
            .map_err(ApplicationError::InvalidSession)
            .and_then(|mut target| {
                target.updated_at_ms = next_update_timestamp(
                    self.snapshot.session.updated_at_ms,
                    target.updated_at_ms,
                );
                storage.save(&target).map_err(application_error_from_port)?;
                Ok(target)
            });
        let target = match result {
            Ok(target) => target,
            Err(error) => {
                if undo {
                    self.history.push_undo(entry);
                } else {
                    self.history.push_redo(entry);
                }
                return Err(error);
            }
        };
        let current = self.snapshot.session.clone();
        if undo {
            self.history.push_redo(current);
        } else {
            self.history.push_undo(current);
        }
        self.exchange(target.clone());
        Ok(target)
    }

    fn exchange(&mut self, session: CreativeSession) {
        self.snapshot = Arc::new(CanonicalSnapshot {
            session,
            sequence: self.snapshot.sequence + 1,
        });
    }
}

fn next_update_timestamp(previous: u64, candidate: u64) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(previous);
    now.max(candidate).max(previous.saturating_add(1))
}

fn application_error_from_port(error: crate::ports::PortError) -> ApplicationError {
    match error {
        crate::ports::PortError::Storage(message) => ApplicationError::Storage(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::asset::mint_asset_id;
    use crate::domain::{
        AudioClip, AudioClipMove, FrameRange, MidiClip, MidiNote, TimelineTick, TrackInstrument,
        TrackKind,
    };
    use crate::ports::{PortError, SessionStorage};
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

    #[test]
    fn commit_persists_and_records_history() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);

        let committed = core
            .application(&storage)
            .add_track_with_created_ids("Main", TrackKind::Audio)
            .unwrap();

        assert_eq!(committed.session.arrangement.tracks.len(), 1);
        assert_eq!(storage.sessions.lock().unwrap().len(), 1);
        assert!(core.history_state().can_undo);
    }

    #[test]
    fn add_track_normalizes_the_canonical_name() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);

        let committed = core
            .application(&storage)
            .add_track_with_created_ids(format!("  {}  ", "a".repeat(100)), TrackKind::Audio)
            .unwrap();

        assert_eq!(committed.session.arrangement.tracks[0].name, "a".repeat(80));
    }

    #[test]
    fn adding_an_audio_asset_creates_the_missing_audio_track() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let asset_id = mint_asset_id();

        let committed = core
            .application(&storage)
            .add_audio_asset_clip_with_created_ids(
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

        assert_eq!(committed.session.arrangement.tracks.len(), 1);
        assert_eq!(
            committed.session.arrangement.tracks[0].kind,
            TrackKind::Audio
        );
        assert_eq!(committed.session.arrangement.audio_clips.len(), 1);
        assert_eq!(
            committed.session.arrangement.audio_clips[0].track_id,
            committed.session.arrangement.tracks[0].id
        );
    }

    #[test]
    fn adding_a_midi_asset_creates_the_track_and_replaces_transient_ids() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let asset_id = mint_asset_id();

        let committed = core
            .application(&storage)
            .add_midi_asset_clip_with_created_ids(crate::application::MidiAssetClipPlacement {
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

        assert_eq!(
            committed.session.arrangement.tracks[0].kind,
            TrackKind::Instrument
        );
        assert_eq!(committed.session.arrangement.midi_clips.len(), 1);
        assert_ne!(
            committed.session.arrangement.midi_clips[0].notes[0].id,
            "adapter:temporary"
        );
    }

    #[test]
    fn creating_an_empty_midi_clip_is_core_owned_and_requires_an_instrument_track() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let audio = core
            .application(&storage)
            .add_track_with_created_ids("Audio", TrackKind::Audio)
            .unwrap();
        let audio_id = audio.session.arrangement.tracks[0].id.clone();

        let error = core
            .application(&storage)
            .create_midi_clip_with_created_ids(&audio_id, TimelineTick(0), 960, None)
            .unwrap_err();
        assert!(error.to_string().contains("requires an Instrument Track"));

        let instrument = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let instrument_id = instrument.session.arrangement.tracks[1].id.clone();
        let committed = core
            .application(&storage)
            .create_midi_clip_with_created_ids(
                &instrument_id,
                TimelineTick(480),
                960,
                Some("  Lead  ".into()),
            )
            .unwrap();
        let clip = &committed.session.arrangement.midi_clips[0];

        assert!(clip.id.starts_with("midi-clip:"));
        assert_eq!(clip.name, "Lead");
        assert_eq!(clip.track_id, instrument_id);
        assert_eq!(clip.start_tick, TimelineTick(480));
        assert_eq!(clip.duration_ticks, 960);
        assert!(clip.asset_id.is_none());
        assert!(clip.notes.is_empty());
        assert!(clip.events.is_empty());
    }

    #[test]
    fn batch_midi_note_insert_and_remove_each_have_one_undoable_commit() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        core.application(&storage)
            .create_midi_clip_with_created_ids(&track_id, TimelineTick(0), 1_920, None)
            .unwrap();
        let before_insert_saves = storage.sessions.lock().unwrap().len();

        let inserted = core
            .application(&storage)
            .insert_midi_notes_with_created_ids(
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

        let clip_id = core.snapshot().session.clone().arrangement.midi_clips[0]
            .id
            .clone();
        let inserted = core
            .application(&storage)
            .insert_midi_notes_with_created_ids(
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
                        duration_ticks: 480,
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
        assert_eq!(inserted.session.arrangement.midi_clips[0].notes.len(), 2);
        assert_eq!(
            inserted.session.arrangement.midi_clips[0].notes[1].duration_ticks,
            480
        );
        assert_ne!(
            inserted.session.arrangement.midi_clips[0].notes[0].id,
            inserted.session.arrangement.midi_clips[0].notes[1].id
        );

        let existing_note_id = inserted.session.arrangement.midi_clips[0].notes[0]
            .id
            .clone();
        let empty_selection = core
            .application(&storage)
            .duplicate_midi_notes_with_created_ids(&clip_id, Vec::new(), 1_920)
            .unwrap_err();
        assert!(empty_selection.to_string().contains("no midi notes"));

        let missing_note = core
            .application(&storage)
            .duplicate_midi_notes_with_created_ids(
                &clip_id,
                vec![existing_note_id.clone(), "note:missing".into()],
                1_920,
            )
            .unwrap_err();
        assert!(missing_note.to_string().contains("not found"));

        let duplicate_selection = core
            .application(&storage)
            .duplicate_midi_notes_with_created_ids(
                &clip_id,
                vec![existing_note_id.clone(), existing_note_id],
                1_920,
            )
            .unwrap_err();
        assert!(duplicate_selection.to_string().contains("duplicate"));

        let note_ids = inserted.session.arrangement.midi_clips[0]
            .notes
            .iter()
            .map(|note| note.id.clone())
            .collect::<Vec<_>>();
        let undone_insert = core.undo(&storage).unwrap();
        assert!(undone_insert.arrangement.midi_clips[0].notes.is_empty());
        let redone_insert = core.redo(&storage).unwrap();
        assert_eq!(redone_insert.arrangement.midi_clips[0].notes.len(), 2);

        let before_remove_saves = storage.sessions.lock().unwrap().len();
        let removed = core
            .application(&storage)
            .remove_midi_notes(&clip_id, note_ids)
            .unwrap();
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
    fn midi_note_duplicate_and_paste_reject_out_of_range_notes() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        let created = core
            .application(&storage)
            .create_midi_clip_with_created_ids(&track_id, TimelineTick(0), 1_920, None)
            .unwrap();
        let clip_id = created.session.arrangement.midi_clips[0].id.clone();
        let inserted = core
            .application(&storage)
            .insert_midi_notes_with_created_ids(
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
        let note_id = inserted.session.arrangement.midi_clips[0].notes[0]
            .id
            .clone();

        let duplicated = core
            .application(&storage)
            .duplicate_midi_notes_with_created_ids(&clip_id, vec![note_id], 1_920)
            .unwrap_err();
        assert!(duplicated.to_string().contains("invalid note"));
        let unchanged = core.snapshot();
        assert_eq!(
            unchanged.session.arrangement.midi_clips[0].duration_ticks,
            1_920
        );
        assert_eq!(unchanged.session.arrangement.midi_clips[0].notes.len(), 1);

        let pasted = core
            .application(&storage)
            .insert_midi_notes_with_created_ids(
                &clip_id,
                vec![crate::application::MidiNoteInput {
                    pitch: 64,
                    start_tick: TimelineTick(1_920),
                    duration_ticks: 480,
                    velocity: 100,
                    channel: 1,
                }],
            )
            .unwrap_err();
        assert!(pasted.to_string().contains("invalid note"));
        let unchanged = core.snapshot();
        assert_eq!(
            unchanged.session.arrangement.midi_clips[0].duration_ticks,
            1_920
        );
        assert_eq!(unchanged.session.arrangement.midi_clips[0].notes.len(), 1);
    }

    #[test]
    fn prepared_plugin_commit_uses_the_runtime_validated_candidate() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let with_track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = with_track.session.arrangement.tracks[0].id.clone();
        let prepared = core
            .application(&storage)
            .prepare_track_instrument(
                &track_id,
                TrackInstrument::vst3(
                    "device:instrument".into(),
                    "Synth".into(),
                    "Synth.vst3".into(),
                )
                .unwrap(),
            )
            .unwrap();
        let validated_arrangement = prepared.session().arrangement.clone();

        let committed = core
            .application(&storage)
            .commit_prepared(prepared)
            .unwrap();

        assert_eq!(committed.arrangement, validated_arrangement);
    }

    #[test]
    fn core_executes_a_complete_daw_edit_history_and_save_flow() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let with_track = core
            .application(&storage)
            .add_track_with_created_ids("Audio", TrackKind::Audio)
            .unwrap();
        let track_id = with_track.session.arrangement.tracks[0].id.clone();
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

        let with_clip = core
            .application(&storage)
            .add_audio_clip(clip, |_| true)
            .unwrap();
        let moved = core
            .application(&storage)
            .move_audio_clips(vec![AudioClipMove {
                clip_id: "clip:1".into(),
                start_tick: TimelineTick(960),
                track_id: track_id.clone(),
            }])
            .unwrap();

        core.application(&storage)
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
        core.application(&storage)
            .split_audio_clip_with_created_ids("clip:1", TimelineTick(1_440))
            .unwrap();

        let with_midi_track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let midi_track_id = with_midi_track.session.arrangement.tracks[1].id.clone();
        core.application(&storage)
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

        core.application(&storage)
            .add_track_effect_with_created_ids(&track_id, "Gain".into(), "builtin:gain".into())
            .unwrap();

        let undone = core.undo(&storage).unwrap();
        let redone = core.redo(&storage).unwrap();

        assert_eq!(with_clip.arrangement.audio_clips.len(), 1);
        assert_eq!(
            moved.arrangement.audio_clips[0].start_tick,
            TimelineTick(960)
        );
        assert!(undone.arrangement.tracks[0].effects.is_empty());
        assert_eq!(redone.arrangement.tracks[0].effects.len(), 1);
        assert_eq!(redone.arrangement.audio_clips.len(), 2);
        assert_eq!(redone.arrangement.midi_clips.len(), 1);
        assert!(storage.sessions.lock().unwrap().len() >= 10);

        let duplicated = core
            .application(&storage)
            .duplicate_track_with_created_ids(&track_id)
            .unwrap();
        assert_eq!(duplicated.session.arrangement.tracks.len(), 3);
        assert_eq!(duplicated.session.arrangement.audio_clips.len(), 4);

        let marked = core
            .application(&storage)
            .add_marker_with_created_ids(TimelineTick(1_920), "  Chorus  ".into())
            .unwrap();
        assert_eq!(marked.session.arrangement.markers[0].name, "Chorus");

        let settings = core
            .application(&storage)
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
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);

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
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        core.application(&storage)
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

        let with_note = core
            .application(&storage)
            .add_midi_note_with_created_ids("midi:1", TimelineTick(0), 60, 480, 100, 1)
            .unwrap();
        let note_id = with_note.session.arrangement.midi_clips[0].notes[0]
            .id
            .clone();
        let updated = core
            .application(&storage)
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

        let duplicated = core
            .application(&storage)
            .duplicate_midi_notes_with_created_ids("midi:1", vec![note_id.clone()], 480)
            .unwrap();
        assert_eq!(duplicated.session.arrangement.midi_clips[0].notes.len(), 2);
        let removed = core
            .application(&storage)
            .remove_midi_note("midi:1", &note_id)
            .unwrap();
        assert_eq!(removed.arrangement.midi_clips[0].notes.len(), 1);
    }

    #[test]
    fn application_facade_owns_plugin_state_edits() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);

        let track = core
            .application(&storage)
            .add_track_with_created_ids("Synth", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        let instrument =
            TrackInstrument::vst3("device:synth".into(), "Synth".into(), "Synth.vst3".into())
                .unwrap();
        core.application(&storage)
            .set_track_instrument(&track_id, Some(instrument))
            .unwrap();
        let with_state = core
            .application(&storage)
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
        let vst3 = instrument.as_vst3().unwrap();
        assert_eq!(vst3.parameter_values, [0.25]);
        assert_eq!(vst3.state_data.as_deref(), Some("state"));
        assert!(instrument.bypassed);
        let disabled = core
            .application(&storage)
            .disable_missing_plugin("device:synth")
            .unwrap();
        assert!(
            disabled.arrangement.tracks[0]
                .instrument
                .as_ref()
                .unwrap()
                .as_vst3()
                .unwrap()
                .disabled_placeholder
        );

        let replacement =
            TrackInstrument::vst3("device:synth".into(), "Other".into(), "Other.vst3".into())
                .unwrap();
        let replaced = core
            .application(&storage)
            .replace_track_instrument("device:synth", replacement)
            .unwrap();
        assert_eq!(
            replaced.arrangement.tracks[0]
                .instrument
                .as_ref()
                .unwrap()
                .as_vst3()
                .unwrap()
                .path,
            "Other.vst3"
        );

        let built_in_track = core
            .application(&storage)
            .add_track_with_created_ids("Built-in", TrackKind::Instrument)
            .unwrap();
        let built_in_track_id = built_in_track.session.arrangement.tracks[1].id.clone();
        core.application(&storage)
            .set_track_instrument(
                &built_in_track_id,
                Some(
                    TrackInstrument::built_in(
                        "device:built-in".into(),
                        "Clean Sub Bass".into(),
                        "01-clean-sub-bass".into(),
                        r#"{"schemaVersion":1}"#.into(),
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        assert!(
            core.application(&storage)
                .disable_missing_plugin("device:built-in")
                .is_err()
        );
        assert!(
            core.application(&storage)
                .replace_track_instrument(
                    "device:built-in",
                    TrackInstrument::vst3(
                        "device:built-in".into(),
                        "Other".into(),
                        "Other.vst3".into(),
                    )
                    .unwrap(),
                )
                .is_err()
        );
    }

    #[test]
    fn undo_and_redo_are_core_owned_and_persisted() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        core.application(&storage)
            .add_track_with_created_ids("Main", TrackKind::Audio)
            .unwrap();

        let undone = core.undo(&storage).unwrap();
        assert!(undone.arrangement.tracks.is_empty());
        assert!(core.history_state().can_redo);

        let redone = core.redo(&storage).unwrap();
        assert_eq!(redone.arrangement.tracks[0].name, "Main");
        assert_eq!(storage.sessions.lock().unwrap().len(), 3);
    }

    #[test]
    fn stale_merge_uses_latest_canonical_state() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let base = core.snapshot().session.clone();
        core.application(&storage)
            .add_track_with_created_ids("Current", TrackKind::Audio)
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
        let core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let snapshot = core.snapshot();
        assert_eq!(snapshot.sequence, 0);
        assert!(!snapshot.session.session_id.is_empty());
    }

    #[test]
    fn canonical_state_keeps_history_with_the_same_revision() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);

        let initial = core.canonical_state();
        assert_eq!(initial.sequence, 0);
        assert_eq!(initial.history, HistoryState::default());

        core.application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let committed = core.canonical_state();
        assert_eq!(committed.sequence, 1);
        assert!(committed.history.can_undo);
        assert_eq!(committed.session.arrangement.tracks.len(), 1);

        core.undo(&storage).unwrap();
        let undone = core.canonical_state();
        assert_eq!(undone.sequence, 2);
        assert!(!undone.history.can_undo);
        assert!(undone.history.can_redo);
    }

    #[test]
    fn stale_prepared_commit_returns_typed_conflict_without_mutating_state() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        let prepared = core
            .application(&storage)
            .prepare_track_instrument(
                &track_id,
                TrackInstrument::vst3(
                    "device:instrument".into(),
                    "Synth".into(),
                    "Synth.vst3".into(),
                )
                .unwrap(),
            )
            .unwrap();
        core.application(&storage)
            .update_session_settings(crate::application::SessionSettingsPatch {
                project_name: Some(Some("Newer".into())),
                ..Default::default()
            })
            .unwrap();
        let current = core.canonical_state();
        let saved = storage.sessions.lock().unwrap().len();

        let error = core
            .application(&storage)
            .commit_prepared(prepared)
            .unwrap_err();

        assert_eq!(
            error,
            ApplicationError::Conflict {
                expected_sequence: 1,
                current_sequence: 2,
            }
        );
        assert_eq!(core.canonical_state(), current);
        assert_eq!(storage.sessions.lock().unwrap().len(), saved);
    }

    #[test]
    fn persistence_failure_leaves_canonical_state_and_history_unchanged() {
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let before = core.snapshot();

        let error = core
            .application(&FailingStorage)
            .add_track_with_created_ids("Main", TrackKind::Audio)
            .unwrap_err();

        assert_eq!(error, ApplicationError::Storage("disk full".into()));
        let after = core.snapshot();
        assert_eq!(after.session, before.session);
        assert_eq!(after.sequence, before.sequence);
        assert_eq!(core.history_state(), HistoryState::default());
    }

    #[test]
    fn add_track_leaves_coloring_to_the_ui() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);

        let committed = core
            .application(&storage)
            .add_track_with_created_ids("Main", TrackKind::Audio)
            .unwrap();

        assert_eq!(committed.session.arrangement.tracks[0].color, None);
    }

    #[test]
    fn transform_midi_notes_touches_only_the_selected_notes() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        core.application(&storage)
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
        core.application(&storage)
            .add_midi_note_with_created_ids("midi:1", TimelineTick(0), 60, 480, 100, 1)
            .unwrap();
        let with_second = core
            .application(&storage)
            .add_midi_note_with_created_ids("midi:1", TimelineTick(480), 64, 240, 40, 1)
            .unwrap();
        let ids: Vec<String> = with_second.session.arrangement.midi_clips[0]
            .notes
            .iter()
            .map(|note| note.id.clone())
            .collect();

        let transformed = core
            .application(&storage)
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
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        core.application(&storage)
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
        core.application(&storage)
            .add_midi_note_with_created_ids("midi:1", TimelineTick(0), 60, 480, 100, 1)
            .unwrap();
        core.application(&storage)
            .add_midi_note_with_created_ids("midi:1", TimelineTick(480), 64, 240, 40, 1)
            .unwrap();

        let transformed = core
            .application(&storage)
            .transform_midi_notes("midi:1", Vec::new(), -3, 5)
            .unwrap();

        let notes = &transformed.arrangement.midi_clips[0].notes;
        assert_eq!((notes[0].note, notes[0].velocity), (57, 105));
        assert_eq!((notes[1].note, notes[1].velocity), (61, 45));
    }

    #[test]
    fn transform_midi_notes_clamps_pitch_and_velocity() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        core.application(&storage)
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
        core.application(&storage)
            .add_midi_note_with_created_ids("midi:1", TimelineTick(0), 120, 480, 20, 1)
            .unwrap();

        let transformed = core
            .application(&storage)
            .transform_midi_notes("midi:1", Vec::new(), 30, -200)
            .unwrap();

        let notes = &transformed.arrangement.midi_clips[0].notes;
        assert_eq!(notes[0].note, 127);
        assert_eq!(notes[0].velocity, 0);
    }

    #[test]
    fn transform_midi_notes_rejects_unknown_note_ids() {
        let storage = MemoryStorage::default();
        let mut core = AppCore::new("project:test".into(), CreativeSession::new(1), 0);
        let track = core
            .application(&storage)
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        core.application(&storage)
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
        core.application(&storage)
            .add_midi_note_with_created_ids("midi:1", TimelineTick(0), 60, 480, 100, 1)
            .unwrap();

        let error = core
            .application(&storage)
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
