use super::*;
use riffra_core::{
    AudioTakeVariant, CreativeSession, FadeShape, InternalInstrumentResource, MidiEventKind,
    MonitoringState, Track, TrackInstrumentSource, TrackKind, Vst3Plugin,
};
use std::collections::HashSet;

/// Builds a live snapshot from canonical session state and already-resolved resources.
pub(crate) fn project(
    project_id: &str,
    session: &CreativeSession,
    resources: &ResolvedResources,
) -> ProjectedTimeline {
    let (graph, diagnostics) = project_graph(session, resources);
    ProjectedTimeline {
        snapshot: TimelineSnapshot {
            project_id: project_id.to_owned(),
            revision: session.arrangement.revision,
            graph,
        },
        diagnostics,
    }
}

/// Projects canonical arrangement state without performing I/O.
pub(crate) fn project_graph(
    session: &CreativeSession,
    resources: &ResolvedResources,
) -> (ExecutionGraph, ProjectionDiagnostics) {
    let arrangement = &session.arrangement;
    let mut diagnostics = ProjectionDiagnostics::default();
    for clip in &arrangement.audio_clips {
        if !resources.audio_paths.contains_key(&clip.asset_id) {
            diagnostics.unavailable_clip_ids.push(clip.id.clone());
        }
    }

    let tracks = arrangement
        .tracks
        .iter()
        .map(|track| {
            let instrument =
                track
                    .instrument
                    .as_ref()
                    .and_then(|instrument| match &instrument.source {
                        TrackInstrumentSource::Vst3(plugin) => plugin_state(
                            &instrument.id,
                            plugin,
                            instrument.bypassed,
                            resources,
                            &mut diagnostics,
                        )
                        .map(|state| GraphInstrument::Vst3 {
                            id: instrument.id.clone(),
                            path: plugin.path.clone(),
                            state,
                            midi: instrument.midi.clone(),
                        }),
                        TrackInstrumentSource::Internal {
                            definition_json,
                            resource: InternalInstrumentResource::BuiltInPreset { preset_id },
                        } => resources
                            .built_in_base_dirs
                            .get(preset_id)
                            .map(|base_dir| GraphInstrument::Internal {
                                id: instrument.id.clone(),
                                bypassed: instrument.bypassed,
                                definition_json: definition_json.clone(),
                                definition_base_dir: base_dir.to_string_lossy().into_owned(),
                                midi: instrument.midi.clone(),
                            })
                            .or_else(|| {
                                diagnostics.missing_device_ids.push(instrument.id.clone());
                                None
                            }),
                        TrackInstrumentSource::Internal {
                            definition_json,
                            resource: InternalInstrumentResource::UserSnapshot { snapshot_id, .. },
                        } => Some(GraphInstrument::Internal {
                            id: instrument.id.clone(),
                            bypassed: instrument.bypassed,
                            definition_json: definition_json.clone(),
                            definition_base_dir: resources
                                .data_root
                                .join("project-instruments")
                                .join(snapshot_id)
                                .to_string_lossy()
                                .into_owned(),
                            midi: instrument.midi.clone(),
                        }),
                    });

            let effects = project_effects(&track.effects, resources, &mut diagnostics);

            let audio_clips = arrangement
                .audio_clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
                .filter_map(|clip| {
                    let path = resources.audio_paths.get(&clip.asset_id)?;
                    Some(GraphAudioClip {
                        id: clip.id.clone(),
                        path: path.to_string_lossy().into_owned(),
                        source_sample_rate: clip.source_sample_rate,
                        source_start_frame: clip.source_range.start,
                        source_end_frame: clip.source_range.end,
                        duration_frames: clip.timeline_duration.frames,
                        duration_sample_rate: clip.timeline_duration.sample_rate,
                        start_tick: clip.start_tick.0,
                        fade_in_frames: clip.fade_in.frames,
                        fade_out_frames: clip.fade_out.frames,
                        fade_shape: match clip.fade_shape {
                            FadeShape::Linear => GraphFadeShape::Linear,
                            FadeShape::EqualPower => GraphFadeShape::EqualPower,
                            FadeShape::Smooth => GraphFadeShape::Smooth,
                        },
                        gain_db: clip.gain_db,
                        pan: clip.pan,
                        take_variant: match clip.take_variant {
                            AudioTakeVariant::Raw => GraphTakeVariant::Raw,
                            AudioTakeVariant::Processed => GraphTakeVariant::Processed,
                        },
                        loop_enabled: clip.loop_enabled,
                        muted: clip.muted,
                    })
                })
                .collect();

            let midi_clips = arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
                .map(|clip| GraphMidiClip {
                    instrument_control_events: clip.instrument_control_events.clone(),
                    id: clip.id.clone(),
                    start_tick: clip.start_tick.0,
                    duration_ticks: clip.duration_ticks,
                    loop_enabled: clip.loop_enabled,
                    muted: clip.muted,
                    notes: clip
                        .notes
                        .iter()
                        .map(|note| GraphMidiNote {
                            start_tick: note.start_tick.0,
                            duration_ticks: note.duration_ticks,
                            note: note.note,
                            velocity: note.velocity,
                            channel: note.channel,
                        })
                        .collect(),
                    events: clip
                        .events
                        .iter()
                        .map(|event| GraphMidiEvent {
                            kind: match event.kind {
                                MidiEventKind::ControlChange => GraphMidiEventKind::ControlChange,
                                MidiEventKind::PitchBend => GraphMidiEventKind::PitchBend,
                                MidiEventKind::ChannelPressure => {
                                    GraphMidiEventKind::ChannelPressure
                                }
                            },
                            tick: event.tick.0,
                            channel: event.channel,
                            data1: event.data1,
                            data2: event.data2,
                        })
                        .collect(),
                })
                .collect();

            GraphTrack {
                pan_law: track.pan_law,
                external_audio_source_track_ids: track.external_audio_source_track_ids.clone(),
                output_bus_id: track.output_bus_id.clone(),
                id: track.id.clone(),
                kind: match track.kind {
                    TrackKind::Audio => GraphTrackKind::Audio,
                    TrackKind::Instrument => GraphTrackKind::Instrument,
                },
                gain_db: track.gain_db,
                pan: track.pan,
                muted: track.muted,
                solo: track.solo,
                armed: track.armed,
                monitor_input: monitors_audio_input(track),
                audio_input: track.audio_input.map(|route| GraphAudioInput {
                    channel_index: route.channel_index,
                }),
                midi_input: GraphMidiInput {
                    device_id: track.midi_input.device_id.clone(),
                    channel: track.midi_input.channel,
                },
                effects,
                instrument,
                audio_clips,
                midi_clips,
            }
        })
        .collect();

    let mixer = GraphMixer {
        master_effects: project_effects(
            &arrangement.mixer.master_effects,
            resources,
            &mut diagnostics,
        ),
        buses: arrangement
            .mixer
            .buses
            .iter()
            .map(|bus| GraphMixBus {
                id: bus.id.clone(),
                kind: bus.kind,
                gain_db: bus.gain_db,
                pan: bus.pan,
                pan_law: bus.pan_law,
                muted: bus.muted,
                solo: bus.solo,
                effects: project_effects(&bus.effects, resources, &mut diagnostics),
            })
            .collect(),
        sends: arrangement.mixer.sends.clone(),
        sidechain_sends: arrangement.mixer.sidechain_sends.clone(),
    };
    let mut graph = ExecutionGraph {
        mixer,
        mixdown: GraphMixdown {
            musical_end_tick: session.settings.mixdown.musical_end_tick,
            tail_seconds: session.settings.mixdown.tail_seconds,
            fade_out_seconds: session.settings.mixdown.fade_out_seconds,
        },
        timebase: arrangement.timebase.clone(),
        loop_range: GraphLoopRange {
            enabled: arrangement.loop_range.enabled,
            start_tick: arrangement.loop_range.start_tick.0,
            end_tick: arrangement.loop_range.end_tick.0,
        },
        punch_range: arrangement.punch_range.map(|range| GraphTickRange {
            start_tick: range.start_tick.0,
            end_tick: range.end_tick.0,
        }),
        metronome_enabled: session.settings.metronome_enabled,
        metronome_sound: session.settings.metronome_sound,
        master_gain_db: session.settings.master_db,
        automation: arrangement
            .automation_lanes
            .iter()
            .map(|lane| GraphAutomationLane {
                target: lane.target.clone(),
                interpolation: lane.interpolation,
                points: lane
                    .points
                    .iter()
                    .map(|point| GraphAutomationPoint {
                        tick: point.tick.0,
                        value: point.value,
                    })
                    .collect(),
            })
            .collect(),
        tracks,
    };
    let executable_effect_ids: HashSet<_> = graph
        .tracks
        .iter()
        .flat_map(|track| track.effects.iter())
        .chain(graph.mixer.buses.iter().flat_map(|bus| bus.effects.iter()))
        .chain(graph.mixer.master_effects.iter())
        .map(|effect| effect.id.as_str())
        .collect();
    graph
        .mixer
        .sidechain_sends
        .retain(|send| executable_effect_ids.contains(send.target_device_id.as_str()));
    let executable_sidechain_ids: HashSet<_> = graph
        .mixer
        .sidechain_sends
        .iter()
        .map(|send| send.id.as_str())
        .collect();
    graph.automation.retain(|lane| match &lane.target {
        riffra_core::AutomationTarget::SidechainSendLevel { sidechain_send_id } => {
            executable_sidechain_ids.contains(sidechain_send_id.as_str())
        }
        _ => true,
    });

    (graph, diagnostics)
}

fn project_effects(
    effects: &[riffra_core::EffectDevice],
    resources: &ResolvedResources,
    diagnostics: &mut ProjectionDiagnostics,
) -> Vec<GraphPluginDevice> {
    effects
        .iter()
        .filter_map(|device| {
            plugin_state(
                &device.id,
                &device.plugin,
                device.bypassed,
                resources,
                diagnostics,
            )
            .map(|state| GraphPluginDevice {
                id: device.id.clone(),
                path: device.plugin.path.clone(),
                state,
            })
        })
        .collect()
}

/// Decides whether a Track monitors its physical audio input: an Audio Track
/// set to `On`, or set to `Auto` while armed.
fn monitors_audio_input(track: &Track) -> bool {
    track.kind == TrackKind::Audio
        && match track.monitoring {
            MonitoringState::On => true,
            MonitoringState::Auto => track.armed,
            MonitoringState::Off => false,
        }
}

/// Projects an enabled plugin whose path is installed. A plugin that is not
/// installed is recorded as a missing device; a disabled placeholder is
/// skipped.
fn plugin_state(
    device_id: &str,
    plugin: &Vst3Plugin,
    bypassed: bool,
    resources: &ResolvedResources,
    diagnostics: &mut ProjectionDiagnostics,
) -> Option<GraphPluginState> {
    if plugin.disabled_placeholder {
        return None;
    }
    if !resources.existing_plugin_paths.contains(&plugin.path) {
        diagnostics.missing_device_ids.push(device_id.to_owned());
        return None;
    }
    Some(GraphPluginState {
        state_data: plugin.state_data.clone(),
        parameter_values: plugin.parameter_values.clone(),
        bypassed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{
        AudioClip, AudioInputRoute, AutomationLane, AutomationPoint, AutomationTarget,
        EffectDevice, MidiClip, MidiEvent, MidiInputRoute, MidiNote, Track, TrackInstrument,
        TrackMixParameter,
    };
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn plugin_device(id: &str, name: &str, path: &str, disabled: bool) -> EffectDevice {
        let mut device = EffectDevice::new(id.into(), name.into(), path.into()).unwrap();
        device.plugin.disabled_placeholder = disabled;
        device
    }

    fn resources() -> ResolvedResources {
        ResolvedResources::for_projection(
            PathBuf::from("data-root"),
            HashMap::new(),
            HashSet::from(["fx.vst3".to_owned(), "instrument.vst3".to_owned()]),
            HashMap::from([("preset".to_owned(), PathBuf::from("builtins/preset"))]),
        )
    }

    #[test]
    fn projects_execution_fields_and_ignores_presentation_metadata() {
        let mut session = CreativeSession::new(1);
        session.arrangement.revision = 23;
        session.arrangement.timebase = riffra_core::ProjectTimebase {
            ppq: 960,
            tempo_changes: vec![riffra_core::TempoChange {
                tick: 0,
                bpm: 123.5,
            }],
            time_signature_changes: vec![riffra_core::TimeSignatureChange {
                tick: 0,
                numerator: 7,
                denominator: 8,
            }],
        };
        session.arrangement.loop_range = riffra_core::TimelineLoopRange {
            enabled: true,
            start_tick: riffra_core::TimelineTick(120),
            end_tick: riffra_core::TimelineTick(3_840),
        };
        session.arrangement.punch_range = Some(riffra_core::TimelinePunchRange {
            start_tick: riffra_core::TimelineTick(240),
            end_tick: riffra_core::TimelineTick(1_920),
        });
        session.settings.master_db = -6.0;
        session.settings.metronome_enabled = true;
        session.settings.metronome_sound = riffra_core::MetronomeSound::Wood;

        let mut audio = Track::audio("track:audio".into(), "Audio".into());
        audio.gain_db = -3.0;
        audio.pan = 0.25;
        audio.muted = true;
        audio.solo = true;
        audio.armed = true;
        audio.monitoring = MonitoringState::Auto;
        audio.audio_input = Some(AudioInputRoute { channel_index: 3 });
        audio
            .effects
            .push(plugin_device("device:fx", "FX", "fx.vst3", false));
        let mut instrument = Track::instrument("track:instrument".into(), "Keys".into());
        instrument.midi_input = MidiInputRoute {
            device_id: Some("midi:1".into()),
            channel: Some(2),
        };
        instrument.gain_db = -4.0;
        instrument.pan = -0.5;
        instrument.muted = true;
        instrument.armed = true;
        instrument.monitoring = MonitoringState::On;
        instrument.instrument = Some(
            TrackInstrument::vst3(
                "instrument:vst3".into(),
                "VST".into(),
                "instrument.vst3".into(),
            )
            .unwrap(),
        );
        let mut built_in = Track::instrument("track:internal".into(), "Internal".into());
        built_in.instrument = Some(
            TrackInstrument::built_in(
                "instrument:internal".into(),
                "Internal".into(),
                "preset".into(),
                "{\"schemaVersion\":1}".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks = vec![audio, instrument, built_in];
        session.arrangement.tracks[0].output_bus_id = Some("bus:group".into());
        for (id, kind) in [
            ("bus:group", riffra_core::MixBusKind::Group),
            ("bus:return", riffra_core::MixBusKind::Return),
        ] {
            session.arrangement.mixer.buses.push(riffra_core::MixBus {
                id: id.into(),
                name: id.into(),
                kind,
                gain_db: -2.0,
                pan: 0.2,
                pan_law: riffra_core::PanLaw::UnityCenterStereo,
                muted: false,
                solo: false,
                effects: vec![plugin_device(&format!("{id}:fx"), "FX", "fx.vst3", false)],
            });
        }
        session.arrangement.mixer.master_effects.push(plugin_device(
            "device:master",
            "FX",
            "fx.vst3",
            false,
        ));
        session.arrangement.mixer.sends.push(riffra_core::Send {
            id: "send:audio".into(),
            track_id: "track:audio".into(),
            return_bus_id: "bus:return".into(),
            level_db: -8.0,
            position: riffra_core::SendPosition::PostFader,
            enabled: true,
        });
        session
            .arrangement
            .mixer
            .sidechain_sends
            .push(riffra_core::SidechainSend {
                id: "sidechain:audio".into(),
                source_track_id: "track:audio".into(),
                target_device_id: "bus:return:fx".into(),
                level_db: -12.0,
                position: riffra_core::SendPosition::PreFader,
                enabled: false,
            });

        let audio_asset_id = riffra_core::mint_asset_id();
        let mut audio_clip = AudioClip::full_source(
            "clip:audio".into(),
            "Audio clip".into(),
            "track:audio".into(),
            audio_asset_id.clone(),
            riffra_core::TimelineTick(480),
            48_000,
            96_000,
        );
        audio_clip.source_range = riffra_core::FrameRange {
            start: 12,
            end: 96_000,
        };
        audio_clip.timeline_duration = riffra_core::FrameDuration {
            frames: 48_000,
            sample_rate: 44_100,
        };
        audio_clip.fade_in = riffra_core::FrameDuration {
            frames: 128,
            sample_rate: 48_000,
        };
        audio_clip.fade_out = riffra_core::FrameDuration {
            frames: 256,
            sample_rate: 48_000,
        };
        audio_clip.fade_shape = FadeShape::Smooth;
        audio_clip.gain_db = -2.0;
        audio_clip.pan = 0.5;
        audio_clip.take_variant = AudioTakeVariant::Processed;
        audio_clip.loop_enabled = true;
        audio_clip.muted = true;
        session.arrangement.audio_clips.push(audio_clip);
        let mut midi = MidiClip {
            instrument_control_events: Vec::new(),
            id: "clip:midi".into(),
            name: "MIDI".into(),
            track_id: "track:instrument".into(),
            asset_id: None,
            start_tick: riffra_core::TimelineTick(120),
            duration_ticks: 960,
            notes: vec![MidiNote {
                id: "note:ignored".into(),
                note: 60,
                start_tick: riffra_core::TimelineTick(0),
                duration_ticks: 480,
                velocity: 100,
                channel: 1,
            }],
            events: vec![
                MidiEvent {
                    id: "event:ignored".into(),
                    kind: MidiEventKind::ControlChange,
                    tick: riffra_core::TimelineTick(480),
                    channel: 1,
                    data1: 7,
                    data2: 64,
                },
                MidiEvent {
                    id: "event:pitch".into(),
                    kind: MidiEventKind::PitchBend,
                    tick: riffra_core::TimelineTick(600),
                    channel: 2,
                    data1: 1,
                    data2: 2,
                },
                MidiEvent {
                    id: "event:pressure".into(),
                    kind: MidiEventKind::ChannelPressure,
                    tick: riffra_core::TimelineTick(720),
                    channel: 3,
                    data1: 4,
                    data2: 5,
                },
            ],
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        };
        session.arrangement.midi_clips.push(midi.clone());
        session.arrangement.automation_lanes = vec![
            AutomationLane {
                id: "lane:volume".into(),
                target: AutomationTarget::TrackMix {
                    track_id: "track:audio".into(),
                    parameter: TrackMixParameter::Volume,
                },
                interpolation: riffra_core::AutomationInterpolation::Linear,
                points: vec![AutomationPoint {
                    id: "point:ignored".into(),
                    tick: riffra_core::TimelineTick(240),
                    value: -3.0,
                }],
            },
            AutomationLane {
                id: "lane:pan".into(),
                target: AutomationTarget::TrackMix {
                    track_id: "track:audio".into(),
                    parameter: TrackMixParameter::Pan,
                },
                interpolation: riffra_core::AutomationInterpolation::Linear,
                points: vec![AutomationPoint {
                    id: "point:pan".into(),
                    tick: riffra_core::TimelineTick(360),
                    value: 0.75,
                }],
            },
        ];

        let targets = [
            AutomationTarget::BusMix {
                bus_id: "bus:group".into(),
                parameter: riffra_core::BusMixParameter::Gain,
            },
            AutomationTarget::BusMix {
                bus_id: "bus:return".into(),
                parameter: riffra_core::BusMixParameter::Pan,
            },
            AutomationTarget::MasterGain,
            AutomationTarget::SendLevel {
                send_id: "send:audio".into(),
            },
            AutomationTarget::SidechainSendLevel {
                sidechain_send_id: "sidechain:audio".into(),
            },
            AutomationTarget::DeviceParameter {
                device_id: "device:master".into(),
                parameter_id: "stable-gain".into(),
            },
        ];
        for (index, target) in targets.iter().enumerate() {
            session.arrangement.automation_lanes.push(AutomationLane {
                id: format!("lane:{index}"),
                target: target.clone(),
                interpolation: riffra_core::AutomationInterpolation::Step,
                points: vec![AutomationPoint {
                    id: format!("point:{index}"),
                    tick: riffra_core::TimelineTick(240),
                    value: 0.0,
                }],
            });
        }

        let projection_resources = ResolvedResources::for_projection(
            PathBuf::from("data-root"),
            HashMap::from([(audio_asset_id, PathBuf::from("audio/clip.wav"))]),
            HashSet::from(["fx.vst3".to_owned(), "instrument.vst3".to_owned()]),
            HashMap::from([("preset".to_owned(), PathBuf::from("builtins/preset"))]),
        );
        let first = project_graph(&session, &projection_resources);
        session.arrangement.tracks[0].name = "Renamed".into();
        session.arrangement.tracks[0].color = Some("#123456".into());
        midi.name = "Renamed MIDI".into();
        midi.notes[0].id = "note:changed".into();
        session.arrangement.midi_clips[0] = midi;
        let second = project_graph(&session, &projection_resources);

        assert_eq!(first, second);
        let (graph, diagnostics) = first;
        assert_eq!(diagnostics, ProjectionDiagnostics::default());
        assert_eq!(graph.timebase.tempo_changes[0].bpm, 123.5);
        assert_eq!(graph.loop_range.start_tick, 120);
        assert_eq!(graph.loop_range.end_tick, 3_840);
        assert_eq!(
            graph.punch_range,
            Some(GraphTickRange {
                start_tick: 240,
                end_tick: 1_920,
            })
        );
        assert!(graph.metronome_enabled);
        assert_eq!(graph.metronome_sound, riffra_core::MetronomeSound::Wood);
        assert_eq!(graph.master_gain_db, -6.0);
        assert_eq!(graph.tracks[0].output_bus_id.as_deref(), Some("bus:group"));
        assert_eq!(graph.mixer.buses.len(), 2);
        assert_eq!(graph.mixer.buses[1].effects[0].id, "bus:return:fx");
        assert_eq!(graph.mixer.master_effects[0].id, "device:master");
        assert_eq!(graph.mixer.sends, session.arrangement.mixer.sends);
        assert_eq!(
            graph.mixer.sidechain_sends,
            session.arrangement.mixer.sidechain_sends
        );
        assert_eq!(graph.tracks.len(), 3);
        assert_eq!(graph.tracks[0].kind, GraphTrackKind::Audio);
        assert_eq!(graph.tracks[0].gain_db, -3.0);
        assert_eq!(graph.tracks[0].pan, 0.25);
        assert!(graph.tracks[0].muted && graph.tracks[0].solo && graph.tracks[0].armed);
        assert!(graph.tracks[0].monitor_input);
        assert_eq!(graph.tracks[0].effects.len(), 1);
        assert_eq!(graph.tracks[0].audio_clips.len(), 1);
        assert_eq!(graph.tracks[0].audio_clips[0].path, "audio/clip.wav");
        assert_eq!(graph.tracks[0].audio_clips[0].source_start_frame, 12);
        assert_eq!(graph.tracks[0].audio_clips[0].duration_sample_rate, 44_100);
        assert_eq!(
            graph.tracks[0].audio_clips[0].fade_shape,
            GraphFadeShape::Smooth
        );
        assert_eq!(
            graph.tracks[0].audio_clips[0].take_variant,
            GraphTakeVariant::Processed
        );
        assert!(graph.tracks[0].audio_clips[0].loop_enabled);
        assert!(graph.tracks[0].audio_clips[0].muted);
        assert_eq!(
            graph.tracks[0].audio_input.as_ref().unwrap().channel_index,
            3
        );
        assert_eq!(graph.automation[0].points[0].value, -3.0);
        assert_eq!(graph.automation[1].points[0].value, 0.75);
        assert_eq!(graph.automation.len(), 8);
        for (lane, target) in graph.automation[2..].iter().zip(targets) {
            assert_eq!(lane.target, target);
            assert_eq!(
                lane.interpolation,
                riffra_core::AutomationInterpolation::Step
            );
            assert_eq!(
                lane.points,
                vec![GraphAutomationPoint {
                    tick: 240,
                    value: 0.0
                }]
            );
        }
        assert_eq!(
            graph.tracks[1].midi_input.device_id.as_deref(),
            Some("midi:1")
        );
        assert_eq!(graph.tracks[1].midi_input.channel, Some(2));
        assert_eq!(graph.tracks[1].midi_clips[0].notes[0].note, 60);
        assert_eq!(graph.tracks[1].midi_clips[0].events[0].data2, 64);
        assert_eq!(
            graph.tracks[1].midi_clips[0].events[1].kind,
            GraphMidiEventKind::PitchBend
        );
        assert_eq!(
            graph.tracks[1].midi_clips[0].events[2].kind,
            GraphMidiEventKind::ChannelPressure
        );
        assert!(matches!(
            graph.tracks[1].instrument,
            Some(GraphInstrument::Vst3 { .. })
        ));
        assert!(matches!(
            graph.tracks[2].instrument,
            Some(GraphInstrument::Internal { .. })
        ));
    }

    #[test]
    fn monitors_audio_input_only_for_audio_tracks_that_request_it() {
        // Arrange
        let cases = [
            (TrackKind::Audio, MonitoringState::Off, true, false),
            (TrackKind::Audio, MonitoringState::Auto, false, false),
            (TrackKind::Audio, MonitoringState::Auto, true, true),
            (TrackKind::Audio, MonitoringState::On, false, true),
            (TrackKind::Instrument, MonitoringState::On, true, false),
        ];

        for (kind, monitoring, armed, expected) in cases {
            let track = Track {
                kind,
                monitoring,
                armed,
                ..Track::audio("track:1".into(), "Track".into())
            };

            // Act
            let monitor_input = monitors_audio_input(&track);

            // Assert
            assert_eq!(
                monitor_input, expected,
                "{kind:?} {monitoring:?} armed={armed}"
            );
        }
    }

    #[test]
    fn excludes_unresolved_and_disabled_resources_with_ordered_diagnostics() {
        #[derive(Clone, Copy)]
        enum MissingDevice {
            Plugin,
            Vst3Instrument,
            BuiltInPreset,
        }

        let cases = [
            ("plugin", MissingDevice::Plugin, "device:missing"),
            (
                "VST3 instrument",
                MissingDevice::Vst3Instrument,
                "instrument:missing-vst",
            ),
            (
                "built-in preset",
                MissingDevice::BuiltInPreset,
                "instrument:missing-preset",
            ),
        ];

        for (label, missing_device, expected_id) in cases {
            let mut session = CreativeSession::new(1);
            let mut track = Track::instrument("track:instrument".into(), "Keys".into());
            match missing_device {
                MissingDevice::Plugin => track.effects.push(plugin_device(
                    "device:missing",
                    "Missing",
                    "missing.vst3",
                    false,
                )),
                MissingDevice::Vst3Instrument => {
                    track.instrument = Some(
                        TrackInstrument::vst3(
                            "instrument:missing-vst".into(),
                            "Missing VST".into(),
                            "missing-instrument.vst3".into(),
                        )
                        .unwrap(),
                    );
                }
                MissingDevice::BuiltInPreset => {
                    track.instrument = Some(
                        TrackInstrument::built_in(
                            "instrument:missing-preset".into(),
                            "Missing preset".into(),
                            "not-resolved".into(),
                            "{}".into(),
                        )
                        .unwrap(),
                    );
                }
            }
            track.effects.push(plugin_device(
                "device:disabled",
                "Disabled",
                "absent.vst3",
                true,
            ));
            session.arrangement.tracks.push(track);
            let asset_id = riffra_core::mint_asset_id();
            session.arrangement.audio_clips.push(AudioClip::full_source(
                "clip:missing".into(),
                "Missing".into(),
                "track:instrument".into(),
                asset_id,
                riffra_core::TimelineTick(0),
                48_000,
                48_000,
            ));
            let mut disabled_vst =
                Track::instrument("track:disabled-vst".into(), "Disabled VST".into());
            disabled_vst.instrument = Some(
                TrackInstrument::vst3(
                    "instrument:disabled".into(),
                    "Disabled".into(),
                    "absent.vst3".into(),
                )
                .unwrap(),
            );
            if let Some(plugin) = disabled_vst
                .instrument
                .as_mut()
                .and_then(TrackInstrument::as_vst3_mut)
            {
                plugin.disabled_placeholder = true;
            }
            session.arrangement.tracks.push(disabled_vst);

            let (graph, diagnostics) = project_graph(&session, &resources());

            assert_eq!(graph.tracks[0].instrument, None, "{label}");
            assert!(graph.tracks[0].effects.is_empty(), "{label}");
            assert_eq!(graph.tracks[1].instrument, None, "{label}");
            assert_eq!(
                diagnostics.unavailable_clip_ids,
                ["clip:missing"],
                "{label}"
            );
            assert_eq!(diagnostics.missing_device_ids, [expected_id], "{label}");
        }
    }

    #[test]
    fn excludes_sidechain_routes_to_missing_effects_and_their_automation() {
        // Arrange
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:source".into(), "Source".into()));
        let mut target = Track::audio("track:target".into(), "Target".into());
        target.effects.push(plugin_device(
            "device:missing",
            "Missing",
            "missing.vst3",
            false,
        ));
        target.effects.push(plugin_device(
            "device:available",
            "Available",
            "fx.vst3",
            false,
        ));
        session.arrangement.tracks.push(target);
        session.arrangement.mixer.sidechain_sends = vec![
            riffra_core::SidechainSend {
                id: "sidechain:missing".into(),
                source_track_id: "track:source".into(),
                target_device_id: "device:missing".into(),
                level_db: -6.0,
                position: riffra_core::SendPosition::PostFader,
                enabled: true,
            },
            riffra_core::SidechainSend {
                id: "sidechain:available".into(),
                source_track_id: "track:source".into(),
                target_device_id: "device:available".into(),
                level_db: -6.0,
                position: riffra_core::SendPosition::PostFader,
                enabled: true,
            },
        ];
        session.arrangement.automation_lanes = ["sidechain:missing", "sidechain:available"]
            .into_iter()
            .map(|sidechain_send_id| AutomationLane {
                id: format!("lane:{sidechain_send_id}"),
                target: AutomationTarget::SidechainSendLevel {
                    sidechain_send_id: sidechain_send_id.into(),
                },
                interpolation: riffra_core::AutomationInterpolation::Linear,
                points: vec![AutomationPoint {
                    id: format!("point:{sidechain_send_id}"),
                    tick: riffra_core::TimelineTick(0),
                    value: -6.0,
                }],
            })
            .collect();

        // Act
        let (graph, diagnostics) = project_graph(&session, &resources());

        // Assert
        assert_eq!(diagnostics.missing_device_ids, ["device:missing"]);
        assert_eq!(
            graph.mixer.sidechain_sends,
            [session.arrangement.mixer.sidechain_sends[1].clone()]
        );
        assert_eq!(graph.automation.len(), 1);
        assert_eq!(
            graph.automation[0].target,
            AutomationTarget::SidechainSendLevel {
                sidechain_send_id: "sidechain:available".into()
            }
        );
    }
}
