use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// Identity allocated once for one live Host process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostIdentity {
    pub instance_id: String,
    pub pid: u32,
}

impl HostIdentity {
    /// Allocates a process identity for a new Host composition.
    pub fn new() -> Self {
        Self {
            instance_id: crate::new_instance_id(),
            pid: std::process::id(),
        }
    }
}

impl Default for HostIdentity {
    fn default() -> Self {
        Self::new()
    }
}
use uuid::Uuid;

/// Local transport used by one live Riffra Host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LocalControlEndpoint {
    /// Current-user-scoped Windows Named Pipe.
    WindowsNamedPipe { name: String },
    /// Owner-only Unix Domain Socket.
    UnixSocket { path: PathBuf },
}

/// Filesystem descriptor used to discover one running Riffra Host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDescriptor {
    pub instance_id: String,
    pub pid: u32,
    pub endpoint: LocalControlEndpoint,
}

impl EndpointDescriptor {
    /// Creates a descriptor using the platform's default local endpoint.
    pub fn new(instance_id: impl Into<String>, pid: u32) -> Self {
        let instance_id = instance_id.into();
        Self {
            endpoint: default_endpoint(&instance_id),
            instance_id,
            pid,
        }
    }

    /// Creates a descriptor whose Unix socket is placed below the data root.
    ///
    /// A Unix socket has a small platform-defined path limit. If the data
    /// root would make the socket path too long, the endpoint is placed in a
    /// private temporary directory and the actual path is recorded in the
    /// descriptor.
    pub fn for_data_root(data_root: &Path, instance_id: impl Into<String>, pid: u32) -> Self {
        let instance_id = instance_id.into();
        let endpoint = if cfg!(windows) {
            LocalControlEndpoint::WindowsNamedPipe {
                name: pipe_name(&instance_id),
            }
        } else {
            let preferred = data_root.join("control").join("host.sock");
            if preferred.to_string_lossy().len() < unix_socket_path_limit() {
                LocalControlEndpoint::UnixSocket { path: preferred }
            } else {
                let fallback = fallback_socket_path(&instance_id);
                let path = if fallback.to_string_lossy().len() < unix_socket_path_limit() {
                    fallback
                } else {
                    std::env::temp_dir()
                        .join(format!("riffra-{instance_id}"))
                        .join("host.sock")
                };
                LocalControlEndpoint::UnixSocket { path }
            }
        };
        Self {
            instance_id,
            pid,
            endpoint,
        }
    }

    /// Returns the endpoint's transport-specific address.
    pub fn endpoint(&self) -> &LocalControlEndpoint {
        &self.endpoint
    }
}

fn default_endpoint(instance_id: &str) -> LocalControlEndpoint {
    if cfg!(windows) {
        LocalControlEndpoint::WindowsNamedPipe {
            name: pipe_name(instance_id),
        }
    } else {
        LocalControlEndpoint::UnixSocket {
            path: std::env::temp_dir().join(format!("riffra-{instance_id}.sock")),
        }
    }
}

fn fallback_socket_path(instance_id: &str) -> PathBuf {
    let private_root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|path| path.join("riffra"))
        .unwrap_or_else(|| std::env::temp_dir().join(format!("riffra-{instance_id}")));
    private_root.join(format!("host-{instance_id}.sock"))
}

#[cfg(unix)]
const fn unix_socket_path_limit() -> usize {
    // Linux exposes sun_path as 108 bytes. Keep a small margin for platforms
    // with a slightly smaller sockaddr_un field.
    100
}

#[cfg(not(unix))]
const fn unix_socket_path_limit() -> usize {
    usize::MAX
}

/// Returns the endpoint descriptor path under a Data Root.
pub fn endpoint_path(data_root: &Path) -> PathBuf {
    data_root.join("control").join("host.json")
}

/// Reads a published endpoint descriptor.
pub fn read_endpoint(data_root: &Path) -> Result<EndpointDescriptor, String> {
    let path = endpoint_path(data_root);
    let bytes = std::fs::read(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "Riffra Host control endpoint is unavailable: {}",
                path.display()
            )
        } else {
            format!("Riffra Host control endpoint could not be read: {error}")
        }
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("Riffra Host control endpoint is invalid: {error}"))
}

/// Publishes a descriptor using a temporary file and rename.
pub fn publish_endpoint(data_root: &Path, descriptor: &EndpointDescriptor) -> Result<(), String> {
    let path = endpoint_path(data_root);
    let directory = path
        .parent()
        .ok_or_else(|| "Riffra Host control endpoint has no parent directory".to_string())?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Riffra Host control directory could not be created: {error}"))?;
    set_private_directory(directory)?;
    let temporary = directory.join(format!("host.{}.tmp", descriptor.instance_id));
    let bytes = serde_json::to_vec_pretty(descriptor)
        .map_err(|error| format!("Riffra Host control endpoint could not be encoded: {error}"))?;
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("Riffra Host control endpoint could not be written: {error}"))?;
    set_private_file(&temporary)?;
    if let Err(error) = replace_endpoint(&temporary, &path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "Riffra Host control endpoint could not be published: {error}"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn replace_endpoint(temporary: &Path, destination: &Path) -> io::Result<()> {
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
fn replace_endpoint(temporary: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(temporary, destination)
}

/// Removes the descriptor only when it still identifies this Host instance.
pub fn remove_endpoint_if_matches(data_root: &Path, instance_id: &str) -> Result<(), String> {
    let path = endpoint_path(data_root);
    let Ok(descriptor) = std::fs::read(&path).and_then(|bytes| {
        serde_json::from_slice::<EndpointDescriptor>(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }) else {
        return Ok(());
    };
    if descriptor.instance_id == instance_id {
        std::fs::remove_file(path).map_err(|error| {
            format!("Riffra Host control endpoint could not be removed: {error}")
        })?;
        if let LocalControlEndpoint::UnixSocket { path } = descriptor.endpoint {
            let parent = path.parent().map(Path::to_path_buf);
            let _ = std::fs::remove_file(&path);
            if let Some(parent) = parent.as_deref()
                && parent.file_name().and_then(|name| name.to_str()) != Some("control")
            {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }
    Ok(())
}

/// Generates a new process-instance identifier.
pub fn new_instance_id() -> String {
    Uuid::now_v7().to_string()
}

/// Builds the user-scoped named pipe name for an instance.
pub fn pipe_name(instance_id: &str) -> String {
    format!(r"\\.\pipe\riffra-{instance_id}")
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
        format!("Riffra Host control directory permissions could not be set: {error}")
    })
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|error| {
        format!("Riffra Host control endpoint permissions could not be set: {error}")
    })
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_round_trips_instance_and_endpoint() {
        let descriptor = EndpointDescriptor::new("instance-1", 1234);
        let encoded = serde_json::to_string(&descriptor).unwrap();
        let decoded: EndpointDescriptor = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, descriptor);
        assert_eq!(decoded.instance_id, "instance-1");
        #[cfg(windows)]
        assert!(matches!(
            decoded.endpoint,
            LocalControlEndpoint::WindowsNamedPipe { .. }
        ));
        #[cfg(unix)]
        assert!(matches!(
            decoded.endpoint,
            LocalControlEndpoint::UnixSocket { .. }
        ));
    }

    #[test]
    fn publishing_a_new_instance_replaces_the_previous_descriptor() {
        let root = std::env::temp_dir().join(format!(
            "riffra-control-endpoint-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let first = EndpointDescriptor::new("first", 1);
        let second = EndpointDescriptor::new("second", 2);

        publish_endpoint(&root, &first).unwrap();
        publish_endpoint(&root, &second).unwrap();

        assert_eq!(read_endpoint(&root).unwrap(), second);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn long_data_root_uses_a_short_recorded_socket_path() {
        let data_root = PathBuf::from("/").join("r".repeat(160));
        let descriptor = EndpointDescriptor::for_data_root(&data_root, "long-instance", 1234);

        let LocalControlEndpoint::UnixSocket { path } = descriptor.endpoint else {
            panic!("Linux hosts must publish a Unix socket endpoint");
        };
        assert!(path.to_string_lossy().len() < unix_socket_path_limit());
        assert!(!path.starts_with(&data_root));
    }
}
