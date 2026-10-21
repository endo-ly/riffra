//! Shared cancellable FFmpeg execution for completed WAV processing.

use std::{
    fs,
    path::Path,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(crate) fn run(
    input: &Path,
    output: Option<&Path>,
    filter: &str,
    sample_rate: u32,
    directory: &Path,
    cancelled: Option<&AtomicBool>,
    log_level: &str,
) -> Result<String, String> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err("audio processing was cancelled".into());
    }
    let log = directory.join("ffmpeg.log");
    let stderr = fs::File::create(&log)
        .map_err(|error| format!("ffmpeg log could not be created: {error}"))?;
    let mut command = crate::process::sidecar_command("ffmpeg");
    command
        .args([
            "-hide_banner",
            "-nostdin",
            "-y",
            "-loglevel",
            log_level,
            "-i",
        ])
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
        .map_err(|error| format!("ffmpeg processing could not start: {error}"))?;
    let status = loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("audio processing was cancelled".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("ffmpeg processing could not be monitored: {error}"));
            }
        }
    };
    let text = fs::read_to_string(log)
        .map_err(|error| format!("ffmpeg log could not be read: {error}"))?;
    if !status.success() {
        return Err(format!("ffmpeg processing failed: {}", text.trim()));
    }
    Ok(text)
}
