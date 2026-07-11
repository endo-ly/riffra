# Specification traceability

This is the living completion ledger for specification version 2.0. `Implemented` means executable behavior with an automated or documented manual acceptance check; a visible placeholder does not count.

| Product acceptance area | Owning subsystem | Current gate | Evidence required |
| --- | --- | --- | --- |
| A. Instant Play | audio sidecar + session recovery | In progress | emergency-muted startup, muted device recovery with rack reprepare, validated rack restoration request; cold/warm timing remains |
| B. Tone Design | plugin workers + rack engine | In progress | isolated real VST3 load/process, persisted rack path, explicit safety-preserving bypass and session A/B rack snapshots; plugin parameter state, macro/parallel rack and matched A/B remain |
| C. Capture | audio sidecar + recording journal | In progress | JUCE threaded Raw/Processed WAV, completed/recoverable manifest, Inbox recording index, synthetic self-test; hardware interruption matrix remains |
| D. Arrange | timeline engine + offline render | Planned | non-destructive audio/MIDI edit, PDC, stem/master export |
| E. Sample | sampler engine | Planned | slice, pad/key map, MIDI play/record, reusable instrument |
| F. Analyze | analysis workers | Planned | waveform, spectrum, loudness, phase and matched reference |
| G. Separate | background job provider | Planned | cancellable/recoverable stems and synchronized comparison |
| H. AI | Rust ChangeSet service | Planned | context preview, partial apply, one-step undo, external-send control |
| I. Creative Memory | SQLite library + preview bus | In progress | VST3 catalog and recording-manifest search/listing started; SQLite cross-asset provenance, related assets and non-destructive preview remain |
| J. Recovery | supervisor + autosave generations | In progress | corrupted current save fallback, safe mode, missing dependency open |

## Gate 1 acceptance checks

- App opens directly into a Scratch Session without a project dialog.
- Session state is written atomically and at least five previous generations are kept.
- Corrupt `current.json` falls back to the newest valid generation.
- Home, Play, Arrange, Sample, Analyze and Separate share transport and selection state.
- Emergency mute is reachable by pointer and keyboard from every workspace.
- The default VST3 folder is discovered without blocking initial UI rendering.
- Empty/loading/error states state what happened, affected scope, data safety and available action.

## Quality accounting

Each later feature must add its own automated unit/integration tests and a manual hardware matrix entry. Performance claims must record driver, interface, sample rate, buffer size, plugin set, Windows build and test duration.
