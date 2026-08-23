use serde::{Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};

/// Maximum encoded payload accepted for one local control frame.
pub const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

/// A stream that can carry framed control messages.
pub trait ReadWrite: Read + Write + Send {}

impl<T> ReadWrite for T where T: Read + Write + Send {}

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

/// Connects to a named-pipe endpoint on Windows.
#[cfg(windows)]
pub fn connect(pipe_name: &str) -> Result<Box<dyn ReadWrite>, TransportError> {
    let stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name)?;
    Ok(Box::new(stream))
}

/// Named pipes are intentionally not implemented for non-Windows Desktop Attach.
#[cfg(not(windows))]
pub fn connect(_pipe_name: &str) -> Result<Box<dyn ReadWrite>, TransportError> {
    Err(TransportError::UnsupportedPlatform)
}

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::NamedPipeListener;

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
