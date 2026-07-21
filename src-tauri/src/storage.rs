use crate::session::{CREATIVE_SESSION_FORMAT, CreativeSession};
use serde::Serialize;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const GENERATIONS_TO_KEEP: usize = 20;
const STORAGE_HEADROOM_BYTES: u64 = 64 * 1024;

/// Result of loading the active session, including how the load resolved.
#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub session: CreativeSession,
    pub recovered_from_generation: bool,
}

/// Failure mode while reading a session file.
#[derive(Debug)]
pub struct SessionLoadError(pub io::Error);

impl From<io::Error> for SessionLoadError {
    fn from(error: io::Error) -> Self {
        Self(error)
    }
}

impl std::fmt::Display for SessionLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "session could not be loaded: {}", self.0)
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
                    });
                }
                Err(error) => {
                    if let Some(session) = self.newest_valid_generation()? {
                        return Ok(LoadedSession {
                            session,
                            recovered_from_generation: true,
                        });
                    }
                    // A corrupt current session with no valid generation is a
                    // hard load failure. Creating a new file here would destroy
                    // the only recoverable copy of the user's session.
                    return Err(SessionLoadError(error));
                }
            }
        }

        if let Some(session) = self.newest_valid_generation()? {
            return Ok(LoadedSession {
                session,
                recovered_from_generation: true,
            });
        }

        let session = CreativeSession::new(now_ms());
        self.save(&session)?;
        Ok(LoadedSession {
            session,
            recovered_from_generation: false,
        })
    }

    /// Reads the active session file. Only the current v2 format is accepted;
    /// any other shape is reported as corrupt without touching the original.
    fn read_active(&self, path: &Path) -> Result<CreativeSession, io::Error> {
        let payload = fs::read(path)?;
        let session: CreativeSession = serde_json::from_slice(&payload)
            .map_err(|error| corrupt_io_error(&format!("v2 session is invalid: {error}")))?;
        if session.format_version != CREATIVE_SESSION_FORMAT {
            return Err(corrupt_io_error(&format!(
                "session format {} is not supported (expected {})",
                session.format_version, CREATIVE_SESSION_FORMAT
            )));
        }
        let session = session
            .validate_and_normalize()
            .map_err(|error| corrupt_io_error(&error))?;
        crate::asset::validate_session_references(&self.data_root, &session)
            .map_err(|error| corrupt_io_error(&error))?;
        Ok(session)
    }

    fn newest_valid_generation(&self) -> io::Result<Option<CreativeSession>> {
        for candidate in self.generation_files_newest_first()? {
            if let Ok(session) = self.read_generation(&candidate) {
                return Ok(Some(session));
            }
        }
        Ok(None)
    }

    /// Reads a generation file as a v2 session. Recovery never offers a session
    /// the app cannot open.
    fn read_generation(&self, path: &Path) -> io::Result<CreativeSession> {
        let payload = fs::read(path)?;
        let session: CreativeSession = serde_json::from_slice(&payload)?;
        if session.format_version != CREATIVE_SESSION_FORMAT {
            return Err(corrupt_io_error(&format!(
                "generation format {} is not supported (expected {})",
                session.format_version, CREATIVE_SESSION_FORMAT
            )));
        }
        let session = session.validate_and_normalize().map_err(invalid_data)?;
        crate::asset::validate_session_references(&self.data_root, &session)
            .map_err(invalid_data)?;
        Ok(session)
    }

    pub fn save(&self, session: &CreativeSession) -> io::Result<()> {
        crate::asset::validate_session_references(&self.data_root, session)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
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

/// Extracts recovery-listing metadata from a session payload without a full
/// deserialize, so listing candidates never mutates the Asset store.
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
    let note = value
        .get("settings")
        .and_then(|settings| settings.get("note"))
        .and_then(serde_json::Value::as_str)
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
pub(crate) fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
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
    use crate::asset::{AssetKind, mint_asset_id};
    use crate::rack::DeviceKind;
    use crate::session::AudioClip;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("riffra-{name}-{}", now_ms()))
    }

    fn write_wav(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"RIFF\0\0\0\0WAVE").unwrap();
    }

    fn push_audio_clip(
        session: &mut CreativeSession,
        id: &str,
        name: &str,
        asset_id: crate::asset::AssetId,
    ) {
        if session.arrangement.tracks.is_empty() {
            session
                .arrangement
                .tracks
                .push(crate::session::Track::audio("main".into(), "Main".into()));
        }
        session.arrangement.audio_clips.push(AudioClip::full_source(
            id.into(),
            name.into(),
            "main".into(),
            asset_id,
            crate::session::TimelineTick(0),
            48_000,
            4_800,
        ));
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
        assert_eq!(error.0.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&current).unwrap(), original);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_to_load_an_unsupported_format_without_touching_the_original() {
        let root = test_root("unsupported");
        let store = SessionStore::new(&root);
        store.ensure_layout().unwrap();
        let current = root.join("scratch/current.json");
        let payload = br#"{"formatVersion":99,"sessionId":"future","updatedAtMs":1}"#;
        fs::write(&current, payload).unwrap();

        let error = store.load_or_create().unwrap_err();
        assert_eq!(error.0.kind(), io::ErrorKind::InvalidData);
        // Unsupported format must surface as a load error and never overwrite
        // the original file.
        assert_eq!(fs::read(&current).unwrap(), payload);
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
    fn loads_and_round_trips_a_v3_session() {
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
    fn save_rejects_a_session_with_an_unknown_asset_reference() {
        let root = test_root("save-unknown-asset");
        let store = SessionStore::new(&root);
        let mut session = CreativeSession::new(now_ms());
        push_audio_clip(&mut session, "clip:unknown", "unknown", mint_asset_id());

        let error = store.save(&session).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!root.join("scratch/current.json").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_rejects_a_current_session_with_an_unknown_asset_reference() {
        let root = test_root("load-unknown-asset");
        let store = SessionStore::new(&root);
        store.ensure_layout().unwrap();
        let current = root.join("scratch/current.json");

        let mut session = CreativeSession::new(now_ms());
        push_audio_clip(&mut session, "clip:unknown", "unknown", mint_asset_id());
        let payload = serde_json::to_vec_pretty(&session).unwrap();
        fs::write(&current, &payload).unwrap();
        let original = fs::read(&current).unwrap();

        // Load must reject the unknown reference rather than adopting an
        // unrecoverable session, and must never overwrite the original file.
        let error = store.load_or_create().unwrap_err();
        assert_eq!(error.0.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&current).unwrap(), original);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_falls_back_to_a_valid_generation_when_current_has_unknown_asset() {
        let root = test_root("load-fallback-generation");
        let store = SessionStore::new(&root);
        store.ensure_layout().unwrap();

        // A valid generation that references no external asset.
        let mut generation = CreativeSession::new(now_ms());
        generation.project_name = Some("Recoverable".into());
        let generation_payload = serde_json::to_vec_pretty(&generation).unwrap();
        fs::write(
            root.join("scratch/generations").join(format!(
                "{}-{}.json",
                now_ms(),
                std::process::id()
            )),
            &generation_payload,
        )
        .unwrap();

        // Current session references an unknown asset and must be rejected.
        let current = root.join("scratch/current.json");
        let mut session = CreativeSession::new(now_ms());
        push_audio_clip(&mut session, "clip:unknown", "unknown", mint_asset_id());
        fs::write(&current, serde_json::to_vec_pretty(&session).unwrap()).unwrap();

        let loaded = store.load_or_create().unwrap();
        assert!(loaded.recovered_from_generation);
        assert_eq!(loaded.session.project_name.as_deref(), Some("Recoverable"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn save_accepts_a_registered_asset_when_its_content_file_is_missing() {
        let root = test_root("save-missing-content");
        let wav = root.join("take.wav");
        write_wav(&wav);
        let asset_id = crate::asset::register(
            &root,
            AssetKind::Audio,
            "take",
            &wav.to_string_lossy(),
            None,
        )
        .unwrap();
        std::fs::remove_file(&wav).unwrap();

        let mut session = CreativeSession::new(now_ms());
        push_audio_clip(&mut session, "clip:asset-backed", "asset-backed", asset_id);
        SessionStore::new(&root).save(&session).unwrap();
        assert!(root.join("scratch/current.json").is_file());
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
        assert!(session.arrangement.tracks.is_empty());
        assert_eq!(session.rack.devices.len(), 3);
        assert_eq!(session.rack.devices[0].kind, DeviceKind::Input);
    }
}
