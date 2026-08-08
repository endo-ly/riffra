# Riffra native audio engine

This sidecar owns the real-time timing domain. The Tauri process supervises it and never runs audio callbacks or third-party plugin code.

Current executable modes:

- `riffra-audio.exe --probe` enumerates ASIO/WASAPI device types without opening an audio stream.
- `riffra-audio.exe --serve` opens the default device in emergency-mute state and accepts one JSON command per stdin line.

The safety chain is deliberately small and auditable: immediate emergency mute, −18 dB conservative startup gain, 500 ms fade-in after unmute, non-finite sample rejection, a 0.98 hard ceiling, DC offset blocking on the output path, and acoustic feedback detection that auto-mutes when sustained near-peak input is observed. Instrument and effect plugins live on individual Tracks; they are configured through the Arrangement Timeline Snapshot and targeted Track Device commands rather than a global rack.

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
{"type":"startRecording","directory":"C:\\path\\to\\recording"}
{"type":"stopRecording"}
{"type":"shutdown"}
```

Responses are JSON Lines and always include an error scope and `dataSafe` when a request fails.

Status replies include `feedbackSuspected` when the detector has engaged emergency mute due to acoustic feedback. The flag clears on device recovery (`audioDeviceAboutToStart`).

When an input is open, `startRecording` also captures note-on/note-off events to
`midi.json` beside the Raw and Processed WAV files. The sidecar caps the event
journal at 200,000 events and finalizes it on `stopRecording`.

## Building

The engine is built with CMake. Use the wrapper script for your platform:

```powershell
# Windows
.\build.ps1 -Configuration Debug
```

```bash
# macOS / Linux
./build.sh Debug
```

Both scripts do the following:

1. Configure CMake.
2. Build `riffra-audio` and `riffra-plugin-scan`.
3. Run CTest.
4. Install the sidecars to `apps/desktop/src-tauri/binaries/` with `cmake --install`.

This directory can be built independently of npm. The Tauri application expects
the sidecars to exist under `apps/desktop/src-tauri/binaries/` before it starts.

For a full project verification that also runs TypeScript and Rust checks, run
`npm run verify:native` from the repository root. See the root
[README.md](../../README.md) for the complete workflow.
