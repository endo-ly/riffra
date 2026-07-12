# Riffra native audio engine

This sidecar owns the real-time timing domain. The Tauri process supervises it and never runs audio callbacks or third-party plugin code.

Current executable modes:

- `riffra-audio.exe --probe` enumerates ASIO/WASAPI device types without opening an audio stream.
- `riffra-audio.exe --serve` opens the default device in emergency-mute state and accepts one JSON command per stdin line.

The first safety chain is deliberately small and auditable: immediate emergency mute, −18 dB conservative startup gain, 500 ms fade-in after unmute, non-finite sample rejection, and a 0.98 hard ceiling. VST3 loading runs in the sidecar through an isolated rack; click-free rack transitions, dropout accounting, and recoverable raw/processed recording remain explicit follow-on gates.

## Protocol examples

```json
{"type":"status"}
{"type":"setEmergencyMute","muted":false}
{"type":"setMasterGainDb","gainDb":-24.0}
{"type":"loadPlugin","path":"C:\\Program Files\\Common Files\\VST3\\IK Multimedia\\AmpliTube 5\\AmpliTube 5.vst3"}
{"type":"setPluginBypassed","bypassed":true}
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

- `riffra-audio.exe --recording-self-test <directory>` writes and reopens a synthetic Raw/Processed take without accessing a hardware input.
