use serde::{Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::path::PathBuf;

use crate::LocalControlEndpoint;

/// Maximum encoded payload accepted for one local control frame.
pub const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

/// A stream that can carry framed control messages.
pub trait ReadWrite: Read + Write + Send + Sync {
    /// Clones the underlying OS stream so a separate owner can close it.
    fn try_clone_stream(&self) -> io::Result<Box<dyn ReadWrite>>;

    /// Interrupts a blocked read on the underlying local connection.
    fn close_stream(&self);
}

#[cfg(unix)]
impl ReadWrite for std::os::unix::net::UnixStream {
    fn try_clone_stream(&self) -> io::Result<Box<dyn ReadWrite>> {
        Ok(Box::new(std::os::unix::net::UnixStream::try_clone(self)?))
    }

    fn close_stream(&self) {
        let _ = self.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(windows)]
impl ReadWrite for std::fs::File {
    fn try_clone_stream(&self) -> io::Result<Box<dyn ReadWrite>> {
        Ok(Box::new(std::fs::File::try_clone(self)?))
    }

    fn close_stream(&self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::IO::CancelIoEx;

        unsafe {
            let _ = CancelIoEx(self.as_raw_handle() as _, std::ptr::null());
        }
    }
}

/// Errors raised by framing or local transport.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("control transport I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("control frame exceeds the {limit}-byte limit: {size} bytes")]
    FrameTooLarge { size: usize, limit: usize },
    #[error("control frame is not valid UTF-8")]
    InvalidUtf8,
    #[error("control frame is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("control transport is unavailable on this platform")]
    UnsupportedPlatform,
}

/// Writes one length-prefixed JSON frame.
pub fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), TransportError>
where
    W: Write,
    T: Serialize,
{
    let payload = serde_json::to_vec(value)?;
    if payload.len() > MAX_FRAME_SIZE {
        return Err(TransportError::FrameTooLarge {
            size: payload.len(),
            limit: MAX_FRAME_SIZE,
        });
    }
    let length = u32::try_from(payload.len()).map_err(|_| TransportError::FrameTooLarge {
        size: payload.len(),
        limit: MAX_FRAME_SIZE,
    })?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Reads one length-prefixed JSON frame.
pub fn read_frame<R, T>(reader: &mut R) -> Result<T, TransportError>
where
    R: Read,
    T: DeserializeOwned,
{
    let mut length_bytes = [0; 4];
    reader.read_exact(&mut length_bytes)?;
    let size = u32::from_le_bytes(length_bytes) as usize;
    if size > MAX_FRAME_SIZE {
        return Err(TransportError::FrameTooLarge {
            size,
            limit: MAX_FRAME_SIZE,
        });
    }
    let mut payload = vec![0; size];
    reader.read_exact(&mut payload)?;
    let payload = String::from_utf8(payload).map_err(|_| TransportError::InvalidUtf8)?;
    Ok(serde_json::from_str(&payload)?)
}

/// Connects to a host-local endpoint.
pub fn connect(endpoint: &LocalControlEndpoint) -> Result<Box<dyn ReadWrite>, TransportError> {
    match endpoint {
        #[cfg(windows)]
        LocalControlEndpoint::WindowsNamedPipe { name } => {
            let stream = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(name)?;
            Ok(Box::new(stream))
        }
        #[cfg(unix)]
        LocalControlEndpoint::UnixSocket { path } => {
            Ok(Box::new(std::os::unix::net::UnixStream::connect(path)?))
        }
        _ => Err(TransportError::UnsupportedPlatform),
    }
}

/// A platform-neutral listener for the local Host control endpoint.
pub enum LocalControlListener {
    #[cfg(windows)]
    Windows(NamedPipeListener),
    #[cfg(unix)]
    Unix(UnixDomainListener),
}

impl LocalControlListener {
    /// Binds the endpoint and removes an inactive stale Unix socket.
    pub fn bind(endpoint: &LocalControlEndpoint) -> Result<Self, TransportError> {
        match endpoint {
            #[cfg(windows)]
            LocalControlEndpoint::WindowsNamedPipe { name } => {
                Ok(Self::Windows(NamedPipeListener::bind(name)?))
            }
            #[cfg(unix)]
            LocalControlEndpoint::UnixSocket { path } => {
                Ok(Self::Unix(UnixDomainListener::bind(path)?))
            }
            _ => Err(TransportError::UnsupportedPlatform),
        }
    }

    /// Accepts the next local client.
    pub fn accept(&mut self) -> Result<Box<dyn ReadWrite>, TransportError> {
        match self {
            #[cfg(windows)]
            Self::Windows(listener) => Ok(Box::new(listener.accept()?)),
            #[cfg(unix)]
            Self::Unix(listener) => Ok(Box::new(listener.accept()?)),
        }
    }
}

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::NamedPipeListener;

#[cfg(unix)]
pub struct UnixDomainListener {
    listener: std::os::unix::net::UnixListener,
    path: PathBuf,
}

#[cfg(unix)]
impl UnixDomainListener {
    fn bind(path: &std::path::Path) -> io::Result<Self> {
        if path.exists() {
            match std::os::unix::net::UnixStream::connect(path) {
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "Riffra Host control socket is already in use",
                    ));
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    std::fs::remove_file(path)?;
                }
                Err(error) => return Err(error),
            }
        }
        Self::prepare_socket_parent(path)?;
        let listener = std::os::unix::net::UnixListener::bind(path)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }

    fn accept(&self) -> io::Result<std::os::unix::net::UnixStream> {
        self.listener.accept().map(|(stream, _)| stream)
    }

    fn prepare_socket_parent(path: &std::path::Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Unix socket has no parent")
        })?;
        let existed = parent.exists();
        std::fs::create_dir_all(parent)?;
        use std::os::unix::fs::PermissionsExt;
        let private_name = parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name == "control" || name == "riffra" || name.starts_with("riffra-")
            });
        if !existed || private_name {
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for UnixDomainListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn partial_reads_round_trip_one_frame() {
        let mut encoded = Cursor::new(Vec::new());
        write_frame(&mut encoded, &serde_json::json!({"request": 1})).unwrap();
        let mut bytes = encoded.into_inner();
        let split = bytes.len() / 2;
        let second = bytes.split_off(split);
        let mut reader = Chunks::new(vec![bytes, second]);

        let value: serde_json::Value = read_frame(&mut reader).unwrap();
        assert_eq!(value["request"], 1);
    }

    #[test]
    fn sequential_frames_are_read_in_order() {
        let mut encoded = Cursor::new(Vec::new());
        write_frame(&mut encoded, &serde_json::json!({"request": 1})).unwrap();
        write_frame(&mut encoded, &serde_json::json!({"request": 2})).unwrap();
        let mut reader = Cursor::new(encoded.into_inner());

        let first: serde_json::Value = read_frame(&mut reader).unwrap();
        let second: serde_json::Value = read_frame(&mut reader).unwrap();

        assert_eq!(first["request"], 1);
        assert_eq!(second["request"], 2);
    }

    #[test]
    fn exact_frame_limit_is_accepted() {
        let value = "x".repeat(MAX_FRAME_SIZE - 2);
        let mut encoded = Cursor::new(Vec::new());
        write_frame(&mut encoded, &value).unwrap();
        let mut reader = Cursor::new(encoded.into_inner());

        let decoded: String = read_frame(&mut reader).unwrap();

        assert_eq!(decoded.len(), MAX_FRAME_SIZE - 2);
    }

    #[test]
    fn oversized_frame_is_rejected_before_allocation() {
        let size = (MAX_FRAME_SIZE as u32 + 1).to_le_bytes();
        let mut reader = Cursor::new(size);

        let error = read_frame::<_, serde_json::Value>(&mut reader).unwrap_err();
        assert!(matches!(error, TransportError::FrameTooLarge { .. }));
    }

    #[test]
    fn invalid_utf8_frame_is_rejected() {
        let mut reader = Cursor::new([1, 0, 0, 0, 0xff]);

        let error = read_frame::<_, serde_json::Value>(&mut reader).unwrap_err();

        assert!(matches!(error, TransportError::InvalidUtf8));
    }

    #[test]
    fn invalid_json_frame_is_rejected() {
        let mut reader = Cursor::new([1, 0, 0, 0, b'{']);

        let error = read_frame::<_, serde_json::Value>(&mut reader).unwrap_err();

        assert!(matches!(error, TransportError::InvalidJson(_)));
    }

    #[cfg(unix)]
    #[test]
    fn unix_domain_socket_round_trips_and_reconnects() {
        let path = std::env::temp_dir().join(format!(
            "riffra-control-test-{}-{}.sock",
            std::process::id(),
            crate::new_instance_id()
        ));
        let endpoint = crate::LocalControlEndpoint::UnixSocket { path: path.clone() };
        let mut listener = LocalControlListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            for expected in [1, 2] {
                let mut stream = listener.accept().unwrap();
                let request: serde_json::Value = read_frame(&mut stream).unwrap();
                assert_eq!(request["request"], expected);
                write_frame(&mut stream, &serde_json::json!({"request": expected})).unwrap();
            }
        });
        for expected in [1, 2] {
            let mut stream = connect(&endpoint).unwrap();
            write_frame(&mut stream, &serde_json::json!({"request": expected})).unwrap();
            let response: serde_json::Value = read_frame(&mut stream).unwrap();
            assert_eq!(response["request"], expected);
        }
        server.join().unwrap();
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn unix_domain_socket_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!(
            "riffra-control-permissions-{}-{}.sock",
            std::process::id(),
            crate::new_instance_id()
        ));
        let endpoint = crate::LocalControlEndpoint::UnixSocket { path: path.clone() };
        let listener = LocalControlListener::bind(&endpoint).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(listener);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn stale_unix_domain_socket_is_replaced() {
        let path = std::env::temp_dir().join(format!(
            "riffra-control-stale-{}-{}.sock",
            std::process::id(),
            crate::new_instance_id()
        ));
        let endpoint = crate::LocalControlEndpoint::UnixSocket { path: path.clone() };
        let stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
        drop(stale);
        let second = LocalControlListener::bind(&endpoint).unwrap();
        drop(second);
    }

    struct Chunks {
        chunks: Vec<Vec<u8>>,
        offset: usize,
    }

    impl Chunks {
        fn new(chunks: Vec<Vec<u8>>) -> Self {
            Self { chunks, offset: 0 }
        }
    }

    impl Read for Chunks {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            while self.offset >= self.chunks.first().map_or(0, Vec::len) {
                if self.chunks.is_empty() {
                    return Ok(0);
                }
                self.chunks.remove(0);
                self.offset = 0;
            }
            let chunk = &self.chunks[0];
            let amount = buffer.len().min(chunk.len() - self.offset);
            buffer[..amount].copy_from_slice(&chunk[self.offset..self.offset + amount]);
            self.offset += amount;
            Ok(amount)
        }
    }
}
