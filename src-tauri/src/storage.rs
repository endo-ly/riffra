use crate::model::{CURRENT_SESSION_FORMAT, RecoveryCandidate, ScratchSession};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const GENERATIONS_TO_KEEP: usize = 20;
const STORAGE_HEADROOM_BYTES: u64 = 64 * 1024;
const MIGRATION_BACKUP_TAG: &str = "migration-backup";

/// Result of loading the active session, including how the load resolved.
#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub session: ScratchSession,
    pub recovered_from_generation: bool,
    pub migration: Option<MigrationNotice>,
}

/// Explicit record produced when a session file uses an unsupported format version.
///
/// The original file is never modified or deleted by the migration reader; a byte-identical
/// backup is written before any fallback so the user's data stays recoverable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationNotice {
    pub found_format: u32,
    pub expected_format: u32,
    pub backup_path: PathBuf,
}

/// Failure mode while reading a session file.
#[derive(Debug)]
pub enum SessionLoadError {
    Corrupt(io::Error),
    UnsupportedFormat(MigrationNotice),
}

impl From<io::Error> for SessionLoadError {
    fn from(error: io::Error) -> Self {
        SessionLoadError::Corrupt(error)
    }
}

impl std::fmt::Display for SessionLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionLoadError::Corrupt(error) => write!(formatter, "corrupt session: {error}"),
            SessionLoadError::UnsupportedFormat(notice) => write!(
                formatter,
                "session format {} is unsupported (expected {}); original backed up to {}",
                notice.found_format,
                notice.expected_format,
                notice.backup_path.display()
            ),
        }
    }
}

impl std::error::Error for SessionLoadError {}

#[derive(Debug)]
pub struct SessionStore {
    scratch_dir: PathBuf,
    generations_dir: PathBuf,
}

impl SessionStore {
    pub fn new(data_root: &Path) -> Self {
        let scratch_dir = data_root.join("scratch");
        let generations_dir = scratch_dir.join("generations");
        Self {
            scratch_dir,
            generations_dir,
        }
    }

    pub fn ensure_layout(&self) -> io::Result<()> {
        fs::create_dir_all(&self.generations_dir)
    }

    pub fn load_or_create(&self) -> Result<LoadedSession, SessionLoadError> {
        self.ensure_layout()?;
        let current = self.scratch_dir.join("current.json");
        let mut migration_notice = None;
        if current.is_file() {
            match self.read_session_with_migration(&current) {
                Ok(session) => {
                    return Ok(LoadedSession {
                        session,
                        recovered_from_generation: false,
                        migration: None,
                    });
                }
                Err(SessionLoadError::UnsupportedFormat(notice)) => {
                    // The original file stays on disk and a byte-identical backup already exists,
                    // so falling back must not lose data. The mismatch is reported explicitly.
                    migration_notice = Some(notice);
                }
                Err(SessionLoadError::Corrupt(_)) => {
                    // Unreadable current file: fall through to the newest valid generation.
                }
            }
        }

        if let Some(session) = self.newest_valid_generation()? {
            return Ok(LoadedSession {
                session,
                recovered_from_generation: true,
                migration: migration_notice,
            });
        }

        let session = ScratchSession::new(now_ms());
        self.save(&session)?;
        Ok(LoadedSession {
            session,
            recovered_from_generation: false,
            migration: migration_notice,
        })
    }

    /// Reads the active session and confirms its format version. On a version mismatch a
    /// pre-conversion backup of the original bytes is written and the mismatch is reported
    /// explicitly instead of being silently discarded.
    fn read_session_with_migration(&self, path: &Path) -> Result<ScratchSession, SessionLoadError> {
        let payload = fs::read(path).map_err(SessionLoadError::Corrupt)?;
        let session: ScratchSession = serde_json::from_slice(&payload).map_err(|error| {
            SessionLoadError::Corrupt(io::Error::new(io::ErrorKind::InvalidData, error))
        })?;
        if session.format_version != CURRENT_SESSION_FORMAT {
            let backup_path = self.write_migration_backup(path, &payload)?;
            return Err(SessionLoadError::UnsupportedFormat(MigrationNotice {
                found_format: session.format_version,
                expected_format: CURRENT_SESSION_FORMAT,
                backup_path,
            }));
        }
        session.validate_and_normalize().map_err(|error| {
            SessionLoadError::Corrupt(io::Error::new(io::ErrorKind::InvalidData, error))
        })
    }

    fn write_migration_backup(&self, original: &Path, payload: &[u8]) -> io::Result<PathBuf> {
        let stem = original
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("current");
        let backup_path =
            original.with_file_name(format!("{stem}.{MIGRATION_BACKUP_TAG}.{}.json", now_ms()));
        fs::write(&backup_path, payload)?;
        Ok(backup_path)
    }

    fn newest_valid_generation(&self) -> io::Result<Option<ScratchSession>> {
        for candidate in self.generation_files_newest_first()? {
            if let Ok(session) = read_session(&candidate) {
                return Ok(Some(session));
            }
        }
        Ok(None)
    }

    pub fn save(&self, session: &ScratchSession) -> io::Result<()> {
        self.ensure_layout()?;
        let current = self.scratch_dir.join("current.json");
        let payload = serde_json::to_vec_pretty(session)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let current_bytes = current
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        ensure_storage_capacity(
            &self.scratch_dir,
            required_space(payload.len() as u64, current_bytes),
        )?;
        if current.is_file() {
            let generation =
                self.generations_dir
                    .join(format!("{}-{}.json", now_ms(), std::process::id()));
            fs::copy(&current, generation)?;
        }
        let temporary =
            self.scratch_dir
                .join(format!(".current-{}-{}.tmp", std::process::id(), now_ms()));
        {
            let mut file = File::create(&temporary)?;
            file.write_all(&payload)?;
            file.sync_all()?;
        }

        if let Err(error) = fs::rename(&temporary, &current) {
            if current.exists() {
                fs::remove_file(&current)?;
                fs::rename(&temporary, &current)?;
            } else {
                return Err(error);
            }
        }
        self.prune_generations()
    }

    pub fn recovery_candidates(&self) -> io::Result<Vec<RecoveryCandidate>> {
        self.ensure_layout()?;
        let mut candidates = Vec::new();
        for path in self.generation_files_newest_first()? {
            let Ok(session) = read_session(&path) else {
                continue;
            };
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            candidates.push(RecoveryCandidate {
                file_name: file_name.to_owned(),
                updated_at_ms: session.updated_at_ms,
                session_id: session.session_id,
                project_name: session.project_name,
                note: session.note,
            });
        }
        Ok(candidates)
    }

    pub fn restore_generation(&self, file_name: &str) -> io::Result<ScratchSession> {
        if file_name.trim().is_empty()
            || file_name
                != Path::new(file_name)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
            || !file_name.ends_with(".json")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Recovery generation name is invalid.",
            ));
        }
        let path = self.generations_dir.join(file_name);
        let session = read_session(&path)?;
        self.save(&session)?;
        Ok(session)
    }

    fn generation_files_newest_first(&self) -> io::Result<Vec<PathBuf>> {
        let mut files = fs::read_dir(&self.generations_dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
        Ok(files)
    }

    fn prune_generations(&self) -> io::Result<()> {
        for stale in self
            .generation_files_newest_first()?
            .into_iter()
            .skip(GENERATIONS_TO_KEEP)
        {
            let _ = fs::remove_file(stale);
        }
        Ok(())
    }
}

fn required_space(payload_bytes: u64, current_bytes: u64) -> u64 {
    payload_bytes
        .saturating_add(current_bytes)
        .saturating_add(STORAGE_HEADROOM_BYTES)
}

fn ensure_storage_capacity(directory: &Path, required_bytes: u64) -> io::Result<()> {
    let Some(available) = available_space(directory)? else {
        return Ok(());
    };
    if available < required_bytes {
        return Err(io::Error::new(
            io::ErrorKind::StorageFull,
            format!(
                "Not enough free disk space for an atomic session save ({} bytes required, {} available).",
                required_bytes, available
            ),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn available_space(directory: &Path) -> io::Result<Option<u64>> {
    use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let mut path = directory.as_os_str().encode_wide().collect::<Vec<_>>();
    path.push(0);
    let mut free_bytes = 0_u64;
    // The directory is created before this check, so Windows can resolve the volume.
    let success =
        unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free_bytes, null_mut(), null_mut()) };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(Some(free_bytes))
    }
}

#[cfg(not(windows))]
fn available_space(_directory: &Path) -> io::Result<Option<u64>> {
    Ok(None)
}

fn read_session(path: &Path) -> io::Result<ScratchSession> {
    let payload = fs::read(path)?;
    serde_json::from_slice::<ScratchSession>(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .validate_and_normalize()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AiChangeSet, DeviceKind, RackDevice, SamplePad, SessionSnapshot, TimelineClip,
    };

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("riffra-{name}-{}", now_ms()))
    }

    #[test]
    fn keeps_last_valid_generation_when_current_is_corrupt() {
        let root = test_root("recovery");
        let store = SessionStore::new(&root);
        let mut first = ScratchSession::new(now_ms());
        first.note = "recover me".into();
        store.save(&first).unwrap();
        let mut second = first.clone();
        second.note = "newer".into();
        store.save(&second).unwrap();
        fs::write(root.join("scratch/current.json"), b"not json").unwrap();

        let loaded = store.load_or_create().unwrap();
        assert!(loaded.recovered_from_generation);
        assert_eq!(loaded.session.note, "recover me");
        assert!(loaded.migration.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_and_restores_valid_recovery_generations() {
        let root = test_root("recovery-candidates");
        let store = SessionStore::new(&root);
        let mut first = ScratchSession::new(now_ms());
        first.note = "stable choice".into();
        store.save(&first).unwrap();
        let mut second = first.clone();
        second.note = "newer choice".into();
        store.save(&second).unwrap();
        let candidates = store.recovery_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        let restored = store.restore_generation(&candidates[0].file_name).unwrap();
        assert_eq!(restored.note, "stable choice");
        let loaded = store.load_or_create().unwrap();
        assert!(!loaded.recovered_from_generation);
        assert_eq!(loaded.session.note, "stable choice");
        assert!(loaded.migration.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_recovery_paths_outside_generation_directory() {
        let root = test_root("recovery-path");
        let store = SessionStore::new(&root);
        let error = store.restore_generation("..\\current.json").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clamps_unsafe_master_gain_before_persisting() {
        let mut session = ScratchSession::new(now_ms());
        session.master_db = 12.0;
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.master_db, 0.0);
    }

    #[test]
    fn preserves_audio_driver_preference() {
        let mut session = ScratchSession::new(now_ms());
        session.audio_driver = Some("ASIO".into());
        let encoded = serde_json::to_vec(&session).unwrap();
        let decoded: ScratchSession = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.audio_driver.as_deref(), Some("ASIO"));
    }

    #[test]
    fn preserves_bounded_count_in_preference() {
        let mut session = ScratchSession::new(now_ms());
        session.count_in_beats = 4;
        let normalized = session.clone().validate_and_normalize().unwrap();
        assert_eq!(normalized.count_in_beats, 4);
        session.count_in_beats = 9;
        assert!(session.validate_and_normalize().is_err());
    }

    #[test]
    fn preserves_ai_permission_and_context_preferences() {
        let mut session = ScratchSession::new(now_ms());
        session.ai_permission = "Apply".into();
        session.ai_context = vec!["selectedRack".into(), "analysis".into()];
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.ai_permission, "Apply");
        assert_eq!(normalized.ai_context, vec!["selectedRack", "analysis"]);
    }

    #[test]
    fn rejects_unknown_ai_permission_and_context() {
        let mut session = ScratchSession::new(now_ms());
        session.ai_permission = "Auto".into();
        assert!(session.validate_and_normalize().is_err());

        let mut session = ScratchSession::new(now_ms());
        session.ai_context = vec!["unknown".into(), "analysis".into(), "analysis".into()];
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.ai_context, vec!["analysis"]);
    }

    #[test]
    fn preserves_reversible_ai_change_set_history() {
        let mut session = ScratchSession::new(now_ms());
        session.ai_history.push(AiChangeSet {
            id: "ai:1".into(),
            created_at_ms: now_ms(),
            permission: "Apply".into(),
            target: "clip:1".into(),
            current_gain_db: 0.0,
            proposed_gain_db: -3.0,
            reason: "Match reference RMS".into(),
            expected_effect: "Closer perceived level".into(),
            risk: "Low · reversible".into(),
            context: vec!["analysis".into(), "selectedClip".into()],
            applied: true,
        });
        let encoded = serde_json::to_vec(&session).unwrap();
        let decoded: ScratchSession = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.ai_history.len(), 1);
        assert_eq!(decoded.ai_history[0].target, "clip:1");
        assert!(decoded.ai_history[0].applied);
    }

    #[test]
    fn preserves_persisted_plugin_path() {
        let mut session = ScratchSession::new(now_ms());
        session.rack.push(RackDevice {
            id: "plugin:amplitube".into(),
            name: "AmpliTube 5".into(),
            kind: DeviceKind::Plugin,
            path: Some(r"C:\Program Files\Common Files\VST3\AmpliTube 5.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        });
        let encoded = serde_json::to_vec(&session).unwrap();
        let decoded: ScratchSession = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            decoded
                .rack
                .last()
                .and_then(|device| device.path.as_deref()),
            Some(r"C:\Program Files\Common Files\VST3\AmpliTube 5.vst3")
        );
    }

    #[test]
    fn preserves_a_b_snapshot_state() {
        let mut session = ScratchSession::new(now_ms());
        session.snapshots.push(SessionSnapshot {
            id: "snapshot:A".into(),
            name: "A".into(),
            created_at_ms: now_ms(),
            description: "Clean reference".into(),
            tag: Some("reference".into()),
            parent_id: None,
            master_db: -18.0,
            rack: session.rack.clone(),
            macros: session.macros.clone(),
        });
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.snapshots[0].name, "A");
        assert_eq!(normalized.snapshots[0].rack.len(), 3);
    }

    #[test]
    fn preserves_plugin_state_through_storage_round_trip() {
        let root = test_root("plugin-state");
        let store = SessionStore::new(&root);
        let mut session = ScratchSession::new(now_ms());
        session.rack.push(RackDevice {
            id: "plugin:amplitube".into(),
            name: "AmpliTube 5".into(),
            kind: DeviceKind::Plugin,
            path: Some(r"C:\Program Files\Common Files\VST3\AmpliTube 5.vst3".into()),
            bypassed: true,
            gain_db: -6.0,
            parameter_values: vec![0.8, 0.3],
            state_data: Some("opaque-vst3-state-blob".into()),
            disabled_placeholder: false,
        });
        store.save(&session).unwrap();
        let loaded = read_session(&root.join("scratch/current.json")).unwrap();
        let device = loaded.rack.last().expect("rack device preserved");
        assert_eq!(device.state_data.as_deref(), Some("opaque-vst3-state-blob"));
        assert!(device.bypassed);
        assert_eq!(device.gain_db, -6.0);
        assert_eq!(device.parameter_values, vec![0.8, 0.3]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preserves_plugin_state_within_snapshot_round_trip() {
        let root = test_root("plugin-snapshot");
        let store = SessionStore::new(&root);
        let mut session = ScratchSession::new(now_ms());
        session.rack.push(RackDevice {
            id: "plugin:valhalla".into(),
            name: "ValhallaSupermassive".into(),
            kind: DeviceKind::Plugin,
            path: Some(r"C:\VST3\ValhallaSupermassive.vst3".into()),
            bypassed: false,
            gain_db: -3.0,
            parameter_values: vec![0.25, 0.5, 0.75],
            state_data: Some("opaque-snapshot-state".into()),
            disabled_placeholder: false,
        });
        session.snapshots.push(SessionSnapshot {
            id: "snapshot:B".into(),
            name: "B".into(),
            created_at_ms: now_ms(),
            description: "Wet space".into(),
            tag: None,
            parent_id: None,
            master_db: -12.0,
            rack: session.rack.clone(),
            macros: session.macros.clone(),
        });
        store.save(&session).unwrap();
        let loaded = read_session(&root.join("scratch/current.json")).unwrap();
        let device = loaded
            .snapshots
            .last()
            .and_then(|snapshot| snapshot.rack.last())
            .expect("snapshot rack preserved");
        assert_eq!(device.state_data.as_deref(), Some("opaque-snapshot-state"));
        assert_eq!(device.parameter_values, vec![0.25, 0.5, 0.75]);
        assert_eq!(device.gain_db, -3.0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preserves_non_destructive_timeline_clip() {
        let mut session = ScratchSession::new(now_ms());
        session.timeline.push(TimelineClip {
            id: "clip:take-1".into(),
            asset_path: r"C:\recordings\take-1\processed.wav".into(),
            name: "take-1".into(),
            track_id: "main".into(),
            start_ms: 250,
            duration_ms: 1_000,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let encoded = serde_json::to_vec(&session).unwrap();
        let decoded: ScratchSession = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.timeline[0].start_ms, 250);
        assert_eq!(
            decoded.timeline[0].asset_path,
            r"C:\recordings\take-1\processed.wav"
        );
    }

    #[test]
    fn caps_kept_generations_and_keeps_newest_on_prune() {
        let root = test_root("generation-cap");
        let store = SessionStore::new(&root);
        let mut newest = ScratchSession::new(now_ms());
        for index in 0..(GENERATIONS_TO_KEEP + 5) {
            let mut session = ScratchSession::new(now_ms());
            session.note = format!("generation-{index}");
            store.save(&session).unwrap();
            newest = session;
        }
        let candidates = store.recovery_candidates().unwrap();
        assert!(
            candidates.len() <= GENERATIONS_TO_KEEP,
            "too many generations kept: {}",
            candidates.len()
        );
        let loaded = store.load_or_create().unwrap();
        assert!(!loaded.recovered_from_generation);
        assert_eq!(loaded.session.note, newest.note);
        assert!(loaded.migration.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reserves_space_for_current_generation_and_atomic_temp_file() {
        assert_eq!(required_space(100, 50), 64 * 1024 + 150);
    }

    #[test]
    fn preserves_sample_pad_mapping() {
        let mut session = ScratchSession::new(now_ms());
        session.sample_pads.push(SamplePad {
            id: "pad:take-1".into(),
            name: "take-1".into(),
            asset_path: r"C:\recordings\take-1\processed.wav".into(),
            start_ms: 0,
            end_ms: 1_000,
            midi_key: 36,
            gain_db: 0.0,
            loop_enabled: false,
        });
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.sample_pads[0].midi_key, 36);
        assert_eq!(normalized.sample_pads[0].end_ms, 1_000);
    }

    fn write_current_with_format(root: &Path, format_version: u32) {
        let mut value = serde_json::to_value(ScratchSession::new(now_ms())).unwrap();
        value["formatVersion"] = serde_json::Value::from(format_version);
        let scratch = root.join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        fs::write(
            scratch.join("current.json"),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn loads_current_format_without_migration() {
        let root = test_root("migration-current");
        let store = SessionStore::new(&root);
        store.save(&ScratchSession::new(now_ms())).unwrap();
        let loaded = store.load_or_create().unwrap();
        assert!(loaded.migration.is_none());
        assert!(!loaded.recovered_from_generation);
        assert_eq!(loaded.session.format_version, CURRENT_SESSION_FORMAT);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn backs_up_and_reports_unsupported_older_format() {
        let root = test_root("migration-older");
        write_current_with_format(&root, 0);
        let store = SessionStore::new(&root);
        let loaded = store.load_or_create().unwrap();
        let notice = loaded
            .migration
            .expect("unsupported format must be reported explicitly");
        assert_eq!(notice.found_format, 0);
        assert_eq!(notice.expected_format, CURRENT_SESSION_FORMAT);
        assert!(
            notice.backup_path.is_file(),
            "pre-conversion backup must exist"
        );
        let backup: ScratchSession =
            serde_json::from_slice(&fs::read(&notice.backup_path).unwrap()).unwrap();
        assert_eq!(
            backup.format_version, 0,
            "backup preserves the original format version"
        );
        assert!(
            root.join("scratch/current.json").is_file(),
            "original file is preserved on disk"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn backs_up_and_reports_newer_format() {
        let root = test_root("migration-newer");
        write_current_with_format(&root, 99);
        let store = SessionStore::new(&root);
        let loaded = store.load_or_create().unwrap();
        let notice = loaded
            .migration
            .expect("unsupported format must be reported explicitly");
        assert_eq!(notice.found_format, 99);
        assert_eq!(notice.expected_format, CURRENT_SESSION_FORMAT);
        assert!(notice.backup_path.is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovers_from_generation_when_format_unsupported() {
        let root = test_root("migration-generation");
        let store = SessionStore::new(&root);
        let mut base = ScratchSession::new(now_ms());
        base.note = "generation choice".into();
        store.save(&base).unwrap();
        store.save(&ScratchSession::new(now_ms())).unwrap();
        write_current_with_format(&root, 0);
        let loaded = store.load_or_create().unwrap();
        assert!(loaded.recovered_from_generation);
        assert_eq!(loaded.session.note, "generation choice");
        let notice = loaded
            .migration
            .expect("unsupported format must be reported explicitly");
        assert_eq!(notice.found_format, 0);
        let _ = fs::remove_dir_all(root);
    }
}
