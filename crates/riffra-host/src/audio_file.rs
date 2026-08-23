//! Pure WAV structure parsing used by timeline placement and analysis.

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
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Audio file is not a RIFF/WAVE file.".into());
    }
    let mut cursor = 12_usize;
    let mut format = None;
    let mut data = None;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = read_u32(&bytes[cursor + 4..cursor + 8])? as usize;
        let start = cursor + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| "WAV chunk size overflowed.".to_string())?;
        if end > bytes.len() {
            return Err("WAV chunk exceeds the file boundary.".into());
        }
        if id == b"fmt " && size >= 16 {
            format = Some((
                read_u16(&bytes[start..start + 2])?,
                read_u16(&bytes[start + 2..start + 4])?,
                read_u32(&bytes[start + 4..start + 8])?,
                read_u16(&bytes[start + 14..start + 16])?,
            ));
        } else if id == b"data" {
            data = Some((start, size));
        }
        cursor = end
            .checked_add(size % 2)
            .ok_or_else(|| "WAV chunk boundary overflowed.".to_string())?;
    }
    let (format, channels, sample_rate, bits_per_sample) =
        format.ok_or_else(|| "WAV fmt chunk is missing.".to_string())?;
    let (data_offset, data_len) = data.ok_or_else(|| "WAV data chunk is missing.".to_string())?;
    let bytes_per_sample = usize::from(bits_per_sample / 8);
    let frame_bytes = bytes_per_sample.saturating_mul(usize::from(channels));
    let frame_count = data_len.checked_div(frame_bytes).unwrap_or_default() as u64;
    Ok(WavMetadata {
        format,
        channels,
        sample_rate,
        bits_per_sample,
        data_offset,
        data_len,
        frame_count,
    })
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

fn read_u32(bytes: &[u8]) -> Result<u32, String> {
    bytes
        .get(..4)
        .ok_or_else(|| "WAV value is truncated.".to_string())
        .and_then(|value| {
            value
                .try_into()
                .map(u32::from_le_bytes)
                .map_err(|_| "WAV value is truncated.".into())
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
        let metadata = parse_wav(&wav(&[0; 16], 2, 48_000, 16)).unwrap();
        assert_eq!(metadata.sample_rate, 48_000);
        assert_eq!(metadata.frame_count, 4);
    }

    #[test]
    fn rejects_broken_wav() {
        assert!(parse_wav(b"not wav").is_err());
    }
}
