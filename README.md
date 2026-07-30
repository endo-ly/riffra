# Riffra

Riffra is a music production workbench built around a short creative loop: hear, shape, capture, compare, and reuse. The product contract is defined in [CONCEPT.md](./docs/CONCEPT.md).

## Architecture

- `riffra-core` owns the platform-independent Asset, Rack, and CreativeSession domain plus shared `AppCore` state.
- `apps/desktop/src-tauri` owns the Tauri desktop lifecycle, recovery, jobs, permissions, sidecar supervision, and native integration.
- `apps/desktop/src` contains the React and TypeScript single-window workbench.
- A native C++20/JUCE sidecar owns real-time audio, MIDI, ASIO/WASAPI, VST3 hosting, metering, recording, and render paths.
- Plugin discovery and plugin execution cross process boundaries so a bad plugin cannot corrupt the UI or saved session state.
- SQLite will index reusable assets; portable project and rack manifests remain versioned JSON with standard audio/MIDI files beside them.
- Arrange uses a separate native graph per Track: physical input routing, playback/live plugin instances, MIDI routing, PDC, and Track-isolated recording taps never pass through the Play workspace's global rack.
- Arrange recordings persist a Native Audio Clock manifest plus per-Track Raw, Processed, and MIDI products; Rust finalizes those products into Recording Session / Pass / Take records and stable timeline slots.

Reference documentation under `docs/`:

- [architecture.md](./docs/architecture.md) — overall structure and layer responsibility boundaries
- [data-model.md](./docs/data-model.md) — session, project, and asset data model
- [ipc.md](./docs/ipc.md) — IPC contracts across Tauri, Native, and plugins
- [ui-ux-design/ui-ux-design.md](./docs/ui-ux-design/ui-ux-design.md) — UI/UX design (see also, [arrange-screen.md](./docs/ui-ux-design/arrange-screen.md))
- [test-strategy.md](./docs/test-strategy.md) — test strategy and quality policy

## Prerequisites

- Windows 11 x64
- Node.js 24+
- Rust stable MSVC (`%USERPROFILE%\.cargo\bin` must be on `PATH`)
- Visual Studio Build Tools 2022 with the C++ workload and CMake
- WebView2 Runtime

The target VST3 folder defaults to `C:\Program Files\Common Files\VST3` and is user-configurable.

## Development

Riffra has three independent build domains. Use the standard toolchain for each:

| Domain              | Toolchain     | Entry point                                   |
| ------------------- | ------------- | --------------------------------------------- |
| Frontend            | Vite / npm    | `npm run dev`                                 |
| Application host    | Cargo / Tauri | `npm run dev:tauri`                           |
| Native audio engine | CMake         | `native/audio-engine/build.ps1` or `build.sh` |

The domains are kept separate so the C++ engine can be built and tested without
npm, and the frontend can be developed without a full Tauri build.

### 1. Native audio engine (C++)

Build the sidecars first. They are required by the Tauri application.

Windows:

```powershell
cd native/audio-engine
.\build.ps1 -Configuration Debug
```

macOS / Linux:

```bash
cd native/audio-engine
./build.sh Debug
```

Each script configures CMake, builds `riffra-audio` and `riffra-plugin-scan`,
runs CTest, and installs the binaries to `apps/desktop/src-tauri/binaries/` using
`cmake --install`.

The sidecars are intentionally ignored by Git because they are platform-specific
build outputs. Rebuild them after a fresh checkout or after changing Native code.

### 2. Tauri application

After the sidecars are built, install Node dependencies and start the desktop
application:

```powershell
npm install
npm run dev:tauri
```

`npm run dev:tauri` is just `npm run tauri dev`. It does **not** rebuild the
Native sidecars, so remember to run the C++ build step when Native code changes.

To open a project in recovery-oriented Safe Mode, pass the flag or set the
environment variable:

```powershell
npm run dev:tauri -- --safe-mode
```

```powershell
$env:RIFFRA_SAFE_MODE = 1
npm run dev:tauri
```

Safe Mode keeps VST3 discovery, MIDI input, driver changes, live sample preview,
and new hardware recordings isolated while still allowing project open, library
access, offline analysis/render, and manifest export/import.

### 3. Frontend only

To work on the React UI without starting Tauri:

```powershell
npm run dev
```

### Verification

Run the non-GUI checks:

```powershell
npm run verify
```

Add `--native` to also build and test the C++ engine:

```powershell
npm run verify:native
```

The verification script uses `.artifacts/verify/cargo` as its Cargo target
directory. This is intentionally separate from the Tauri development target, so
Rust tests and Clippy can run while `npm run dev:tauri` is active without
competing for Cargo's build lock.

## Licensing note

JUCE framework modules are dual-licensed under AGPLv3 and a commercial JUCE licence. A local development build must comply with one of those options. Distribution terms for Riffra will be finalized before a distributable installer is produced; adding JUCE does not by itself grant a proprietary redistribution right. The VST3 SDK used by current JUCE releases is MIT-licensed, while the optional ASIO dependency has separate terms.
