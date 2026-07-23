# Riffra

Riffra is a music production workbench built around a short creative loop: hear, shape, capture, compare, and reuse. The product contract is defined in [CONCEPT.md](./docs/CONCEPT.md).

## Architecture

- Tauri 2 and Rust own the desktop lifecycle, durable session state, recovery, jobs, permissions, and native integration.
- React and TypeScript render the single-window workbench.
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

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
npm install
npm run dev:tauri
```

`npm run dev:tauri` is the standard Tauri development entry point. It builds the
Debug Native sidecars before starting Tauri and uses a dedicated Cargo target
directory at `.artifacts/cargo-tauri-dev`.

Do not use `npm run tauri dev` for normal development after changing Native
code: it bypasses the sidecar build wrapper and can run an older sidecar.

To open a project in recovery-oriented Safe Mode, pass the explicit flag (or set
`RIFFRA_SAFE_MODE=1`). Safe Mode keeps VST3 discovery, MIDI input, driver
changes, live sample preview, and new hardware recordings isolated while still
allowing project open, library access, offline analysis/render, and manifest
export/import:

```powershell
npm run dev:tauri -- --safe-mode
```

Run the non-GUI checks with:

```powershell
npm run verify
```

The verification script uses `.artifacts/verify/cargo` as its Cargo target
directory. This is intentionally separate from the Tauri development target,
so Rust tests and Clippy can run while `npm run dev:tauri` is active without
competing for Cargo's build lock.

Run the same checks plus a Native sidecar build and Native self-tests:

```powershell
npm run verify:native
```

`npm run verify:native` also runs the Native Timeline, Arrangement Graph,
Track-isolated Arrange recording, legacy recording, and safety self-tests. It
is the standard verification entry point for changes that affect the audio
engine; run it instead of invoking a generated Native executable directly.

`build-native.ps1` places the two debug sidecars under `src-tauri/binaries/`. The sidecars are intentionally ignored by Git because they are platform-specific build outputs; rebuild them after a fresh checkout before running a Tauri build.

## Licensing note

JUCE framework modules are dual-licensed under AGPLv3 and a commercial JUCE licence. A local development build must comply with one of those options. Distribution terms for Riffra will be finalized before a distributable installer is produced; adding JUCE does not by itself grant a proprietary redistribution right. The VST3 SDK used by current JUCE releases is MIT-licensed, while the optional ASIO dependency has separate terms.
