# Riffra architecture

## Product boundaries

Riffra separates work by failure and timing domain, not by screen.

```text
Tauri main process (Rust)
├─ session/project model, commands, undo journal
├─ autosave, versions, recovery and diagnostics
├─ library index, jobs, AI permission/change sets
├─ Windows lifecycle and credential integration
└─ native audio supervisor
   ├─ audio engine sidecar (C++/JUCE, real-time priority)
   ├─ plugin scan workers (one disposable process per candidate)
   └─ heavy-job workers (analysis, render, separation)

WebView UI (React/TypeScript)
├─ global bar, library, workspace and inspector
├─ Home / Play / Arrange / Sample / Analyze / Separate
└─ presentation state only; durable audio/session truth stays native
```

## Non-negotiable invariants

1. The audio callback never performs allocation, file I/O, logging, UI work, scanning, or network work.
2. Startup gain is conservative, fades in, and passes through a master limiter. Emergency mute remains available in every workspace.
3. A plugin scan crash is contained to its worker. A hosted plugin crash may restart its audio worker but cannot take down the Tauri UI or corrupt the last stable session.
4. Recording writes recoverable chunks continuously. Raw input and processed output are separate immutable sources.
5. Every destructive-looking edit is a reference or operation until explicitly flattened. Undo, autosave, and recovery share one versioned operation model.
6. Projects open with missing files or plugins by creating disabled placeholders with provenance and state blobs intact.
7. AI produces a previewable ChangeSet. Applying it is explicit, partially selectable, logged, and reversible.

## Data layout

The default application data root is resolved through the Windows application data API, never from the current working directory.

```text
Riffra/
├─ scratch/
│  ├─ current.json
│  └─ generations/*.json
├─ library/riffra.db
├─ recordings/inbox/<recording-id>/
│  ├─ raw.wav.partial
│  ├─ processed.wav.partial
│  └─ manifest.json
├─ plugins/catalog.json
├─ jobs/
├─ logs/
└─ recovery/
```

Project packages use a versioned JSON manifest plus standard WAV/FLAC/MIDI assets. Plugin binary files are never copied into a package. Rendered fallbacks and complete plugin state blobs may be stored beside the manifest.

## Process communication

Control messages use length-prefixed, versioned messages over Windows named pipes. Metering and audio blocks use bounded shared-memory rings with monotonic sequence counters. Every message carries protocol version, request id, deadline, and explicit error scope. The UI never talks directly to plugin workers.

## Delivery gates

1. Shell and memory: startup, Scratch Session, generational autosave, recovery choice, all main workspaces, command palette, keyboard language.
2. Sound first: WASAPI playback/input, ASIO, MIDI, safety chain, meters, hot-plug, quick raw/processed recording.
3. Tone: isolated scan, VST3 hosting, common parameter view, rack graph, parallel path, macro, snapshot, loudness-matched compare.
4. Creation: timeline, sample instruments, analysis/reference, background separation, reversible AI changes.
5. Ownership and release: unified indexed library, portable package, DAW handoff, rendered fallback, crash/soak/performance tests, MSIX/installer and uninstall data choice.

