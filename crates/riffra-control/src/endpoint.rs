use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Filesystem descriptor used to discover one running Desktop instance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDescriptor {
    pub instance_id: String,
    pub pid: u32,
    pub pipe_name: String,
}

impl EndpointDescriptor {
    /// Creates a descriptor for the current process and instance id.
    pub fn new(instance_id: impl Into<String>, pid: u32) -> Self {
        let instance_id = instance_id.into();
        Self {
            pipe_name: pipe_name(&instance_id),
            instance_id,
            pid,
        }
    }
}

/// Returns the endpoint descriptor path under a Data Root.
pub fn endpoint_path(data_root: &Path) -> PathBuf {
    data_root.join("control").join("desktop.json")
}

/// Reads a published endpoint descriptor.
pub fn read_endpoint(data_root: &Path) -> Result<EndpointDescriptor, String> {
    let path = endpoint_path(data_root);
    let bytes = std::fs::read(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "Desktop control endpoint is unavailable: {}",
                path.display()
            )
        } else {
            format!("Desktop control endpoint could not be read: {error}")
        }
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("Desktop control endpoint is invalid: {error}"))
}

/// Publishes a descriptor using a temporary file and rename.
pub fn publish_endpoint(data_root: &Path, descriptor: &EndpointDescriptor) -> Result<(), String> {
    let path = endpoint_path(data_root);
    let directory = path
        .parent()
        .ok_or_else(|| "Desktop control endpoint has no parent directory".to_string())?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Desktop control directory could not be created: {error}"))?;
    let temporary = directory.join(format!("desktop.{}.tmp", descriptor.instance_id));
    let bytes = serde_json::to_vec_pretty(descriptor)
        .map_err(|error| format!("Desktop control endpoint could not be encoded: {error}"))?;
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("Desktop control endpoint could not be written: {error}"))?;
    if let Err(error) = replace_endpoint(&temporary, &path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "Desktop control endpoint could not be published: {error}"
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

/// Removes the descriptor only when it still identifies this Desktop instance.
pub fn remove_endpoint_if_matches(data_root: &Path, instance_id: &str) -> Result<(), String> {
    let path = endpoint_path(data_root);
    let Ok(descriptor) = std::fs::read(&path).and_then(|bytes| {
        serde_json::from_slice::<EndpointDescriptor>(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }) else {
        return Ok(());
    };
    if descriptor.instance_id == instance_id {
        std::fs::remove_file(path)
            .map_err(|error| format!("Desktop control endpoint could not be removed: {error}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_round_trips_instance_and_pipe() {
        let descriptor = EndpointDescriptor::new("instance-1", 1234);
        let encoded = serde_json::to_string(&descriptor).unwrap();
        let decoded: EndpointDescriptor = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, descriptor);
        assert!(decoded.pipe_name.ends_with("instance-1"));
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
}
