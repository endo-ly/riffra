# Riffra native audio engine

This sidecar owns the real-time timing domain. The Tauri process supervises it and never runs audio callbacks or third-party plugin code.

Current executable modes:

- `riffra-audio.exe --probe` enumerates ASIO/WASAPI device types without opening an audio stream.
- `riffra-audio.exe --serve` opens the default device in emergency-mute state and accepts one JSON command per stdin line.
- `riffra-audio.exe --safety-self-test` exercises the DC blocker and feedback detector with synthetic data without accessing a hardware input.
- `riffra-audio.exe --recording-self-test <directory>` writes and reopens a synthetic Raw/Processed take without accessing a hardware input.

The safety chain is deliberately small and auditable: immediate emergency mute, −18 dB conservative startup gain, 500 ms fade-in after unmute, non-finite sample rejection, a 0.98 hard ceiling, DC offset blocking on the output path, and acoustic feedback detection that auto-mutes when sustained near-peak input is observed. VST3 loading runs in the sidecar through an isolated rack; click-free rack transitions and dropout accounting remain explicit follow-on gates. Both effect plugins (stereo input bus required) and instrument plugins (no input bus, MIDI driven) are supported. `sendMidi` enqueues raw MIDI bytes to the loaded plugin's MIDI buffer and is intended for testing and headless rendering.

## Protocol examples

```json
{"type":"status"}
{"type":"setEmergencyMute","muted":false}
{"type":"setMasterGainDb","gainDb":-24.0}
{"type":"loadPlugin","path":"C:\\Program Files\\Common Files\\VST3\\IK Multimedia\\AmpliTube 5\\AmpliTube 5.vst3"}
{"type":"setPluginBypassed","bypassed":true}
{"type":"openPluginEditor"}
{"type":"sendMidi","bytes":[144,60,100]}
{"type":"recoverAudioDevice"}
{"type":"clearPlugin"}
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

- `riffra-audio.exe --recording-self-test <directory>` writes and reopens a synthetic Raw/Processed take without accessing a hardware input.
