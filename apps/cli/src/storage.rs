use riffra_core::{CreativeSession, PortError, SessionStorage, deserialize_session};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct SessionFileStorage {
    path: PathBuf,
}

impl SessionFileStorage {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_or_new(&self) -> Result<CreativeSession, String> {
        if !self.path.exists() {
            let session = CreativeSession::new(now_ms());
            self.save(&session)
                .map_err(|error| format!("new session could not be saved: {error}"))?;
            return Ok(session);
        }
        let payload = fs::read(&self.path)
            .map_err(|error| format!("session file could not be read: {error}"))?;
        deserialize_session(&payload)
            .map_err(|error| format!("session file is invalid: {error}"))?
            .validate_and_normalize()
            .map_err(|error| format!("session file is invalid: {error}"))
    }
}

impl SessionStorage for SessionFileStorage {
    fn save(&self, session: &CreativeSession) -> Result<(), PortError> {
        let normalized = session
            .clone()
            .validate_and_normalize()
            .map_err(PortError::Storage)?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| PortError::Storage("session file has no parent directory".into()))?;
        fs::create_dir_all(parent).map_err(|error| {
            PortError::Storage(format!("session folder could not be created: {error}"))
        })?;
        let temporary = self.path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(&normalized).map_err(|error| {
            PortError::Storage(format!("session could not be encoded: {error}"))
        })?;
        fs::write(&temporary, payload).map_err(|error| {
            PortError::Storage(format!("session could not be written: {error}"))
        })?;
        replace_file(&temporary, &self.path)
            .map_err(|error| PortError::Storage(format!("session could not be finalized: {error}")))
    }
}

#[cfg(windows)]
fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
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
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
}

fn now_ms() -> u64 {
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

    #[test]
    fn missing_session_is_saved_before_first_command() {
        let nonce = now_ms();
        let root = std::env::temp_dir().join(format!("riffra-cli-storage-{nonce}"));
        let path = root.join("session.json");
        let storage = SessionFileStorage::new(path.clone());

        let first = storage
            .load_or_new()
            .expect("new session should be created");
        let second = storage
            .load_or_new()
            .expect("saved session should be readable");

        assert!(path.exists());
        assert_eq!(first.session_id, second.session_id);
        let _ = fs::remove_dir_all(root);
    }
}
