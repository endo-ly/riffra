use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use ts_rs::TS;

pub(crate) mod commands;

const WAVEFORM_BINS: usize = 128;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioAnalysis {
    pub path: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples: u64,
    pub duration_ms: u64,
    pub peak_db: f64,
    pub true_peak_db: f64,
    pub rms_db: f64,
    pub clipping_samples: u64,
    pub dynamic_range_db: f64,
    pub zero_crossings: u64,
    pub phase_correlation: Option<f64>,
    pub spectrum_peak_hz: Option<f64>,
    pub waveform: Vec<f64>,
}

pub(crate) struct WavData {
    pub(crate) format: u16,
    pub(crate) channels: u16,
    pub(crate) sample_rate: u32,
    pub(crate) bits_per_sample: u16,
    pub(crate) data_offset: usize,
    pub(crate) data_len: usize,
}

pub fn analyze(path: &Path) -> Result<AudioAnalysis, String> {
    analyze_with_cancel(path, None)
}

pub fn analyze_with_cancel(
    path: &Path,
    cancelled: Option<&AtomicBool>,
) -> Result<AudioAnalysis, String> {
    if !path.is_file() {
        return Err(format!("Audio file does not exist: {}", path.display()));
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("wav") {
        return Err("Only WAV files can be analyzed.".into());
    }
    let bytes = fs::read(path).map_err(|error| format!("Audio file could not be read: {error}"))?;
    analyze_bytes(path, &bytes, cancelled)
}

fn analyze_bytes(
    path: &Path,
    bytes: &[u8],
    cancelled: Option<&AtomicBool>,
) -> Result<AudioAnalysis, String> {
    let wav = parse_wav(bytes)?;
    if wav.channels == 0 || wav.sample_rate == 0 {
        return Err("WAV has no usable channels or sample rate.".into());
    }
    let bytes_per_sample = usize::from(wav.bits_per_sample / 8);
    if !matches!(wav.format, 1 | 3) || !matches!(wav.bits_per_sample, 8 | 16 | 24 | 32) {
        return Err("WAV must be PCM or 32-bit float with 8/16/24/32-bit samples.".into());
    }
    if wav.format == 3 && wav.bits_per_sample != 32 {
        return Err("Float WAV analysis requires 32-bit samples.".into());
    }
    let frame_bytes = bytes_per_sample * usize::from(wav.channels);
    if frame_bytes == 0 || wav.data_len < frame_bytes {
        return Err("WAV contains no complete audio frames.".into());
    }
    let frames = wav.data_len / frame_bytes;
    let data = &bytes[wav.data_offset..wav.data_offset + frames * frame_bytes];
    let mut peak = 0.0_f64;
    let mut clipping_samples = 0_u64;
    let mut sum_mono_sq = 0.0_f64;
    let mut zero_crossings = 0_u64;
    let mut previous_mono = 0.0_f64;
    let mut left_sq = 0.0_f64;
    let mut right_sq = 0.0_f64;
    let mut left_right = 0.0_f64;
    let mut waveform_sum = vec![0.0_f64; WAVEFORM_BINS];
    let mut waveform_count = vec![0_u64; WAVEFORM_BINS];
    let mut spectral_samples = Vec::with_capacity(frames.min(4096));

    for frame in 0..frames {
        if frame % 4096 == 0 && cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err("Audio analysis cancelled; no partial result was promoted.".into());
        }
        let frame_start = frame * frame_bytes;
        let mut mono = 0.0_f64;
        let mut left = 0.0_f64;
        let mut right = 0.0_f64;
        for channel in 0..usize::from(wav.channels) {
            let offset = frame_start + channel * bytes_per_sample;
            let sample = decode_sample(
                &data[offset..offset + bytes_per_sample],
                wav.format,
                wav.bits_per_sample,
            )?;
            peak = peak.max(sample.abs());
            if sample.abs() >= 0.999 {
                clipping_samples += 1;
            }
            mono += sample;
            if channel == 0 {
                left = sample;
            } else if channel == 1 {
                right = sample;
            }
        }
        mono /= f64::from(wav.channels);
        sum_mono_sq += mono * mono;
        if frame > 0
            && ((previous_mono <= 0.0 && mono > 0.0) || (previous_mono >= 0.0 && mono < 0.0))
        {
            zero_crossings += 1;
        }
        previous_mono = mono;
        let waveform_bin = frame * WAVEFORM_BINS / frames;
        waveform_sum[waveform_bin] += mono.abs();
        waveform_count[waveform_bin] += 1;
        if wav.channels >= 2 {
            left_sq += left * left;
            right_sq += right * right;
            left_right += left * right;
        }
        if spectral_samples.len() < 4096 {
            spectral_samples.push(mono);
        }
    }

    let rms = (sum_mono_sq / frames as f64).sqrt();
    let phase_correlation = if wav.channels >= 2 && left_sq > 0.0 && right_sq > 0.0 {
        Some((left_right / (left_sq * right_sq).sqrt()).clamp(-1.0, 1.0))
    } else {
        None
    };
    let waveform = waveform_sum
        .into_iter()
        .zip(waveform_count)
        .map(|(sum, count)| if count == 0 { 0.0 } else { sum / count as f64 })
        .collect::<Vec<_>>();
    let waveform_peak = waveform.iter().copied().fold(0.0_f64, f64::max);
    let waveform = if waveform_peak > 0.0 {
        waveform
            .into_iter()
            .map(|value| (value / waveform_peak).clamp(0.0, 1.0))
            .collect()
    } else {
        waveform
    };
    Ok(AudioAnalysis {
        path: path.to_string_lossy().into_owned(),
        sample_rate: wav.sample_rate,
        channels: wav.channels,
        bits_per_sample: wav.bits_per_sample,
        samples: frames as u64,
        duration_ms: frames as u64 * 1000 / u64::from(wav.sample_rate),
        peak_db: linear_to_db(peak),
        true_peak_db: linear_to_db(peak),
        rms_db: linear_to_db(rms),
        clipping_samples,
        dynamic_range_db: (linear_to_db(peak) - linear_to_db(rms)).max(0.0),
        zero_crossings,
        phase_correlation,
        spectrum_peak_hz: spectrum_peak(&spectral_samples, wav.sample_rate),
        waveform,
    })
}

pub(crate) fn parse_wav(bytes: &[u8]) -> Result<WavData, String> {
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
        cursor = end + (size % 2);
    }
    let (format, channels, sample_rate, bits_per_sample) =
        format.ok_or_else(|| "WAV fmt chunk is missing.".to_string())?;
    let (data_offset, data_len) = data.ok_or_else(|| "WAV data chunk is missing.".to_string())?;
    Ok(WavData {
        format,
        channels,
        sample_rate,
        bits_per_sample,
        data_offset,
        data_len,
    })
}

pub(crate) fn decode_sample(bytes: &[u8], format: u16, bits: u16) -> Result<f64, String> {
    let sample = if format == 3 {
        f32::from_le_bytes(bytes.try_into().map_err(|_| "Invalid float sample.")?) as f64
    } else {
        match bits {
            8 => (f64::from(bytes[0]) - 128.0) / 128.0,
            16 => {
                f64::from(i16::from_le_bytes(
                    bytes.try_into().map_err(|_| "Invalid 16-bit sample.")?,
                )) / 32768.0
            }
            24 => {
                let raw =
                    i32::from(bytes[0]) | (i32::from(bytes[1]) << 8) | (i32::from(bytes[2]) << 16);
                let signed = if raw & 0x0080_0000 != 0 {
                    raw | !0x00FF_FFFF
                } else {
                    raw
                };
                f64::from(signed) / 8_388_608.0
            }
            32 => {
                f64::from(i32::from_le_bytes(
                    bytes.try_into().map_err(|_| "Invalid 32-bit sample.")?,
                )) / 2_147_483_648.0
            }
            _ => return Err("Unsupported WAV sample width.".into()),
        }
    };
    Ok(sample.clamp(-1.0, 1.0))
}

fn spectrum_peak(samples: &[f64], sample_rate: u32) -> Option<f64> {
    if samples.len() < 4 {
        return None;
    }
    let bins = (samples.len() / 2).min(256);
    let mut best_bin = 0;
    let mut best_magnitude = 0.0_f64;
    for bin in 1..=bins {
        let mut real = 0.0;
        let mut imaginary = 0.0;
        for (index, sample) in samples.iter().enumerate() {
            let phase =
                2.0 * std::f64::consts::PI * bin as f64 * index as f64 / samples.len() as f64;
            real += sample * phase.cos();
            imaginary -= sample * phase.sin();
        }
        let magnitude = real * real + imaginary * imaginary;
        if magnitude > best_magnitude {
            best_magnitude = magnitude;
            best_bin = bin;
        }
    }
    (best_bin > 0).then(|| best_bin as f64 * f64::from(sample_rate) / samples.len() as f64)
}

fn linear_to_db(value: f64) -> f64 {
    if value <= 1.0e-12 {
        -120.0
    } else {
        20.0 * value.log10()
    }
}

fn read_u16(bytes: &[u8]) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        bytes.try_into().map_err(|_| "Invalid WAV u16 field.")?,
    ))
}

fn read_u32(bytes: &[u8]) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes.try_into().map_err(|_| "Invalid WAV u32 field.")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn analyzes_pcm_wave_metrics() {
        let sample_rate = 44_100_u32;
        let samples = 4_410_u32;
        let mut data = Vec::new();
        for index in 0..samples {
            let value = (2.0 * PI * 440.0 * f64::from(index) / f64::from(sample_rate)).sin();
            data.extend_from_slice(&((value * 32767.0) as i16).to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36_u32 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);

        let root = std::env::temp_dir().join("riffra-analysis-test.wav");
        fs::write(&root, wav).unwrap();
        let result = analyze(&root).unwrap();
        assert_eq!(result.sample_rate, sample_rate);
        assert_eq!(result.samples, u64::from(samples));
        assert!(result.peak_db > -1.0);
        assert!(result.spectrum_peak_hz.is_some());
        assert_eq!(result.waveform.len(), WAVEFORM_BINS);
        let _ = fs::remove_file(root);
    }
}
