//! Cancellable two-pass loudness mastering of the native float WAV.

use crate::api::output::{LoudnessMeasurement, MasteringReport};
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
) -> Result<MasteringReport, String> {
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
    Ok(MasteringReport {
        target: target.clone(),
        input: input_measurement,
        output,
        deviation,
        normalization_type,
        true_peak_correction_db: correction,
    })
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
