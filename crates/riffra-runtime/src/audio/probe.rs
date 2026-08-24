use super::AudioSupervisor;
use crate::model::{AudioDeviceProbe, DeviceChannels};
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

impl AudioSupervisor {
    /// Probes devices through the same native executable used by the live runtime.
    pub fn probe_devices(&self, timeout: Duration) -> Result<AudioDeviceProbe, String> {
        let output = run_probe(&self.binaries.audio, ["--probe"], timeout)?;
        if !output.status.success() {
            return Err(format_probe_failure(&output));
        }
        parse_probe(&output.stdout, "audioDeviceProbe")
    }

    /// Probes channel layouts through the live native executable.
    pub fn probe_device_channels(
        &self,
        driver: &str,
        input_device: &str,
        output_device: &str,
        timeout: Duration,
    ) -> Result<DeviceChannels, String> {
        let output = run_probe(
            &self.binaries.audio,
            [
                "--probe-channels",
                "--audio-driver",
                driver,
                "--input-device",
                input_device,
                "--output-device",
                output_device,
            ],
            timeout,
        )?;
        if !output.status.success() {
            return Err(format_probe_failure(&output));
        }
        parse_probe(&output.stdout, "deviceChannels")
    }
}

fn run_probe<'a, I>(
    executable: &std::path::Path,
    args: I,
    timeout: Duration,
) -> Result<Output, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut child = Command::new(executable)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("audio probe could not start: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "audio probe stdout was not piped".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "audio probe stderr was not piped".to_owned())?;
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return join_probe_output(status, stdout_reader, stderr_reader);
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!(
                    "audio probe timed out after {} ms",
                    timeout.as_millis()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("audio probe status could not be read: {error}"));
            }
        }
    }
}

fn read_pipe(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_probe_output(
    status: std::process::ExitStatus,
    stdout_reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stderr_reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Output, String> {
    let stdout = stdout_reader
        .join()
        .map_err(|_| "audio probe stdout reader panicked".to_owned())?
        .map_err(|error| format!("audio probe stdout could not be read: {error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "audio probe stderr reader panicked".to_owned())?
        .map_err(|error| format!("audio probe stderr could not be read: {error}"))?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn parse_probe<T: serde::de::DeserializeOwned>(bytes: &[u8], expected: &str) -> Result<T, String> {
    let mut invalid_response = None;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value = match serde_json::from_slice::<serde_json::Value>(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("type").and_then(serde_json::Value::as_str) != Some(expected) {
            continue;
        }
        match serde_json::from_slice(line) {
            Ok(response) => return Ok(response),
            Err(error) => invalid_response = Some(error),
        }
    }
    if let Some(error) = invalid_response {
        Err(format!(
            "audio probe {expected} response was invalid: {error}"
        ))
    } else {
        Err(format!("audio probe returned no {expected} response"))
    }
}

fn format_probe_failure(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("audio probe exited with status {}", output.status)
    } else {
        format!("audio probe failed: {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::parse_probe;
    use crate::{AudioDevicePairing, AudioDeviceProbe, DeviceChannels};

    #[test]
    fn parses_audio_probe_with_unicode_device_names() {
        let probe: AudioDeviceProbe = parse_probe(
            br#"{"type":"audioDeviceProbe","drivers":[{"name":"ASIO","accessMode":"driverManaged","devicePairing":"sameDevice","inputs":[{"name":"Focusrite","channels":[{"index":0,"name":"Input 1"}]}],"outputs":[{"name":"Focusrite","channels":[{"index":0,"name":"Output 1"}]}]},{"name":"WASAPI","accessMode":"shared","devicePairing":"independent","inputs":[],"outputs":[]}],"refreshedAtMs":1,"message":""}"#,
            "audioDeviceProbe",
        )
        .unwrap();

        assert_eq!(probe.drivers[0].name, "ASIO");
        assert_eq!(probe.drivers[0].inputs[0].name, "Focusrite");
        assert_eq!(probe.drivers[0].inputs[0].channels[0].name, "Input 1");
        assert_eq!(probe.drivers[0].outputs[0].channels[0].index, 0);
        assert_eq!(probe.drivers[0].outputs[0].channels[0].name, "Output 1");
        assert_eq!(
            probe.drivers[1].device_pairing,
            AudioDevicePairing::Independent
        );
        assert!(probe.drivers[1].inputs.is_empty());
        assert!(probe.drivers[1].outputs.is_empty());
    }

    #[test]
    fn parses_device_channels_detail() {
        let detail: DeviceChannels = parse_probe(
            br#"{"type":"deviceChannels","driver":"ASIO","inputDevice":"Focusrite","inputChannels":[{"index":0,"name":"Analogue 1"}],"outputDevice":"Focusrite","outputChannels":[{"index":0,"name":"Output 1"}]}"#,
            "deviceChannels",
        )
        .unwrap();

        assert_eq!(detail.driver, "ASIO");
        assert_eq!(detail.input_channels[0].name, "Analogue 1");
        assert_eq!(detail.output_channels[0].name, "Output 1");
    }

    #[test]
    fn rejects_non_probe_messages() {
        let error = parse_probe::<DeviceChannels>(br#"{"type":"audioStatus"}"#, "deviceChannels")
            .unwrap_err();

        assert!(error.contains("no deviceChannels response"));
    }
}
