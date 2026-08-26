use crate::{
    ControlCommand, ControlRequest, EndpointDescriptor, LocalControlEndpoint, LocalHostClient,
    LocalHostClientError,
};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

const MAX_REGISTRATION_SIZE: u64 = 64 * 1024;

/// A registry entry describing one Host owned by the current OS user.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalHostRegistration {
    pub instance_id: String,
    pub pid: u32,
    pub data_root: PathBuf,
    pub endpoint: LocalControlEndpoint,
    pub started_at_ms: u64,
}

impl LocalHostRegistration {
    /// Builds a registry entry from the endpoint published by a Host.
    pub fn from_descriptor(
        data_root: impl Into<PathBuf>,
        descriptor: &EndpointDescriptor,
        started_at_ms: u64,
    ) -> Self {
        Self {
            instance_id: descriptor.instance_id.clone(),
            pid: descriptor.pid,
            data_root: data_root.into(),
            endpoint: descriptor.endpoint.clone(),
            started_at_ms,
        }
    }

    pub(crate) fn descriptor(&self) -> EndpointDescriptor {
        EndpointDescriptor {
            instance_id: self.instance_id.clone(),
            pid: self.pid,
            endpoint: self.endpoint.clone(),
        }
    }
}

/// A verified Host returned by local discovery.
#[derive(Debug)]
pub struct LocalHostDiscovery {
    pub registration: LocalHostRegistration,
    pub client: LocalHostClient,
}

/// Current-user local Host registry.
#[derive(Clone, Debug)]
pub struct LocalHostRegistry {
    root: PathBuf,
}

impl LocalHostRegistry {
    /// Resolves the OS-specific current-user registry root.
    pub fn current_user() -> Self {
        #[cfg(windows)]
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Riffra")
            .join("hosts");

        #[cfg(unix)]
        let root = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join(format!("riffra-{}", current_user_id())))
            .join("riffra")
            .join("hosts");

        Self { root }
    }

    /// Creates a registry rooted at an explicit directory, primarily for
    /// composition roots and process-level tests.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the directory containing one JSON entry per Host.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Publishes or replaces one Host registration atomically.
    pub fn register(&self, registration: &LocalHostRegistration) -> Result<(), String> {
        self.ensure_root()?;
        let path = self.entry_path(&registration.instance_id)?;
        let temporary = self.root.join(format!(
            ".{}.{}.tmp",
            registration.instance_id,
            std::process::id()
        ));
        let bytes = serde_json::to_vec_pretty(registration)
            .map_err(|error| format!("local Host registry entry could not be encoded: {error}"))?;
        if let Err(error) = std::fs::write(&temporary, bytes) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!(
                "local Host registry entry could not be written: {error}"
            ));
        }
        if let Err(error) = set_private_file(&temporary) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = replace_entry(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!(
                "local Host registry entry could not be published: {error}"
            ));
        }
        Ok(())
    }

    /// Removes one exact Host registration. Missing entries are already gone.
    pub fn unregister(&self, instance_id: &str) -> Result<(), String> {
        let path = self.entry_path(instance_id)?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "local Host registry entry could not be removed: {error}"
            )),
        }
    }

    /// Reads all syntactically valid entries without trusting their liveness.
    pub fn entries(&self) -> Result<Vec<LocalHostRegistration>, String> {
        let directory = match std::fs::read_dir(&self.root) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!("local Host registry could not be read: {error}"));
            }
        };
        let mut registrations = Vec::new();
        for entry in directory {
            let entry = entry
                .map_err(|error| format!("local Host registry entry could not be read: {error}"))?;
            if !entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            if entry
                .metadata()
                .map(|metadata| metadata.len() > MAX_REGISTRATION_SIZE)
                .unwrap_or(true)
            {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
            };
            match serde_json::from_slice::<LocalHostRegistration>(&bytes) {
                Ok(registration)
                    if path.file_stem().and_then(|stem| stem.to_str())
                        == Some(registration.instance_id.as_str()) =>
                {
                    registrations.push(registration)
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                }
                Ok(_) => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        registrations.sort_by_key(|registration| registration.started_at_ms);
        Ok(registrations)
    }

    /// Verifies each entry through a live command round trip.
    ///
    /// Entries are removed only when they are provably invalid: a dead owner
    /// process, or a handshake that identifies a different Host instance than
    /// the entry claims. A Host that is merely unreachable right now stays
    /// registered and is left out of this discovery result instead, so a
    /// busy or starting Host does not disappear from the Host Selector.
    pub fn discover(&self) -> Result<Vec<LocalHostDiscovery>, String> {
        let mut discovered = Vec::new();
        for registration in self.entries()? {
            if !process_exists(registration.pid) {
                let _ = self.unregister(&registration.instance_id);
                continue;
            }
            match self.verify_registration(&registration) {
                Ok(client) => discovered.push(LocalHostDiscovery {
                    registration,
                    client,
                }),
                Err(LocalHostClientError::Handshake(_)) => {
                    // The endpoint answers, but it is not the Host this entry
                    // describes. The entry can never become valid again.
                    let _ = self.unregister(&registration.instance_id);
                }
                Err(_unavailable) => {
                    // Unavailable now is not stale; keep the entry registered.
                }
            }
        }
        Ok(discovered)
    }

    fn verify_registration(
        &self,
        registration: &LocalHostRegistration,
    ) -> Result<LocalHostClient, LocalHostClientError> {
        let client = LocalHostClient::connect_registration(registration);
        let request = ControlRequest::new(
            format!("discovery-{}", registration.instance_id),
            ControlCommand::new("host.status", serde_json::json!({})),
            None,
        );
        let response = client.request(&request)?;
        verify_host_status(registration, &response).map_err(LocalHostClientError::Handshake)?;
        Ok(client)
    }

    fn ensure_root(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|error| {
            format!("local Host registry directory could not be created: {error}")
        })?;
        let mut directory = self.root.as_path();
        let mut is_root = true;
        loop {
            let is_private_parent = directory
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "riffra" || name.starts_with("riffra-"));
            if !is_root && !is_private_parent {
                break;
            }
            set_private_directory(directory)?;
            is_root = false;
            let Some(parent) = directory.parent() else {
                break;
            };
            directory = parent;
        }
        Ok(())
    }

    fn entry_path(&self, instance_id: &str) -> Result<PathBuf, String> {
        if instance_id.is_empty()
            || !instance_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err("local Host registry instance id is invalid".into());
        }
        Ok(self.root.join(format!("{instance_id}.json")))
    }
}

fn verify_host_status(
    registration: &LocalHostRegistration,
    response: &crate::ControlResponse,
) -> Result<(), String> {
    if !response.ok {
        return Err("Host status request was rejected".into());
    }
    let result = response
        .result
        .as_ref()
        .ok_or_else(|| "Host status response did not contain a result".to_string())?;
    if result.result_type != "hostStatus" {
        return Err("Host status response had an unexpected result type".into());
    }
    let instance_id = result
        .value
        .get("instanceId")
        .and_then(serde_json::Value::as_str);
    if instance_id != Some(registration.instance_id.as_str()) {
        return Err("Host status response did not match the registry instance".into());
    }
    let pid = result
        .value
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok());
    if pid != Some(registration.pid) {
        return Err("Host status response did not match the registry process".into());
    }
    Ok(())
}

/// Returns whether the operating system still has a live process for `pid`.
fn process_exists(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: OpenProcess only reads the pid and the access mask, and the
        // returned handle is released before returning.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            false
        } else {
            unsafe {
                let _ = CloseHandle(handle);
            }
            true
        }
    }

    #[cfg(unix)]
    {
        // SAFETY: kill with signal 0 only probes for process existence.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        true
    }
}

/// Returns the current Unix timestamp in milliseconds for registry ordering.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(unix)]
fn current_user_id() -> u32 {
    // SAFETY: geteuid has no preconditions and does not borrow memory.
    unsafe { libc::geteuid() }
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
        format!("local Host registry directory permissions could not be set: {error}")
    })
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("local Host registry entry permissions could not be set: {error}"))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn replace_entry(temporary: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = temporary
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_entry(temporary: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(temporary, destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_instance_id;

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("riffra-registry-{name}-{}", now_ms()))
    }

    #[test]
    fn registration_round_trips_and_is_owner_only_on_unix() {
        let root = root("round-trip");
        let registry = LocalHostRegistry::at(&root);
        let descriptor = EndpointDescriptor::new("instance-1", 42);
        let registration = LocalHostRegistration::from_descriptor(&root, &descriptor, 7);

        registry.register(&registration).unwrap();

        assert_eq!(registry.entries().unwrap(), vec![registration]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(root.join("instance-1.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        registry.unregister("instance-1").unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_entries_are_removed_during_discovery() {
        let root = root("stale");
        let registry = LocalHostRegistry::at(&root);
        registry.ensure_root().unwrap();
        std::fs::write(root.join("stale.json"), b"not-json").unwrap();

        assert!(registry.discover().unwrap().is_empty());
        assert!(!root.join("stale.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn entries_without_a_live_process_are_removed() {
        let root = root("dead-pid");
        let registry = LocalHostRegistry::at(&root);
        let descriptor = EndpointDescriptor::new(new_instance_id(), 42);
        let registration = LocalHostRegistration::from_descriptor(&root, &descriptor, 1);
        registry.register(&registration).unwrap();
        assert!(!process_exists(42));

        assert!(registry.discover().unwrap().is_empty());
        assert_eq!(
            registry.entries().unwrap(),
            Vec::<LocalHostRegistration>::new()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_temporarily_unreachable_host_stays_registered_but_is_not_discovered() {
        let root = root("unreachable");
        let registry = LocalHostRegistry::at(&root);
        // A live pid whose endpoint was never bound: the Host is not reachable
        // right now, but the entry itself carries no provable defect.
        let registration = LocalHostRegistration::from_descriptor(
            &root,
            &EndpointDescriptor::new(new_instance_id(), std::process::id()),
            1,
        );
        registry.register(&registration).unwrap();

        assert!(registry.discover().unwrap().is_empty());
        assert_eq!(registry.entries().unwrap(), vec![registration.clone()]);
        registry.unregister(&registration.instance_id).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }
}
