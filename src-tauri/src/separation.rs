use crate::analysis::{decode_sample, parse_wav};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::Path,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparationResult {
    pub id: String,
    pub source_path: String,
    pub left_path: String,
    pub right_path: String,
    pub state: String,
    pub created_at_ms: u64,
    pub message: String,
}

pub fn separate_channels(
    source: &Path,
    output_root: &Path,
    created_at_ms: u64,
) -> Result<SeparationResult, String> {
    if !source.is_file() {
        return Err(format!("Audio file does not exist: {}", source.display()));
    }
    if !source
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        return Err("Channel split currently accepts WAV sources only.".into());
    }
    let bytes =
        fs::read(source).map_err(|error| format!("Audio file could not be read: {error}"))?;
    let wav = parse_wav(&bytes)?;
    if wav.channels < 2 {
        return Err("Channel split requires a stereo WAV source.".into());
    }
    if !matches!(wav.format, 1 | 3) || !matches!(wav.bits_per_sample, 8 | 16 | 24 | 32) {
        return Err("Channel split supports PCM or 32-bit float WAV sources.".into());
    }
    if wav.format == 3 && wav.bits_per_sample != 32 {
        return Err("Float channel split requires 32-bit samples.".into());
    }
    let bytes_per_sample = usize::from(wav.bits_per_sample / 8);
    let frame_bytes = bytes_per_sample * usize::from(wav.channels);
    if frame_bytes == 0 || wav.data_len < frame_bytes {
        return Err("WAV contains no complete stereo frames.".into());
    }
    let frames = wav.data_len / frame_bytes;
    let data_end = wav
        .data_offset
        .checked_add(frames * frame_bytes)
        .ok_or_else(|| "WAV data boundary overflowed.".to_string())?;
    let data = bytes
        .get(wav.data_offset..data_end)
        .ok_or_else(|| "WAV data chunk is outside the file boundary.".to_string())?;
    let output_dir = output_root.join(format!("job-{created_at_ms}"));
    fs::create_dir_all(&output_dir)
        .map_err(|error| format!("Separation output folder could not be created: {error}"))?;
    let left = output_dir.join("left.wav");
    let right = output_dir.join("right.wav");
    let left_partial = output_dir.join("left.wav.partial");
    let right_partial = output_dir.join("right.wav.partial");
    let data_len = u32::try_from(frames.checked_mul(4).ok_or("Output WAV is too large.")?)
        .map_err(|_| "Output WAV is too large for a RIFF data chunk.".to_string())?;
    let mut left_writer = split_writer(&left_partial, wav.sample_rate, data_len)
        .map_err(|error| format!("Left channel output could not be created: {error}"))?;
    let mut right_writer = split_writer(&right_partial, wav.sample_rate, data_len)
        .map_err(|error| format!("Right channel output could not be created: {error}"))?;
    for frame in 0..frames {
        let start = frame * frame_bytes;
        let left_sample = decode_sample(
            &data[start..start + bytes_per_sample],
            wav.format,
            wav.bits_per_sample,
        )? as f32;
        let right_start = start + bytes_per_sample;
        let right_sample = decode_sample(
            &data[right_start..right_start + bytes_per_sample],
            wav.format,
            wav.bits_per_sample,
        )? as f32;
        left_writer
            .write_all(&left_sample.to_le_bytes())
            .map_err(|error| format!("Left channel output could not be written: {error}"))?;
        right_writer
            .write_all(&right_sample.to_le_bytes())
            .map_err(|error| format!("Right channel output could not be written: {error}"))?;
    }
    left_writer
        .flush()
        .map_err(|error| format!("Left channel output could not be flushed: {error}"))?;
    right_writer
        .flush()
        .map_err(|error| format!("Right channel output could not be flushed: {error}"))?;
    drop(left_writer);
    drop(right_writer);
    fs::rename(&left_partial, &left)
        .map_err(|error| format!("Left channel output could not be finalized: {error}"))?;
    fs::rename(&right_partial, &right)
        .map_err(|error| format!("Right channel output could not be finalized: {error}"))?;

    let result = SeparationResult {
        id: format!("separation:{created_at_ms}"),
        source_path: source.to_string_lossy().into_owned(),
        left_path: left.to_string_lossy().into_owned(),
        right_path: right.to_string_lossy().into_owned(),
        state: "completed".into(),
        created_at_ms,
        message: "Stereo channels were separated without modifying the source WAV.".into(),
    };
    write_manifest(&output_dir.join("manifest.json"), &result)
        .map_err(|error| format!("Separation manifest could not be saved: {error}"))?;
    Ok(result)
}

pub fn list(output_root: &Path) -> Result<Vec<SeparationResult>, String> {
    let root = output_root.join("separations");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|error| format!("Separation folder could not be read: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("Separation entry could not be read: {error}"))?;
        let manifest = entry.path().join("manifest.json");
        if !manifest.is_file() {
            continue;
        }
        let payload = fs::read(&manifest)
            .map_err(|error| format!("Separation manifest could not be read: {error}"))?;
        if let Ok(result) = serde_json::from_slice::<SeparationResult>(&payload) {
            results.push(result);
        }
    }
    results.sort_by_key(|result| std::cmp::Reverse(result.created_at_ms));
    Ok(results)
}

fn split_writer(path: &Path, sample_rate: u32, data_len: u32) -> io::Result<BufWriter<File>> {
    let mut file = File::create(path)?;
    let riff_len = 36_u32
        .checked_add(data_len)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "RIFF size overflow"))?;
    file.write_all(b"RIFF")?;
    file.write_all(&riff_len.to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&3_u16.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&(sample_rate.saturating_mul(4)).to_le_bytes())?;
    file.write_all(&4_u16.to_le_bytes())?;
    file.write_all(&32_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    Ok(BufWriter::new(file))
}

fn write_manifest(path: &Path, result: &SeparationResult) -> io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    let payload = serde_json::to_vec_pretty(result)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(&temporary, payload)?;
    if let Err(error) = fs::rename(&temporary, path) {
        if path.exists() {
            fs::remove_file(path)?;
            fs::rename(&temporary, path)?;
        } else {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyze;

    #[test]
    fn splits_stereo_pcm_and_preserves_source() {
        let root = std::env::temp_dir().join(format!("riffra-separation-{}", created_id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("stereo.wav");
        write_stereo_test_wav(&source);
        let result = separate_channels(&source, &root.join("separations"), 42).unwrap();
        assert_eq!(result.state, "completed");
        assert!(Path::new(&result.left_path).is_file());
        assert!(Path::new(&result.right_path).is_file());
        assert_eq!(analyze(&source).unwrap().channels, 2);
        assert_eq!(analyze(Path::new(&result.left_path)).unwrap().channels, 1);
        assert_eq!(list(&root).unwrap().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    fn created_id() -> u64 {
        crate::storage::now_ms()
    }

    fn write_stereo_test_wav(path: &Path) {
        let mut data = Vec::new();
        for index in 0..32_i16 {
            data.extend_from_slice(&(index * 512).to_le_bytes());
            data.extend_from_slice(&(-index * 256).to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36_u32 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&44_100_u32.to_le_bytes());
        wav.extend_from_slice(&(44_100_u32 * 4).to_le_bytes());
        wav.extend_from_slice(&4_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        fs::write(path, wav).unwrap();
    }
}
