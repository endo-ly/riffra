//! Cancellable two-pass loudness mastering of the native float WAV.

use crate::api::output::{
    AudioMeasurements, LoudnessMeasurement, LoudnessReadout, MasteringReport,
};
use riffra_core::LoudnessMastering;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

struct TemporaryDirectory(std::path::PathBuf);

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            tracing::error!(%error, "mastering temporary files could not be removed");
        }
    }
}

pub(super) fn master(
    input: &Path,
    target: &LoudnessMastering,
    sample_rate: u32,
    cancelled: Option<&AtomicBool>,
) -> Result<(MasteringReport, LoudnessMeasurement), String> {
    let directory = input
        .parent()
        .ok_or("mastering input has no parent directory")?
        .join(format!("mastering-{}", riffra_control::new_instance_id()));
    fs::create_dir(&directory)
        .map_err(|error| format!("mastering temporary directory could not be created: {error}"))?;
    let temporary = TemporaryDirectory(directory);
    let normalized = temporary.0.join("normalized.wav");
    let corrected = temporary.0.join("corrected.wav");
    let base = format!(
        "loudnorm=I={}:TP={}:LRA={}",
        target.integrated_lufs, target.true_peak_db, target.loudness_range_lu
    );
    let first = loudnorm(
        input,
        None,
        &(base.clone() + ":print_format=json"),
        sample_rate,
        &temporary.0,
        cancelled,
    )?;
    let input_measurement = measurement(&first)?;
    let second_filter = format!(
        "{base}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true:print_format=json",
        number(&first, "input_i")?,
        number(&first, "input_tp")?,
        number(&first, "input_lra")?,
        number(&first, "input_thresh")?,
        number(&first, "target_offset")?
    );
    let second = loudnorm(
        input,
        Some(&normalized),
        &second_filter,
        sample_rate,
        &temporary.0,
        cancelled,
    )?;
    let normalization_type = second
        .get("normalization_type")
        .and_then(Value::as_str)
        .ok_or("ffmpeg loudnorm report is missing normalization_type")?
        .to_owned();
    let mut output = measurement(&loudnorm(
        &normalized,
        None,
        &(base.clone() + ":print_format=json"),
        sample_rate,
        &temporary.0,
        cancelled,
    )?)?;
    let mut final_path = normalized.as_path();
    let mut correction = 0.0;
    if output.true_peak_db > target.true_peak_db {
        correction = target.true_peak_db - output.true_peak_db;
        let filter = format!("volume={correction}dB");
        run(
            &normalized,
            Some(&corrected),
            &filter,
            sample_rate,
            &temporary.0,
            cancelled,
        )?;
        output = measurement(&loudnorm(
            &corrected,
            None,
            &(base + ":print_format=json"),
            sample_rate,
            &temporary.0,
            cancelled,
        )?)?;
        final_path = &corrected;
    }
    if output.true_peak_db > target.true_peak_db {
        return Err(format!(
            "completed wav true peak {} dbtp exceeds target {} dbtp after gain correction",
            output.true_peak_db, target.true_peak_db
        ));
    }
    let deviation = LoudnessMeasurement {
        integrated_lufs: output.integrated_lufs - target.integrated_lufs,
        true_peak_db: output.true_peak_db - target.true_peak_db,
        loudness_range_lu: output.loudness_range_lu - target.loudness_range_lu,
    };
    fs::copy(final_path, input)
        .map_err(|error| format!("mastered wav could not be saved: {error}"))?;
    Ok((
        MasteringReport {
            target: target.clone(),
            input: input_measurement,
            deviation,
            normalization_type,
            true_peak_correction_db: correction,
        },
        output,
    ))
}

pub(super) fn measure_output(
    input: &Path,
    sample_rate: u32,
    mastered: Option<LoudnessMeasurement>,
    target: Option<&LoudnessMastering>,
    cancelled: Option<&AtomicBool>,
) -> Result<(AudioMeasurements, Vec<String>), String> {
    let wav = riffra_host::read_wav_structure(input)?;
    if !matches!(wav.format, 1 | 3)
        || !matches!(wav.bits_per_sample, 8 | 16 | 24 | 32)
        || (wav.format == 3 && wav.bits_per_sample != 32)
    {
        return Err("completed wav sample format is unsupported".into());
    }
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(input)
        .map_err(|error| format!("completed wav could not be opened: {error}"))?;
    file.seek(SeekFrom::Start(wav.data_offset as u64))
        .map_err(|error| format!("completed wav samples could not be located: {error}"))?;
    let width = usize::from(wav.bits_per_sample / 8);
    let mut buffer = [0_u8; 24_576];
    let mut remaining = wav.data_len;
    let mut peak = 0.0_f64;
    while remaining > 0 {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err("timeline render was cancelled".into());
        }
        let count = remaining.min(buffer.len());
        file.read_exact(&mut buffer[..count])
            .map_err(|error| format!("completed wav samples could not be read: {error}"))?;
        for sample in buffer[..count].chunks_exact(width) {
            peak = peak.max(
                crate::analysis::decode_sample(sample, wav.format, wav.bits_per_sample)?.abs(),
            );
        }
        remaining -= count;
    }
    let sample_peak_dbfs = (peak > 0.0).then(|| 20.0 * peak.log10());
    let loudness = if let Some(measurement) = mastered {
        LoudnessReadout::Measured { measurement }
    } else {
        let directory = input
            .parent()
            .ok_or("completed wav has no parent directory")?
            .join(format!("measure-{}", riffra_control::new_instance_id()));
        fs::create_dir(&directory)
            .map_err(|error| format!("measurement directory could not be created: {error}"))?;
        let temporary = TemporaryDirectory(directory);
        match loudnorm(
            input,
            None,
            "loudnorm=print_format=json",
            sample_rate,
            &temporary.0,
            cancelled,
        )
        .and_then(|report| measurement(&report))
        {
            Ok(measurement) => LoudnessReadout::Measured { measurement },
            Err(reason) => LoudnessReadout::Unavailable { reason },
        }
    };
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err("timeline render was cancelled".into());
    }
    let mut warnings = Vec::new();
    if sample_peak_dbfs.is_some_and(|peak| peak > 0.0) {
        warnings.push("sample peak exceeds 0 dbfs".into());
    }
    if let (LoudnessReadout::Measured { measurement }, Some(target)) = (&loudness, target) {
        if measurement.true_peak_db > target.true_peak_db {
            warnings.push("true peak exceeds the configured ceiling".into());
        }
        if (measurement.integrated_lufs - target.integrated_lufs).abs() > 0.5 {
            warnings
                .push("integrated loudness differs from the target by more than 0.5 lufs".into());
        }
        if measurement.loudness_range_lu > target.loudness_range_lu + 0.5 {
            warnings.push("loudness range exceeds the target by more than 0.5 lu".into());
        }
    }
    Ok((
        AudioMeasurements {
            sample_peak_dbfs,
            loudness,
        },
        warnings,
    ))
}

fn loudnorm(
    input: &Path,
    output: Option<&Path>,
    filter: &str,
    sample_rate: u32,
    directory: &Path,
    cancelled: Option<&AtomicBool>,
) -> Result<Value, String> {
    let text = run(input, output, filter, sample_rate, directory, cancelled)?;
    for (index, character) in text.char_indices() {
        if character != '{' {
            continue;
        }
        let mut stream = serde_json::Deserializer::from_str(&text[index..]).into_iter::<Value>();
        if let Some(Ok(report)) = stream.next()
            && report.get("input_i").is_some()
        {
            return Ok(report);
        }
    }
    Err("ffmpeg loudnorm did not emit a json report".into())
}

fn number(report: &Value, field: &str) -> Result<f64, String> {
    report
        .get(field)
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("ffmpeg loudnorm report is missing finite {field}"))
}

fn measurement(report: &Value) -> Result<LoudnessMeasurement, String> {
    Ok(LoudnessMeasurement {
        integrated_lufs: number(report, "input_i")?,
        true_peak_db: number(report, "input_tp")?,
        loudness_range_lu: number(report, "input_lra")?,
    })
}

fn run(
    input: &Path,
    output: Option<&Path>,
    filter: &str,
    sample_rate: u32,
    directory: &Path,
    cancelled: Option<&AtomicBool>,
) -> Result<String, String> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err("timeline render was cancelled".into());
    }
    let log = directory.join("ffmpeg.log");
    let stderr = fs::File::create(&log)
        .map_err(|error| format!("mastering log could not be created: {error}"))?;
    let mut command = crate::process::sidecar_command("ffmpeg");
    command
        .args(["-hide_banner", "-nostdin", "-y", "-i"])
        .arg(input)
        .args(["-af", filter]);
    if let Some(output) = output {
        command
            .args(["-ar", &sample_rate.to_string(), "-c:a", "pcm_f32le"])
            .arg(output);
    } else {
        command.args(["-f", "null", "-"]);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|error| format!("ffmpeg mastering could not start: {error}"))?;
    let status = loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("timeline render was cancelled".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("ffmpeg mastering could not be monitored: {error}"));
            }
        }
    };
    let text = fs::read_to_string(log)
        .map_err(|error| format!("mastering log could not be read: {error}"))?;
    if !status.success() {
        return Err(format!("ffmpeg mastering failed: {}", text.trim()));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_float_samples_above_full_scale_and_reports_loudness_availability() {
        let root = std::env::temp_dir().join(format!(
            "riffra-output-measurement-{}",
            riffra_control::new_instance_id()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("output.wav");
        let sample_rate = 48_000_u32;
        let frames = sample_rate * 4;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + frames * 4).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&3_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(frames * 4).to_le_bytes());
        for frame in 0..frames {
            let sample = if frame == sample_rate {
                1.25_f32
            } else {
                (f64::from(frame) * 440.0 * std::f64::consts::TAU / f64::from(sample_rate)).sin()
                    as f32
                    * 0.25
            };
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        fs::write(&path, bytes).unwrap();

        let (measurements, warnings) =
            measure_output(&path, sample_rate, None, None, None).unwrap();

        assert!((measurements.sample_peak_dbfs.unwrap() - 20.0 * 1.25_f64.log10()).abs() < 0.0001);
        assert_eq!(warnings, ["sample peak exceeds 0 dbfs"]);
        match &measurements.loudness {
            LoudnessReadout::Measured { measurement } => {
                assert!(measurement.integrated_lufs < -5.0 && measurement.integrated_lufs > -30.0);
                assert!(measurement.true_peak_db > 0.0);
                assert!(measurement.loudness_range_lu.is_finite());
            }
            LoudnessReadout::Unavailable { reason } => assert!(reason.contains("ffmpeg")),
        }
        assert!(serde_json::to_vec(&measurements).is_ok());
        if matches!(measurements.loudness, LoudnessReadout::Measured { .. }) {
            let target = LoudnessMastering {
                integrated_lufs: -14.0,
                true_peak_db: -1.0,
                loudness_range_lu: 11.0,
            };
            let (report, output) = master(&path, &target, sample_rate, None).unwrap();
            assert!(output.true_peak_db <= target.true_peak_db);
            let expected_deviation = output.integrated_lufs - target.integrated_lufs;
            let (final_measurements, _) =
                measure_output(&path, sample_rate, Some(output), Some(&target), None).unwrap();
            assert!(final_measurements.sample_peak_dbfs.unwrap() <= 0.0);
            assert_eq!(report.deviation.integrated_lufs, expected_deviation);
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
