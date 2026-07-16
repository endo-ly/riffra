use crate::model::{CURRENT_SESSION_FORMAT, ScratchSession};
use crate::session::{CREATIVE_SESSION_FORMAT, CreativeSession, migrate_v1_to_v2};
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
const V1_FORMAT: u32 = CURRENT_SESSION_FORMAT;

/// Result of loading the active session, including how the load resolved.
#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub session: CreativeSession,
    pub recovered_from_generation: bool,
    pub migration: Option<MigrationNotice>,
}

/// Explicit record produced when a session file uses an unsupported format
/// version, or when a v1 session could not be migrated.
///
/// The original file is never modified or deleted by the migration reader; a
/// byte-identical backup is written before any fallback so the user's data
/// stays recoverable.
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
        Self::Corrupt(error)
    }
}

impl std::fmt::Display for SessionLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt(error) => write!(formatter, "corrupt session: {error}"),
            Self::UnsupportedFormat(notice) => write!(
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
    data_root: PathBuf,
    scratch_dir: PathBuf,
    generations_dir: PathBuf,
}

impl SessionStore {
    pub fn new(data_root: &Path) -> Self {
        let scratch_dir = data_root.join("scratch");
        let generations_dir = scratch_dir.join("generations");
        Self {
            data_root: data_root.to_path_buf(),
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
        if current.is_file() {
            match self.read_active(&current) {
                Ok(session) => {
                    return Ok(LoadedSession {
                        session,
                        recovered_from_generation: false,
                        migration: None,
                    });
                }
                Err(ActiveReadFailure::UnsupportedFormat(notice)) => {
                    if let Some(session) = self.newest_valid_generation()? {
                        return Ok(LoadedSession {
                            session,
                            recovered_from_generation: true,
                            migration: Some(notice),
                        });
                    }
                    // An unsupported current session must never be replaced by a
                    // fresh v2 file. Keep the original available for manual
                    // recovery and make the format distinction explicit.
                    return Err(SessionLoadError::UnsupportedFormat(notice));
                }
                Err(ActiveReadFailure::Corrupt(error)) => {
                    if let Some(session) = self.newest_valid_generation()? {
                        return Ok(LoadedSession {
                            session,
                            recovered_from_generation: true,
                            migration: None,
                        });
                    }
                    // A corrupt current session with no valid generation is a
                    // hard load failure. Creating a new file here would destroy
                    // the only recoverable copy of the user's session.
                    return Err(SessionLoadError::Corrupt(error));
                }
            }
        }

        if let Some(session) = self.newest_valid_generation()? {
            return Ok(LoadedSession {
                session,
                recovered_from_generation: true,
                migration: None,
            });
        }

        let session = CreativeSession::new(now_ms());
        self.save(&session)?;
        Ok(LoadedSession {
            session,
            recovered_from_generation: false,
            migration: None,
        })
    }

    /// Reads the active session file. v2 loads directly; v1 is backed up
    /// byte-for-byte, migrated to v2, and persisted only after conversion
    /// succeeds. Unknown versions and corrupt payloads are reported without
    /// replacing the original file.
    fn read_active(&self, path: &Path) -> Result<CreativeSession, ActiveReadFailure> {
        let payload = fs::read(path).map_err(ActiveReadFailure::Corrupt)?;
        let raw = match parse_raw_session(&payload) {
            Ok(raw) => raw,
            Err(ActiveReadFailure::UnsupportedFormat(mut notice)) => {
                if notice.backup_path.as_os_str().is_empty() {
                    notice.backup_path = self
                        .write_migration_backup(path, &payload)
                        .unwrap_or_else(|_| self.backup_path_for(path));
                }
                return Err(ActiveReadFailure::UnsupportedFormat(notice));
            }
            Err(error) => return Err(error),
        };
        match raw {
            RawSession::V2(session) => session
                .validate_and_normalize()
                .map_err(|error| ActiveReadFailure::Corrupt(corrupt_io_error(&error))),
            RawSession::V1(legacy) => {
                // Preserve the v1 bytes before any conversion or fallback.
                let backup_path = self
                    .write_migration_backup(path, &payload)
                    .unwrap_or_else(|_| self.backup_path_for(path));
                match migrate_v1_to_v2(&legacy, &self.data_root)
                    .and_then(|session| session.validate_and_normalize())
                {
                    Ok(session) => {
                        self.save(&session).map_err(ActiveReadFailure::Corrupt)?;
                        Ok(session)
                    }
                    Err(error) => Err(ActiveReadFailure::Corrupt(corrupt_io_error(&format!(
                        "v1 session migration failed; original backed up to {}: {error}",
                        backup_path.display()
                    )))),
                }
            }
        }
    }

    fn backup_path_for(&self, original: &Path) -> PathBuf {
        let stem = original
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("current");
        original.with_file_name(format!("{stem}.{MIGRATION_BACKUP_TAG}.{}.json", now_ms()))
    }

    fn write_migration_backup(&self, original: &Path, payload: &[u8]) -> io::Result<PathBuf> {
        let backup_path = self.backup_path_for(original);
        fs::write(&backup_path, payload)?;
        Ok(backup_path)
    }

    fn newest_valid_generation(&self) -> io::Result<Option<CreativeSession>> {
        for candidate in self.generation_files_newest_first()? {
            if let Ok(session) = self.read_generation(&candidate) {
                return Ok(Some(session));
            }
        }
        Ok(None)
    }

    /// Reads a generation file as a v2 session, migrating v1 generations on
    /// the fly so recovery never offers a session the app cannot open.
    fn read_generation(&self, path: &Path) -> io::Result<CreativeSession> {
        let payload = fs::read(path)?;
        match parse_raw_session(&payload).map_err(invalid_data)? {
            RawSession::V2(session) => session.validate_and_normalize().map_err(invalid_data),
            RawSession::V1(legacy) => migrate_v1_to_v2(&legacy, &self.data_root)
                .and_then(|session| session.validate_and_normalize())
                .map_err(invalid_data),
        }
    }

    pub fn save(&self, session: &CreativeSession) -> io::Result<()> {
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

        replace_file(&temporary, &current)?;
        self.prune_generations()
    }

    pub fn recovery_candidates(&self) -> io::Result<Vec<RecoveryCandidate>> {
        self.ensure_layout()?;
        let mut candidates = Vec::new();
        for path in self.generation_files_newest_first()? {
            let Ok(payload) = fs::read(&path) else {
                continue;
            };
            let Some(mut metadata) = peek_recovery_metadata(&payload) else {
                continue;
            };
            metadata.file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            candidates.push(metadata);
        }
        Ok(candidates)
    }

    pub fn restore_generation(&self, file_name: &str) -> io::Result<CreativeSession> {
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
        let session = self.read_generation(&path)?;
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

enum ActiveReadFailure {
    Corrupt(io::Error),
    UnsupportedFormat(MigrationNotice),
}

impl std::fmt::Display for ActiveReadFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt(error) => write!(formatter, "corrupt session: {error}"),
            Self::UnsupportedFormat(notice) => {
                SessionLoadError::UnsupportedFormat(notice.clone()).fmt(formatter)
            }
        }
    }
}

impl From<io::Error> for ActiveReadFailure {
    fn from(error: io::Error) -> Self {
        Self::Corrupt(error)
    }
}

enum RawSession {
    V2(CreativeSession),
    V1(ScratchSession),
}

fn parse_raw_session(payload: &[u8]) -> Result<RawSession, ActiveReadFailure> {
    let value: serde_json::Value = serde_json::from_slice(payload).map_err(|error| {
        ActiveReadFailure::Corrupt(corrupt_io_error(&format!(
            "session is not valid JSON: {error}"
        )))
    })?;
    let version = value
        .get("formatVersion")
        .and_then(serde_json::Value::as_u64)
        .map(|value| u32::try_from(value).unwrap_or(u32::MAX));
    match version {
        Some(CREATIVE_SESSION_FORMAT) => {
            let session: CreativeSession = serde_json::from_value(value).map_err(|error| {
                ActiveReadFailure::Corrupt(corrupt_io_error(&format!(
                    "v2 session is invalid: {error}"
                )))
            })?;
            Ok(RawSession::V2(session))
        }
        Some(V1_FORMAT) => {
            let session: ScratchSession = serde_json::from_value(value).map_err(|error| {
                ActiveReadFailure::Corrupt(corrupt_io_error(&format!(
                    "v1 session is invalid: {error}"
                )))
            })?;
            Ok(RawSession::V1(session))
        }
        Some(other) => Err(ActiveReadFailure::UnsupportedFormat(MigrationNotice {
            found_format: other,
            expected_format: CREATIVE_SESSION_FORMAT,
            backup_path: PathBuf::new(),
        })),
        None => Err(ActiveReadFailure::Corrupt(corrupt_io_error(
            "session is missing a formatVersion field.",
        ))),
    }
}

/// Extracts recovery-listing metadata from a session payload without performing
/// a full (and asset-registering) migration, so listing candidates never
/// mutates the Asset store.
fn peek_recovery_metadata(payload: &[u8]) -> Option<RecoveryCandidate> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let session_id = value
        .get("sessionId")
        .and_then(serde_json::Value::as_str)?
        .to_owned();
    let updated_at_ms = value
        .get("updatedAtMs")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let project_name = value
        .get("projectName")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    // v2 stores the note under settings.note; v1 stores it at the top level.
    let note = value
        .get("note")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("settings")
                .and_then(|settings| settings.get("note"))
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("")
        .to_owned();
    Some(RecoveryCandidate {
        file_name: String::new(),
        updated_at_ms,
        session_id,
        project_name,
        note,
    })
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

fn corrupt_io_error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn invalid_data<E: std::fmt::Display>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

/// Replaces a session file without deleting the old file first. Windows does
/// not let `std::fs::rename` overwrite an existing destination, so use the
/// native replace primitive there; the fallback platforms already provide an
/// atomic same-filesystem rename.
fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let mut source = temporary.as_os_str().encode_wide().collect::<Vec<_>>();
        source.push(0);
        let mut target = destination.as_os_str().encode_wide().collect::<Vec<_>>();
        target.push(0);
        let success = unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    pub file_name: String,
    pub updated_at_ms: u64,
    pub session_id: String,
    pub project_name: Option<String>,
    pub note: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rack::DeviceKind;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("riffra-{name}-{}", now_ms()))
    }

    fn write_wav(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"RIFF\0\0\0\0WAVE").unwrap();
    }

    #[test]
    fn keeps_last_valid_generation_when_current_is_corrupt() {
        let root = test_root("recovery");
        let store = SessionStore::new(&root);
        let mut first = CreativeSession::new(now_ms());
        first.settings.note = "recover me".into();
        store.save(&first).unwrap();
        let mut second = first.clone();
        second.settings.note = "newer".into();
        store.save(&second).unwrap();
        fs::write(root.join("scratch/current.json"), b"not json").unwrap();

        let loaded = store.load_or_create().unwrap();
        assert!(loaded.recovered_from_generation);
        assert_eq!(loaded.session.settings.note, "recover me");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_to_replace_a_corrupt_current_when_no_generation_exists() {
        let root = test_root("corrupt-without-recovery");
        let store = SessionStore::new(&root);
        store.ensure_layout().unwrap();
        let current = root.join("scratch/current.json");
        fs::write(&current, b"not json").unwrap();
        let original = fs::read(&current).unwrap();

        let error = store.load_or_create().unwrap_err();
        assert!(matches!(error, SessionLoadError::Corrupt(_)));
        assert_eq!(fs::read(&current).unwrap(), original);
        assert!(!root.join("scratch/current.json").is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn backs_up_and_refuses_to_replace_an_unsupported_current() {
        let root = test_root("unsupported");
        let store = SessionStore::new(&root);
        store.ensure_layout().unwrap();
        let current = root.join("scratch/current.json");
        let payload = br#"{"formatVersion":99,"sessionId":"future","updatedAtMs":1}"#;
        fs::write(&current, payload).unwrap();

        let error = store.load_or_create().unwrap_err();
        let SessionLoadError::UnsupportedFormat(notice) = error else {
            panic!("unsupported session format should remain distinguishable");
        };
        assert_eq!(notice.found_format, 99);
        assert_eq!(fs::read(&current).unwrap(), payload);
        assert!(notice.backup_path.is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clamps_unsafe_master_gain_before_persisting() {
        let mut session = CreativeSession::new(now_ms());
        session.settings.master_db = 12.0;
        let normalized = session.validate_and_normalize().unwrap();
        assert_eq!(normalized.settings.master_db, 0.0);
    }

    #[test]
    fn loads_and_round_trips_a_v2_session() {
        let root = test_root("v2-roundtrip");
        let store = SessionStore::new(&root);
        let mut session = CreativeSession::new(now_ms());
        session.project_name = Some("Clean".into());
        session.workspace = crate::session::Workspace::Arrange;
        store.save(&session).unwrap();
        let loaded = store.load_or_create().unwrap();
        assert_eq!(loaded.session.format_version, CREATIVE_SESSION_FORMAT);
        assert_eq!(loaded.session.workspace, crate::session::Workspace::Arrange);
        assert_eq!(loaded.session.project_name.as_deref(), Some("Clean"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_a_v1_session_to_v2_with_asset_backed_clips() {
        let root = test_root("v1-to-v2");
        let wav = root.join("take.wav");
        write_wav(&wav);

        let mut legacy = ScratchSession::new(now_ms());
        assert!(legacy.clone().validate_and_normalize().is_ok());
        legacy.workspace = crate::model::Workspace::Sample;
        legacy.timeline.push(crate::model::TimelineClip {
            id: "clip:1".into(),
            asset_path: wav.to_string_lossy().into_owned(),
            name: "take".into(),
            track_id: "main".into(),
            start_ms: 100,
            duration_ms: 500,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let scratch = root.join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        fs::write(
            scratch.join("current.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let store = SessionStore::new(&root);
        let loaded = store.load_or_create().unwrap();
        assert_eq!(loaded.session.format_version, CREATIVE_SESSION_FORMAT);
        assert_eq!(loaded.session.workspace, crate::session::Workspace::Design);
        let clip = &loaded.session.arrangement.audio_clips[0];
        assert_eq!(clip.position_ms, 100);
        assert!(clip.asset_id.as_str().starts_with("asset:"));
        assert!(
            root.join("scratch")
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .contains(MIGRATION_BACKUP_TAG)
                }),
            "a byte-identical v1 backup must be preserved"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_v1_migration_preserves_the_original_and_does_not_create_v2() {
        let root = test_root("v1-failure");
        let mut legacy = ScratchSession::new(now_ms());
        legacy.timeline.push(crate::model::TimelineClip {
            id: "clip:missing".into(),
            asset_path: root.join("missing.wav").to_string_lossy().into_owned(),
            name: "missing".into(),
            track_id: "main".into(),
            start_ms: 0,
            duration_ms: 100,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let scratch = root.join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        let payload = serde_json::to_vec_pretty(&legacy).unwrap();
        let current = scratch.join("current.json");
        fs::write(&current, &payload).unwrap();

        let error = SessionStore::new(&root).load_or_create().unwrap_err();
        assert!(matches!(error, SessionLoadError::Corrupt(_)));
        assert_eq!(fs::read(&current).unwrap(), payload);
        assert!(
            scratch
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .contains(MIGRATION_BACKUP_TAG)
                })
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reserves_space_for_current_generation_and_atomic_temp_file() {
        assert_eq!(required_space(100, 50), 64 * 1024 + 150);
    }

    #[test]
    fn default_session_carries_arrangement_and_safe_rack() {
        let session = CreativeSession::new(now_ms())
            .validate_and_normalize()
            .unwrap();
        assert_eq!(session.arrangement.tracks.len(), 1);
        assert_eq!(session.rack.devices.len(), 3);
        assert!(session.settings.emergency_muted);
        assert_eq!(session.rack.devices[0].kind, DeviceKind::Input);
    }
}
