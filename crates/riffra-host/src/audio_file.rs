//! Pure WAV structure parsing used by timeline placement and analysis.

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// Structural metadata for a RIFF/WAVE file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WavMetadata {
    /// WAVE format tag, such as PCM (1) or IEEE float (3).
    pub format: u16,
    /// Number of interleaved audio channels.
    pub channels: u16,
    /// Samples per second.
    pub sample_rate: u32,
    /// Bits per sample in each channel.
    pub bits_per_sample: u16,
    /// Byte offset of the data payload in the source file.
    pub data_offset: usize,
    /// Length of the data payload in bytes.
    pub data_len: usize,
    /// Number of complete interleaved frames in the data payload.
    pub frame_count: u64,
}

/// Parses RIFF/WAVE chunks without decoding samples.
///
/// # Errors
/// Returns an error when the RIFF header, `fmt ` chunk, `data` chunk, or chunk
/// boundaries are malformed.
pub fn parse_wav(bytes: &[u8]) -> Result<WavMetadata, String> {
    read_structure(&mut Cursor::new(bytes), bytes.len() as u64)
}

fn read_u16(bytes: &[u8]) -> Result<u16, String> {
    bytes
        .get(..2)
        .ok_or_else(|| "WAV value is truncated.".to_string())
        .and_then(|value| {
            value
                .try_into()
                .map(u16::from_le_bytes)
                .map_err(|_| "WAV value is truncated.".into())
        })
}

fn wav_format(fmt: &[u8]) -> Result<u16, String> {
    let tag = read_u16(fmt)?;
    if tag != 0xfffe {
        return Ok(tag);
    }
    let subtype = fmt
        .get(24..40)
        .ok_or("extensible wav format is truncated")?;
    if subtype[2..] != [0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71] {
        return Err("extensible wav subtype is unsupported".into());
    }
    read_u16(subtype)
}

/// Reads the sample rate and frame count without loading sample data.
///
/// # Errors
/// Returns an error for unreadable files, malformed chunks, or incomplete frames.
pub fn read_wav_metadata(path: &Path) -> Result<(u32, u64), String> {
    let wav = read_wav_structure(path)?;
    Ok((wav.sample_rate, wav.frame_count))
}

/// Reads WAV structure and sample offsets without loading the audio payload.
///
/// # Errors
/// Returns an error for unreadable files, malformed chunks, or incomplete frames.
pub fn read_wav_structure(path: &Path) -> Result<WavMetadata, String> {
    let mut file =
        File::open(path).map_err(|error| format!("audio could not be opened: {error}"))?;
    let file_len = file
        .metadata()
        .map_err(|error| format!("audio size could not be read: {error}"))?
        .len();
    read_structure(&mut file, file_len)
}

fn read_structure(reader: &mut (impl Read + Seek), file_len: u64) -> Result<WavMetadata, String> {
    let mut header = [0_u8; 12];
    reader
        .read_exact(&mut header)
        .map_err(|error| format!("audio header could not be read: {error}"))?;
    if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err("audio is not a RIFF/WAVE file.".into());
    }
    let mut channels = None;
    let mut format = None;
    let mut data_offset = None;
    let mut sample_rate = None;
    let mut bits_per_sample = None;
    let mut data_len = None;
    let mut chunk_header = [0_u8; 8];
    loop {
        let chunk_start = reader
            .stream_position()
            .map_err(|error| format!("audio position could not be read: {error}"))?;
        if chunk_start == file_len {
            break;
        }
        if file_len.saturating_sub(chunk_start) < 8 {
            return Err("audio has a truncated chunk header.".into());
        }
        reader
            .read_exact(&mut chunk_header)
            .map_err(|error| format!("audio chunk header could not be read: {error}"))?;
        let chunk_len = u64::from(u32::from_le_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]));
        let payload_end = chunk_start
            .checked_add(8)
            .and_then(|position| position.checked_add(chunk_len))
            .and_then(|position| position.checked_add(chunk_len % 2))
            .ok_or_else(|| "audio chunk length overflows the file range.".to_string())?;
        if payload_end > file_len {
            return Err("audio chunk extends past the end of the file.".into());
        }
        match &chunk_header[..4] {
            b"fmt " if chunk_len >= 16 => {
                let mut fmt = [0_u8; 40];
                reader
                    .read_exact(&mut fmt[..16])
                    .map_err(|error| format!("audio format could not be read: {error}"))?;
                channels = Some(u16::from_le_bytes([fmt[2], fmt[3]]));
                if u16::from_le_bytes([fmt[0], fmt[1]]) == 0xfffe {
                    if chunk_len < 40 {
                        return Err("extensible wav format is truncated".into());
                    }
                    reader
                        .read_exact(&mut fmt[16..])
                        .map_err(|error| format!("audio subtype could not be read: {error}"))?;
                }
                format = Some(wav_format(&fmt)?);
                sample_rate = Some(u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]));
                bits_per_sample = Some(u16::from_le_bytes([fmt[14], fmt[15]]));
            }
            b"data" => {
                data_len = Some(chunk_len);
                data_offset = Some(chunk_start + 8);
            }
            _ => {}
        }
        reader
            .seek(SeekFrom::Start(payload_end))
            .map_err(|error| format!("audio chunk could not be skipped: {error}"))?;
    }
    let channels = channels.unwrap_or_default();
    let sample_rate = sample_rate.unwrap_or_default();
    let bits_per_sample = bits_per_sample.unwrap_or_default();
    let data_len = data_len.ok_or_else(|| "audio has no data chunk.".to_string())?;
    let frame_bytes = u64::from(channels)
        .checked_mul(u64::from(bits_per_sample / 8))
        .filter(|_| channels > 0 && bits_per_sample > 0 && bits_per_sample % 8 == 0)
        .ok_or_else(|| "audio has an invalid frame format.".to_string())?;
    if sample_rate == 0 {
        return Err("audio has no sample rate.".into());
    }
    if data_len % frame_bytes != 0 {
        return Err("audio data does not contain complete frames.".into());
    }
    Ok(WavMetadata {
        format: format.ok_or("audio has no format")?,
        channels,
        sample_rate,
        bits_per_sample,
        data_offset: usize::try_from(data_offset.ok_or("audio has no sample offset")?)
            .map_err(|_| "audio sample offset exceeds address range")?,
        data_len: usize::try_from(data_len).map_err(|_| "audio payload exceeds address range")?,
        frame_count: data_len / frame_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_wav;

    fn wav(data: &[u8], channels: u16, sample_rate: u32, bits: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        let riff_size = 4 + 8 + 16 + 8 + data.len();
        bytes.extend_from_slice(&(riff_size as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        let frame_bytes = usize::from(channels) * usize::from(bits / 8);
        bytes.extend_from_slice(&(sample_rate * frame_bytes as u32).to_le_bytes());
        bytes.extend_from_slice(&(frame_bytes as u16).to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn parses_structural_metadata_and_frame_count() {
        let pcm = wav(&[0; 16], 2, 48_000, 16);
        let mut float = wav(&[0; 16], 2, 48_000, 32);
        let size = float.len() as u32 - 8 + 24;
        float[4..8].copy_from_slice(&size.to_le_bytes());
        float[16..20].copy_from_slice(&40_u32.to_le_bytes());
        float[20..22].copy_from_slice(&0xfffe_u16.to_le_bytes());
        float.splice(
            36..36,
            [
                22, 0, 32, 0, 3, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38,
                0x9b, 0x71,
            ],
        );
        for (bytes, format, frames) in [(pcm, 1, 4), (float, 3, 2)] {
            let metadata = parse_wav(&bytes).unwrap();
            assert_eq!(metadata.sample_rate, 48_000);
            assert_eq!(metadata.frame_count, frames);
            assert_eq!(metadata.format, format);
            let path = std::env::temp_dir().join(format!(
                "riffra-wav-structure-{}-{format}.wav",
                std::process::id()
            ));
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(super::read_wav_structure(&path).unwrap(), metadata);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn rejects_broken_wav() {
        // Arrange
        let mut truncated = wav(&[0; 4], 2, 48_000, 16);
        truncated.pop();
        let mut trailing_header = wav(&[0; 4], 2, 48_000, 16);
        trailing_header.push(0);
        let cases = [
            b"not wav".to_vec(),
            truncated,
            trailing_header,
            wav(&[0; 6], 2, 48_000, 16),
            wav(&[0; 4], 0, 48_000, 16),
            wav(&[0; 4], 2, 0, 16),
            wav(&[0; 4], 2, 48_000, 12),
        ];
        let path =
            std::env::temp_dir().join(format!("riffra-broken-wav-{}.wav", std::process::id()));
        for bytes in cases {
            std::fs::write(&path, &bytes).unwrap();

            // Act
            let from_bytes = parse_wav(&bytes);
            let from_file = super::read_wav_structure(&path);

            // Assert
            assert!(from_bytes.is_err());
            assert_eq!(from_bytes, from_file);
        }
        std::fs::remove_file(path).unwrap();
    }
}
