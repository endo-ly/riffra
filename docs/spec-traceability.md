# Specification traceability

This is the living completion ledger for specification version 2.0. `Implemented` means executable behavior with an automated or documented manual acceptance check; a visible placeholder does not count.

| Product acceptance area | Owning subsystem | Current gate | Evidence required |
| --- | --- | --- | --- |
| A. Instant Play | audio sidecar + session recovery | In progress | emergency-muted startup, muted device recovery with rack reprepare, validated rack restoration request, native input/output peak metering and Windows WASAPI/ASIO/MIDI discovery with muted driver switching; cold/warm timing remains |
| B. Tone Design | plugin workers + rack engine | In progress | isolated real VST3 load/process, persisted rack path, explicit safety-preserving bypass and session A/B rack snapshots; plugin parameter state, macro/parallel rack and matched A/B remain |
| C. Capture | audio sidecar + recording journal | In progress | JUCE threaded Raw/Processed WAV, completed/recoverable manifest, Inbox recording index, synthetic self-test; hardware interruption matrix remains |
| D. Arrange | timeline engine + offline render | In progress | non-destructive Inbox audio clip references with editable position/gain/mute and a new stereo float WAV render; MIDI edit, PDC and stem/master export remain |
| E. Sample | sampler preview bus + MIDI monitor | In progress | non-destructive source-to-pad mapping with editable slice range, native safety-limited WAV audition, Windows MIDI port discovery and explicit input monitoring; keyboard sampler and MIDI-triggered pad playback/record remain |
| F. Analyze | analysis workers | In progress | offline WAV 128-bin waveform, peak/RMS, duration, zero crossings, simple spectrum peak, stereo phase correlation and read-only Reference delta/loudness-match metrics; synchronized reference playback remains |
| G. Separate | background job provider | In progress | manifest-backed offline stereo channel split with immutable Left/Right WAV outputs; model-based cancellable stems and synchronized comparison remain |
| H. AI | reversible suggestion service | In progress | offline Reference ChangeSet preview shows target/current/proposed/reason/effect/risk with selected apply, reject and Undo coverage; general context control, provider permissions and external-send control remain |
| I. Creative Memory | library catalog + provenance manifests | In progress | VST3 catalog, recording-manifest search/listing, rack/session provenance sidecars and explicit portable session-manifest export/import are available; SQLite cross-asset index, related assets and non-destructive preview remain |
| J. Recovery | supervisor + autosave generations | In progress | corrupted current save fallback, explicit `--safe-mode` startup isolation, and persisted VST3 placeholders marked Missing dependency; stable-version choice and discard-recovery UI remain |

## Gate 1 acceptance checks

- App opens directly into a Scratch Session without a project dialog.
- Session state is written atomically and at least five previous generations are kept.
- Corrupt `current.json` falls back to the newest valid generation.
- Session edits expose bounded Undo/Redo (40 in-memory revisions) while autosave and generation recovery remain the durable safety net.
- Home, Play, Arrange, Sample, Analyze and Separate share transport and selection state.
- Emergency mute is reachable by pointer and keyboard from every workspace.
- The default VST3 folder is discovered without blocking initial UI rendering.
- Empty/loading/error states state what happened, affected scope, data safety and available action.

## Quality accounting

Each later feature must add its own automated unit/integration tests and a manual hardware matrix entry. Performance claims must record driver, interface, sample rate, buffer size, plugin set, Windows build and test duration.
