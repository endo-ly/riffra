use serde::Serialize;
use ts_rs::TS;

/// A complete versioned timeline snapshot sent to the native audio engine.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TimelineSnapshot {
    pub(crate) project_id: String,
    /// Arrangement revision used for diagnostics and transport display.
    pub(crate) revision: u64,
    pub(crate) graph: ExecutionGraph,
}

/// Executable timeline state shared by live playback and offline rendering.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecutionGraph {
    pub(crate) timebase: GraphTimebase,
    pub(crate) loop_range: GraphLoopRange,
    pub(crate) punch_range: Option<GraphTickRange>,
    pub(crate) metronome_enabled: bool,
    pub(crate) master_gain_db: f64,
    pub(crate) tracks: Vec<GraphTrack>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphTimebase {
    pub(crate) ppq: u32,
    pub(crate) bpm: f64,
    pub(crate) time_signature_numerator: u8,
    pub(crate) time_signature_denominator: u8,
}

/// Disabled loop ranges retain their endpoints for recording status.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphLoopRange {
    pub(crate) enabled: bool,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphTickRange {
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphTrack {
    pub(crate) id: String,
    pub(crate) kind: GraphTrackKind,
    pub(crate) gain_db: f64,
    pub(crate) pan: f64,
    pub(crate) muted: bool,
    pub(crate) solo: bool,
    pub(crate) armed: bool,
    pub(crate) monitoring: GraphMonitoring,
    pub(crate) audio_input: Option<GraphAudioInput>,
    pub(crate) midi_input: GraphMidiInput,
    pub(crate) volume_automation: Vec<GraphAutomationPoint>,
    pub(crate) pan_automation: Vec<GraphAutomationPoint>,
    pub(crate) effects: Vec<GraphPluginDevice>,
    pub(crate) instrument: Option<GraphInstrument>,
    pub(crate) audio_clips: Vec<GraphAudioClip>,
    pub(crate) midi_clips: Vec<GraphMidiClip>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GraphTrackKind {
    Audio,
    Instrument,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GraphMonitoring {
    Off,
    Auto,
    On,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphAudioInput {
    pub(crate) channel_index: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphMidiInput {
    pub(crate) device_id: Option<String>,
    pub(crate) channel: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphAutomationPoint {
    pub(crate) tick: u64,
    pub(crate) value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphPluginDevice {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) state: GraphPluginState,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphPluginState {
    pub(crate) state_data: Option<String>,
    pub(crate) parameter_values: Vec<f32>,
    pub(crate) bypassed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum GraphInstrument {
    Vst3 {
        id: String,
        path: String,
        state: GraphPluginState,
    },
    Internal {
        id: String,
        bypassed: bool,
        definition_json: String,
        definition_base_dir: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphAudioClip {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) source_sample_rate: u32,
    pub(crate) source_start_frame: u64,
    pub(crate) source_end_frame: u64,
    pub(crate) duration_frames: u64,
    pub(crate) duration_sample_rate: u32,
    pub(crate) start_tick: u64,
    pub(crate) fade_in_frames: u64,
    pub(crate) fade_out_frames: u64,
    pub(crate) fade_shape: GraphFadeShape,
    pub(crate) gain_db: f64,
    pub(crate) pan: f64,
    pub(crate) take_variant: GraphTakeVariant,
    pub(crate) loop_enabled: bool,
    pub(crate) muted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GraphFadeShape {
    Linear,
    EqualPower,
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GraphTakeVariant {
    Raw,
    Processed,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphMidiClip {
    pub(crate) id: String,
    pub(crate) start_tick: u64,
    pub(crate) duration_ticks: u64,
    pub(crate) loop_enabled: bool,
    pub(crate) muted: bool,
    pub(crate) notes: Vec<GraphMidiNote>,
    pub(crate) events: Vec<GraphMidiEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphMidiNote {
    pub(crate) start_tick: u64,
    pub(crate) duration_ticks: u64,
    pub(crate) note: u8,
    pub(crate) velocity: u8,
    pub(crate) channel: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphMidiEvent {
    pub(crate) kind: GraphMidiEventKind,
    pub(crate) tick: u64,
    pub(crate) channel: u8,
    pub(crate) data1: u8,
    pub(crate) data2: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GraphMidiEventKind {
    ControlChange,
    PitchBend,
    ChannelPressure,
}

/// A request sent to the offline `riffra-render` process.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OfflineRenderRequest {
    pub(crate) graph: ExecutionGraph,
    pub(crate) destination: String,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) sample_rate: u32,
    pub(crate) block_size: u32,
    pub(crate) normalize: bool,
}

/// Resources missing from an executable projection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionDiagnostics {
    pub unavailable_clip_ids: Vec<String>,
    pub missing_device_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectedTimeline {
    pub(crate) snapshot: TimelineSnapshot,
    pub(crate) diagnostics: ProjectionDiagnostics,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/execution-graph")
    }

    fn state(state_data: Option<&str>, bypassed: bool) -> GraphPluginState {
        GraphPluginState {
            state_data: state_data.map(str::to_owned),
            parameter_values: vec![0.25, 0.75],
            bypassed,
        }
    }

    fn full_snapshot() -> TimelineSnapshot {
        let shared = |kind: GraphTrackKind,
                      monitoring: GraphMonitoring,
                      audio_input: Option<GraphAudioInput>,
                      device_id: Option<&str>,
                      channel: Option<u8>,
                      instrument: Option<GraphInstrument>,
                      audio_clips: Vec<GraphAudioClip>| {
            GraphTrack {
                id: format!("track-{}", matches!(kind, GraphTrackKind::Instrument)),
                kind,
                gain_db: 1.5,
                pan: -0.25,
                muted: false,
                solo: true,
                armed: true,
                monitoring,
                audio_input,
                midi_input: GraphMidiInput {
                    device_id: device_id.map(str::to_owned),
                    channel,
                },
                volume_automation: vec![GraphAutomationPoint {
                    tick: 0,
                    value: 1.0,
                }],
                pan_automation: vec![GraphAutomationPoint {
                    tick: 960,
                    value: -0.5,
                }],
                effects: vec![GraphPluginDevice {
                    id: "fx-1".into(),
                    path: "plugins/fx.vst3".into(),
                    state: state(Some("opaque-state"), false),
                }],
                instrument,
                audio_clips,
                midi_clips: vec![GraphMidiClip {
                    id: "midi-1".into(),
                    start_tick: 120,
                    duration_ticks: 960,
                    loop_enabled: true,
                    muted: false,
                    notes: vec![GraphMidiNote {
                        start_tick: 0,
                        duration_ticks: 480,
                        note: 60,
                        velocity: 100,
                        channel: 1,
                    }],
                    events: vec![GraphMidiEvent {
                        kind: GraphMidiEventKind::ControlChange,
                        tick: 100,
                        channel: 1,
                        data1: 1,
                        data2: 2,
                    }],
                }],
            }
        };
        let audio_clips = vec![
            GraphAudioClip {
                id: "audio-linear-raw".into(),
                path: "audio/one.wav".into(),
                source_sample_rate: 48_000,
                source_start_frame: 0,
                source_end_frame: 48_000,
                duration_frames: 48_000,
                duration_sample_rate: 48_000,
                start_tick: 0,
                fade_in_frames: 0,
                fade_out_frames: 0,
                fade_shape: GraphFadeShape::Linear,
                gain_db: 0.0,
                pan: 0.0,
                take_variant: GraphTakeVariant::Raw,
                loop_enabled: false,
                muted: false,
            },
            GraphAudioClip {
                id: "audio-equal-processed".into(),
                path: "audio/two.wav".into(),
                source_sample_rate: 44_100,
                source_start_frame: 1,
                source_end_frame: 44_101,
                duration_frames: 44_100,
                duration_sample_rate: 44_100,
                start_tick: 960,
                fade_in_frames: 128,
                fade_out_frames: 256,
                fade_shape: GraphFadeShape::EqualPower,
                gain_db: -3.0,
                pan: 0.5,
                take_variant: GraphTakeVariant::Processed,
                loop_enabled: true,
                muted: true,
            },
            GraphAudioClip {
                id: "audio-smooth".into(),
                path: "audio/three.wav".into(),
                source_sample_rate: 48_000,
                source_start_frame: 0,
                source_end_frame: 24_000,
                duration_frames: 24_000,
                duration_sample_rate: 48_000,
                start_tick: 1920,
                fade_in_frames: 32,
                fade_out_frames: 64,
                fade_shape: GraphFadeShape::Smooth,
                gain_db: 2.0,
                pan: -0.5,
                take_variant: GraphTakeVariant::Raw,
                loop_enabled: false,
                muted: false,
            },
        ];
        let mut audio = shared(
            GraphTrackKind::Audio,
            GraphMonitoring::Off,
            Some(GraphAudioInput { channel_index: 2 }),
            None,
            None,
            None,
            audio_clips,
        );
        audio.id = "track-audio".into();
        audio.midi_clips.clear();
        audio.instrument = None;
        audio.effects.clear();
        let mut internal = shared(
            GraphTrackKind::Instrument,
            GraphMonitoring::Auto,
            None,
            Some("midi-device"),
            Some(16),
            Some(GraphInstrument::Internal {
                id: "instrument-internal".into(),
                bypassed: true,
                definition_json: "{\"schemaVersion\":1}".into(),
                definition_base_dir: "instruments/builtin/preset".into(),
            }),
            Vec::new(),
        );
        internal.id = "track-internal".into();
        internal.effects.clear();
        internal.midi_clips[0].events[0].kind = GraphMidiEventKind::PitchBend;
        let mut vst = shared(
            GraphTrackKind::Instrument,
            GraphMonitoring::On,
            None,
            None,
            None,
            Some(GraphInstrument::Vst3 {
                id: "instrument-vst".into(),
                path: "plugins/instrument.vst3".into(),
                state: state(None, false),
            }),
            Vec::new(),
        );
        vst.id = "track-vst".into();
        vst.midi_clips[0].events[0].kind = GraphMidiEventKind::ChannelPressure;
        vst.midi_clips[0].events.push(GraphMidiEvent {
            kind: GraphMidiEventKind::ControlChange,
            tick: 200,
            channel: 1,
            data1: 7,
            data2: 64,
        });
        TimelineSnapshot {
            project_id: "project-full".into(),
            revision: 23,
            graph: ExecutionGraph {
                timebase: GraphTimebase {
                    ppq: 960,
                    bpm: 123.5,
                    time_signature_numerator: 7,
                    time_signature_denominator: 8,
                },
                loop_range: GraphLoopRange {
                    enabled: true,
                    start_tick: 120,
                    end_tick: 3_840,
                },
                punch_range: Some(GraphTickRange {
                    start_tick: 240,
                    end_tick: 1_920,
                }),
                metronome_enabled: true,
                master_gain_db: -6.0,
                tracks: vec![audio, internal, vst],
            },
        }
    }

    fn minimal_snapshot() -> TimelineSnapshot {
        TimelineSnapshot {
            project_id: "project-minimal".into(),
            revision: 0,
            graph: ExecutionGraph {
                timebase: GraphTimebase {
                    ppq: 960,
                    bpm: 120.0,
                    time_signature_numerator: 4,
                    time_signature_denominator: 4,
                },
                loop_range: GraphLoopRange {
                    enabled: false,
                    start_tick: 0,
                    end_tick: 0,
                },
                punch_range: None,
                metronome_enabled: false,
                master_gain_db: 0.0,
                tracks: Vec::new(),
            },
        }
    }

    fn fixture_values() -> Vec<(&'static str, String)> {
        let minimal = minimal_snapshot();
        let full = full_snapshot();
        let graph = full.graph.clone();
        vec![
            (
                "timeline-snapshot-minimal.json",
                serde_json::to_string_pretty(&minimal).unwrap(),
            ),
            (
                "timeline-snapshot-full.json",
                serde_json::to_string_pretty(&full).unwrap(),
            ),
            (
                "offline-render-request.json",
                serde_json::to_string_pretty(&OfflineRenderRequest {
                    graph,
                    destination: "renders/song.wav".into(),
                    start_tick: 0,
                    end_tick: 7_680,
                    sample_rate: 48_000,
                    block_size: 512,
                    normalize: true,
                })
                .unwrap(),
            ),
            (
                "plugin-state.json",
                serde_json::to_string_pretty(&state(Some("opaque-state"), true)).unwrap(),
            ),
        ]
    }

    #[test]
    fn contract_fixtures_are_current() {
        let root = fixture_dir();
        let update = std::env::var("RIFFRA_UPDATE_CONTRACT_FIXTURES").as_deref() == Ok("1");
        if update {
            fs::create_dir_all(&root).unwrap();
        }
        for (name, expected) in fixture_values() {
            let path = root.join(name);
            if update {
                fs::write(path, &expected).unwrap();
            } else {
                let actual = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!(
                        "contract fixture {} is missing or unreadable: {error}",
                        path.display()
                    )
                });
                assert_eq!(
                    actual,
                    expected,
                    "contract fixture {} is stale",
                    path.display()
                );
            }
        }
    }
}
