# Riffra native audio engine

This sidecar owns the real-time timing domain. The Tauri process supervises it and never runs audio callbacks or third-party plugin code.

Current executable modes:

- `riffra-audio --probe` enumerates platform audio device types without opening an audio stream.
- `riffra-audio --serve` opens the configured device in `EngineTransition` mute state and accepts one JSON command per stdin line.

Windows uses ASIO and WASAPI. Linux uses ALSA.

The safety chain is deliberately small and auditable: owner-specific mute reasons, a 50 ms fade-in after an engine transition, non-finite sample rejection, a prepared limiter followed by a 0.98 final ceiling, DC offset blocking on the output path, and acoustic feedback detection that engages `FeedbackProtection` when sustained near-peak input is observed on a software-monitored input. The callback reports the pre-limiter peak, limiter gain reduction, final hard clips, callback overruns, and graph diagnostics. The session master gain defaults to 0 dB and is applied by the safety callback. Host Runtime releases `EngineTransition` only after the device and the canonical graph are both ready; a failed VST graph remains passive and the transition mute is kept. Instrument and effect plugins live on individual Tracks and are configured through the Arrangement Timeline Snapshot and targeted Track Device commands. Plugin scanning uses the same PluginRack load and prepare path as the Arrangement Runtime.

## Ownership

The native engine keeps the realtime path, device lifecycle, and third-party plugin lifecycle in separate owners. The owner is the place where the state transition belongs; callers forward requests across that boundary instead of reaching into another subsystem's state.

| Owner                   | Responsibility                                                                                                                                                                |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AudioEngine`           | Owns `--serve`, command dispatch, periodic status publication, the parent-process watchdog, and the long-lived runtime objects.                                               |
| `AudioDeviceController` | Owns live JUCE device setup, attachment, recovery, device-change notifications, and transition/fault state. `AudioDeviceService` is limited to discovery and setup data.      |
| `AudioRenderPipeline`   | Owns the ordered device-callback render and safety chain, including meters, preview voices, recording control, mute reasons, gain, and limiter state.                         |
| `TimelineEngine`        | Owns the prepared Arrangement graph, transport clock, timeline rendering, recording windows, and graph publication. Timeline instruments remain under `timeline/instruments`. |
| Recording classes       | `RecordingController` owns arrange-capture state at the pipeline boundary; recording sessions own capture segments, manifests, and offline finalization.                      |
| Plugin classes          | `PluginRack` and `PluginChain` own plugin processing and state; `RuntimeLifecycleExecutor` and `PluginEditorHost` own serialized third-party lifecycle and editor access.     |
| MIDI classes            | `MidiInputService` owns physical MIDI input/output access; `MidiScheduler` compiles timeline events into callback sample positions.                                           |

## Thread model

The labels below describe the allowed entry point for each owner. The command reader and periodic publishers are control-side threads; they do not become part of the audio callback.

| Thread                                | Work                                                                                                                                                           |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| JUCE message thread                   | Starts and stops the device, handles device lifecycle notifications, and executes third-party plugin lifecycle tasks dispatched by `RuntimeLifecycleExecutor`. |
| Audio callback thread                 | Runs `AudioDeviceCallback`, `AudioRenderPipeline::processBlock`, the `TimelineEngine` mix, and the safety chain.                                               |
| Command reader thread                 | Reads one JSON command per stdin line and invokes `AudioCommandDispatcher`.                                                                                    |
| Runtime lifecycle worker and watchdog | Serializes queued plugin/timeline lifecycle work and observes its deadline; the work itself is marshalled to the JUCE message thread.                          |
| MIDI callback threads                 | Receive device MIDI and enqueue bounded, non-blocking work for the preview and timeline targets.                                                               |
| Status and supervision threads        | Publish meters and transport status periodically, poll MIDI device changes, and monitor the parent process.                                                    |

## Realtime rules

The audio callback is intentionally a narrow data path. Code reached from `AudioDeviceCallback` must obey all of these rules:

- no allocation
- no blocking wait
- no device lifecycle
- no plugin lifecycle
- no file I/O
- no JSON or stdout

Control-side code prepares graphs, buffers, plugin instances, and recording sessions before the callback can observe them. The callback only exchanges the prepared state and bounded telemetry through the existing atomic and lock-free boundaries.

## Regression contract

Refactoring the native engine must preserve the externally visible contract. JSON Lines command names, response types, error operation names, error messages, status fields, meter fields, and mute-reason ownership remain stable. Device transitions keep the existing ordering: enter transition mute, prepare or recover the device and canonical graph, then release the transition only after both are ready.

The timing and safety behavior is also part of the contract: the transition fade remains 50 ms, the final limiter ceiling remains 0.98, meter and transport telemetry remain 50 ms, MIDI device polling and parent supervision remain 1 s, and timeline VST lifecycle work remains bounded by its existing 45 s timeout. Preview voice selection, recording capture/finalization, timeline graph publication, plugin publication, MIDI routing, and invalid-sample handling must remain behaviorally equivalent.

Run the native regression suite from this directory with `cmake -S . -B build` followed by `cmake --build build --config Debug` and `ctest --test-dir build -C Debug --output-on-failure`. The suite covers the protocol, device, realtime safety, timeline, recording, plugin, MIDI, concurrency, and built-in instrument boundaries.

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

The normal desktop development entry point is `npm run dev:tauri`. Its ensure
step builds the native sidecars with `RelWithDebInfo` and the Sonalloy C API
with Cargo's `release` profile, while installing the unsuffixed sidecars under
`target/debug/` beside the development CLI.

The supported configurations have these meanings:

| Configuration    | Native build                     | Sonalloy Cargo profile | Headless sidecar destination |
| ---------------- | -------------------------------- | ---------------------- | ---------------------------- |
| `Debug`          | Debug symbols and checks         | `dev`                  | `target/debug/`              |
| `RelWithDebInfo` | Optimized with debug information | `release`              | `target/debug/`              |
| `Release`        | Distribution build               | `release`              | `target/release/`            |

Use `Debug` only when explicitly debugging the native engine.

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
   for Debug and RelWithDebInfo, `target/release/` for Release) with
   `cmake --install`.

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
