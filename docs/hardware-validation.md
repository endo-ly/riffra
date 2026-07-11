# Hardware validation matrix

Riffra's audio safety and recording paths are validated in two layers. The deterministic self-test never opens a microphone; the hardware matrix is run only on the named Windows machine with the operator's interface connected.

## Deterministic checks

From the repository root:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo test --manifest-path src-tauri\Cargo.toml
& .\scripts\build-native.ps1 -Configuration Debug
$take = Join-Path (Resolve-Path .artifacts) "recording-self-test-$(Get-Date -Format yyyyMMddHHmmss)"
& .\src-tauri\binaries\riffra-audio-x86_64-pc-windows-msvc.exe --recording-self-test $take
```

The self-test must report `ok: true`, `rawSamples == processedSamples`, `droppedBlocks == 0`, and a `completed` manifest containing `raw.wav` and `processed.wav`. It uses a generated 440 Hz signal and does not capture device input.

## Manual matrix

Record the exact Windows build, Riffra build, driver version, sample rate, buffer size, channel map, and plugin set for every run.

| Case | Driver / device | Pass criteria | Result / notes |
| --- | --- | --- | --- |
| WASAPI shared playback |  | Starts muted, fades safely, no unexpected output |  |
| WASAPI input + output |  | Raw and Processed take lengths match; no clip/dropout |  |
| ASIO low-latency |  | Round-trip latency is reported; emergency mute remains immediate |  |
| Device hot unplug |  | Engine faults softly, completed chunks remain readable |  |
| Sleep / resume |  | Device status recovers or presents an actionable fault |  |
| MIDI hot unplug |  | No stuck notes after disconnect or panic |  |
| VST3 candidate |  | Scanner worker isolates failure and catalog remains usable |  |

Do not treat a passing synthetic self-test as evidence that a particular interface, ASIO driver, MIDI device, or plugin is hardware-validated.
