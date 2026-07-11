# Riffra

Riffra is a Windows-first music production workbench built around a short creative loop: hear, shape, capture, compare, and reuse. The product contract is defined in [Windows統合音楽制作ワークベンチ_製品仕様書_改訂版.md](./Windows統合音楽制作ワークベンチ_製品仕様書_改訂版.md).

## Architecture

- Tauri 2 and Rust own the desktop lifecycle, durable session state, recovery, jobs, permissions, and native integration.
- React and TypeScript render the single-window workbench.
- A native C++20/JUCE sidecar owns real-time audio, MIDI, ASIO/WASAPI, VST3 hosting, metering, recording, and render paths.
- Plugin discovery and plugin execution cross process boundaries so a bad plugin cannot corrupt the UI or saved session state.
- SQLite will index reusable assets; portable project and rack manifests remain versioned JSON with standard audio/MIDI files beside them.

The detailed boundaries and staged acceptance gates are in [docs/architecture.md](./docs/architecture.md) and [docs/spec-traceability.md](./docs/spec-traceability.md).

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
.\scripts\build-native.ps1 -Configuration Debug
npm run tauri dev
```

Run the non-GUI checks with:

```powershell
npm run check
Push-Location src-tauri
cargo test
Pop-Location
```

`build-native.ps1` places the two debug sidecars under `src-tauri/binaries/`. The sidecars are intentionally ignored by Git because they are platform-specific build outputs; rebuild them after a fresh checkout before running a Tauri build.

## Licensing note

JUCE framework modules are dual-licensed under AGPLv3 and a commercial JUCE licence. A local development build must comply with one of those options. Distribution terms for Riffra will be finalized before a distributable installer is produced; adding JUCE does not by itself grant a proprietary redistribution right. The VST3 SDK used by current JUCE releases is MIT-licensed, while the optional ASIO dependency has separate terms.
