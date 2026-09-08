# Riffra native audio engine

This sidecar owns the real-time timing domain. The Tauri process supervises it and never runs audio callbacks or third-party plugin code.

Current executable modes:

- `riffra-audio --probe` enumerates platform audio device types without opening an audio stream.
- `riffra-audio --serve` opens the configured device in `EngineTransition` mute state and accepts one JSON command per stdin line.

Windows uses ASIO and WASAPI. Linux uses ALSA.

The safety chain is deliberately small and auditable: owner-specific mute reasons, a 50 ms fade-in after an engine transition, non-finite sample rejection, a prepared limiter followed by a 0.98 final ceiling, DC offset blocking on the output path, and acoustic feedback detection that engages `FeedbackProtection` when sustained near-peak input is observed on a software-monitored input. The callback reports the pre-limiter peak, limiter gain reduction, final hard clips, callback overruns, and graph diagnostics. The session master gain defaults to 0 dB and is applied by the safety callback. Host Runtime releases `EngineTransition` only after the device and the canonical graph are both ready; a failed VST graph remains passive and the transition mute is kept. Instrument and effect plugins live on individual Tracks and are configured through the Arrangement Timeline Snapshot and targeted Track Device commands. Plugin scanning uses the same PluginRack load and prepare path as the Arrangement Runtime.

## Protocol examples

```json
{"type":"status"}
{"type":"setEmergencyMute","muted":false}
{"type":"setMasterGainDb","gainDb":-24.0}
{"type":"loadTimelineSnapshot","snapshot":{...}}
{"type":"setTrackDeviceParameter","trackId":"track:1","deviceId":"device:1","parameterIndex":0,"value":0.5}
{"type":"setLiveMidiTarget","trackId":"track:1"}
{"type":"sendTrackMidi","trackId":"track:1","bytes":[144,60,100]}
{"type":"recoverAudioDevice"}
{"type":"previewSample","path":"C:\\path\\to\\processed.wav","startMs":0,"endMs":1000,"gain":1.0}
{"type":"stopPreview"}
{"type":"openMidiInput","name":"Controller Name"}
{"type":"closeMidiInput"}
{"type":"startArrangeRecording","directory":"C:\\path\\to\\recording"}
{"type":"stopArrangeRecording"}
{"type":"shutdown"}
```

Responses are JSON Lines. A failed command returns `type: "error"` with `kind`, `message`, `operation`, and an object-valued `details` field. Status replies include the `muteReasons` bitmask and callback diagnostics.

Status replies include `feedbackSuspected` when the detector has engaged `FeedbackProtection` due to acoustic feedback. Each mute owner clears only its own bit; releasing the user mute does not clear an engine transition, device fault, or feedback protection.

When an input is open, live MIDI is routed to the matching Instrument Track. The focused
Play Surface target bypasses only inter-track compensation delay; its prepared delay buffer
continues to advance so returning to compensated playback does not reinitialize timing state.
Arrange recording stores captured MIDI with the track's recording result.

## Building

The engine is built with CMake, a compatible C++ toolchain, Rust/Cargo, and
Node.js. The wrapper script does not require `npm install`:

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
2. Build the three runtime sidecars.
3. Run CTest.
4. Install the Tauri-named sidecars to `apps/desktop/src-tauri/binaries/` and
   unsuffixed copies beside the matching Cargo CLI artifact (`target/debug/`
   for Debug, `target/release/` otherwise) with `cmake --install`.

For development startup, the repository ensure step uses the same wrapper with
`-SidecarsOnly` on Windows or `SIDECARS_ONLY=1` on other platforms. This builds
the three runtime sidecars without compiling or running the native test suite.

This directory can be built independently of the npm workspace. The Tauri application expects
the target-triple-named sidecars under `apps/desktop/src-tauri/binaries/`, while
`riffra serve` resolves the unsuffixed copies beside the `riffra` executable.
Set `RIFFRA_HEADLESS_BINARIES_DESTINATION` to override the headless install
destination when producing a distribution artifact. The same wrapper run also
installs the built-in instrument resource bundle under
`apps/desktop/src-tauri/resources/instruments/builtin/` and beside the headless
executable at `riffra-resources/instruments/builtin/`; set
`RIFFRA_HEADLESS_RESOURCES_DESTINATION` to override the latter destination.

Standalone `riffra` commands resolve the same bundle beside the executable. When
using a resource bundle staged elsewhere, set
`RIFFRA_BUILTIN_INSTRUMENTS_ROOT` to its `instruments/builtin` directory.
