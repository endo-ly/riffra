# Riffra native audio engine

This sidecar owns the real-time timing domain. The Tauri process supervises it and never runs audio callbacks or third-party plugin code.

Current executable modes:

- `riffra-audio --probe` enumerates platform audio device types without opening an audio stream.
- `riffra-audio --serve` opens the default device in emergency-mute state and accepts one JSON command per stdin line.

Windows uses ASIO and WASAPI. Linux uses ALSA.

The safety chain is deliberately small and auditable: immediate emergency mute, −18 dB conservative startup gain, 500 ms fade-in after unmute, non-finite sample rejection, a 0.98 hard ceiling, DC offset blocking on the output path, and acoustic feedback detection that auto-mutes when sustained near-peak input is observed on a software-monitored input. Rust releases startup mute after this safety boundary; a failed VST graph is kept passive and reported separately from device safety. Instrument and effect plugins live on individual Tracks and are configured through the Arrangement Timeline Snapshot and targeted Track Device commands. Plugin scanning uses the same PluginRack load and prepare path as the Arrangement Runtime.

## Protocol examples

```json
{"type":"status"}
{"type":"setEmergencyMute","muted":false}
{"type":"setMasterGainDb","gainDb":-24.0}
{"type":"loadTimelineSnapshot","snapshot":{...}}
{"type":"setTrackDeviceParameter","trackId":"track:1","deviceId":"device:1","index":0,"value":0.5}
{"type":"sendTrackMidi","trackId":"track:1","bytes":[144,60,100]}
{"type":"recoverAudioDevice"}
{"type":"previewSample","path":"C:\\path\\to\\processed.wav","startMs":0,"endMs":1000,"gain":1.0}
{"type":"stopPreview"}
{"type":"configureSamplePads","pads":[{"id":"pad:kick","name":"Kick","assetPath":"C:\\path\\to\\kick.wav","startMs":0,"endMs":500,"midiKey":36}]}
{"type":"openMidiInput","name":"Controller Name"}
{"type":"closeMidiInput"}
{"type":"startArrangeRecording","directory":"C:\\path\\to\\recording"}
{"type":"stopArrangeRecording"}
{"type":"shutdown"}
```

Responses are JSON Lines and always include an error scope and `dataSafe` when a request fails.

Status replies include `feedbackSuspected` when the detector has engaged emergency mute due to acoustic feedback. The flag clears when emergency mute is released or the audio device is restarted.

When an input is open, live MIDI is routed to the matching Instrument Track.
Arrange recording stores captured MIDI with the track's recording result.

## Building

The engine is built with CMake. Use the wrapper script for your platform:

```powershell
# Windows
.\build.ps1 -Configuration Debug
```

The Windows wrapper defaults to the `Visual Studio 17 2022` generator and the
`x64` architecture. Override either value when using another installed
toolchain, for example:

```powershell
.\build.ps1 -Configuration Debug -Generator Ninja
.\build.ps1 -Configuration Debug -Generator 'Visual Studio 16 2019' -Architecture x64
```

```bash
# macOS / Linux
./build.sh Debug
```

The sidecar target triple follows the host platform by default. When
cross-compiling, pass `-DRIFFRA_TARGET_TRIPLE=<triple>` to CMake.

Both scripts do the following:

1. Configure CMake.
2. Build `riffra-audio` and `riffra-plugin-scan`.
3. Run CTest.
4. Install the sidecars to `apps/desktop/src-tauri/binaries/` with `cmake --install`.

This directory can be built independently of npm. The Tauri application expects
the sidecars to exist under `apps/desktop/src-tauri/binaries/` before it starts.
