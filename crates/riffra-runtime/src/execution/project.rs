use super::*;
use riffra_core::{
    AudioTakeVariant, AutomationParameter, CreativeSession, FadeShape, InternalInstrumentResource,
    MidiEventKind, MonitoringState, Track, TrackInstrumentSource, TrackKind, Vst3Plugin,
};

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
                        }),
                    });

            let effects = track
                .effects
                .iter()
                .filter_map(|device| {
                    plugin_state(
                        &device.id,
                        &device.plugin,
                        device.bypassed,
                        resources,
                        &mut diagnostics,
                    )
                    .map(|state| GraphPluginDevice {
                        id: device.id.clone(),
                        path: device.plugin.path.clone(),
                        state,
                    })
                })
                .collect();

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

            let mut volume_automation = Vec::new();
            let mut pan_automation = Vec::new();
            for lane in arrangement
                .automation_lanes
                .iter()
                .filter(|lane| lane.track_id == track.id)
            {
                let target = match lane.parameter {
                    AutomationParameter::Volume => &mut volume_automation,
                    AutomationParameter::Pan => &mut pan_automation,
                };
                target.extend(lane.points.iter().map(|point| GraphAutomationPoint {
                    tick: point.tick.0,
                    value: point.value,
                }));
            }

            GraphTrack {
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
                volume_automation,
                pan_automation,
                effects,
                instrument,
                audio_clips,
                midi_clips,
            }
        })
        .collect();

    (
        ExecutionGraph {
            timebase: GraphTimebase {
                ppq: arrangement.timebase.ppq,
                bpm: arrangement.timebase.bpm,
                time_signature_numerator: arrangement.timebase.time_signature_numerator,
                time_signature_denominator: arrangement.timebase.time_signature_denominator,
            },
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
            master_gain_db: session.settings.master_db,
            tracks,
        },
        diagnostics,
    )
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
        AudioClip, AudioInputRoute, AutomationLane, AutomationPoint, EffectDevice, MidiClip,
        MidiEvent, MidiInputRoute, MidiNote, Track, TrackInstrument,
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
            bpm: 123.5,
            time_signature_numerator: 7,
            time_signature_denominator: 8,
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
                track_id: "track:audio".into(),
                parameter: AutomationParameter::Volume,
                points: vec![AutomationPoint {
                    id: "point:ignored".into(),
                    tick: riffra_core::TimelineTick(240),
                    value: -3.0,
                }],
            },
            AutomationLane {
                id: "lane:pan".into(),
                track_id: "track:audio".into(),
                parameter: AutomationParameter::Pan,
                points: vec![AutomationPoint {
                    id: "point:pan".into(),
                    tick: riffra_core::TimelineTick(360),
                    value: 0.75,
                }],
            },
        ];

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
        assert_eq!(graph.timebase.bpm, 123.5);
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
        assert_eq!(graph.master_gain_db, -6.0);
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
        assert_eq!(graph.tracks[0].volume_automation[0].value, -3.0);
        assert_eq!(graph.tracks[0].pan_automation[0].value, 0.75);
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
}
