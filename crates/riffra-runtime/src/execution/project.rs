use super::*;
use riffra_core::{
    AudioTakeVariant, AutomationParameter, CreativeSession, FadeShape, InternalInstrumentResource,
    MidiEventKind, MonitoringState, TrackInstrumentSource, TrackKind,
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
                        TrackInstrumentSource::Vst3 {
                            path,
                            parameter_values,
                            state_data,
                            disabled_placeholder,
                        } => {
                            if *disabled_placeholder {
                                None
                            } else if resources.existing_plugin_paths.contains(path) {
                                Some(GraphInstrument::Vst3 {
                                    id: instrument.id.clone(),
                                    path: path.clone(),
                                    state: GraphPluginState {
                                        state_data: state_data.clone(),
                                        parameter_values: parameter_values.clone(),
                                        bypassed: instrument.bypassed,
                                    },
                                })
                            } else {
                                diagnostics.missing_device_ids.push(instrument.id.clone());
                                None
                            }
                        }
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
                .rack
                .devices
                .iter()
                .filter(|device| device.kind == riffra_core::DeviceKind::Plugin)
                .filter_map(|device| {
                    if device.disabled_placeholder {
                        return None;
                    }
                    let Some(path) = device.path.as_ref() else {
                        diagnostics.missing_device_ids.push(device.id.clone());
                        return None;
                    };
                    if !resources.existing_plugin_paths.contains(path) {
                        diagnostics.missing_device_ids.push(device.id.clone());
                        return None;
                    }
                    Some(GraphPluginDevice {
                        id: device.id.clone(),
                        path: path.clone(),
                        state: GraphPluginState {
                            state_data: device.state_data.clone(),
                            parameter_values: device.parameter_values.clone(),
                            bypassed: device.bypassed,
                        },
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
                monitoring: match track.monitoring {
                    MonitoringState::Off => GraphMonitoring::Off,
                    MonitoringState::Auto => GraphMonitoring::Auto,
                    MonitoringState::On => GraphMonitoring::On,
                },
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

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{
        AudioClip, AudioInputRoute, AutomationLane, AutomationPoint, MidiClip, MidiEvent,
        MidiInputRoute, MidiNote, RackDevice, Track, TrackInstrument,
    };
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn plugin_device(id: &str, name: &str, path: Option<&str>, disabled: bool) -> RackDevice {
        RackDevice {
            id: id.into(),
            name: name.into(),
            kind: riffra_core::DeviceKind::Plugin,
            path: path.map(str::to_owned),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: disabled,
        }
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
        let mut audio = Track::audio("track:audio".into(), "Audio".into());
        audio.audio_input = Some(AudioInputRoute { channel_index: 3 });
        audio
            .rack
            .devices
            .push(plugin_device("device:fx", "FX", Some("fx.vst3"), false));
        let mut instrument = Track::instrument("track:instrument".into(), "Keys".into());
        instrument.midi_input = MidiInputRoute {
            device_id: Some("midi:1".into()),
            channel: Some(2),
        };
        instrument.instrument = Some(
            TrackInstrument::vst3(
                "instrument:vst3".into(),
                "VST".into(),
                "instrument.vst3".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks = vec![audio, instrument];
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
            events: vec![MidiEvent {
                id: "event:ignored".into(),
                kind: MidiEventKind::ControlChange,
                tick: riffra_core::TimelineTick(480),
                channel: 1,
                data1: 7,
                data2: 64,
            }],
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        };
        session.arrangement.midi_clips.push(midi.clone());
        session.arrangement.automation_lanes.push(AutomationLane {
            id: "lane:volume".into(),
            track_id: "track:audio".into(),
            parameter: AutomationParameter::Volume,
            points: vec![AutomationPoint {
                id: "point:ignored".into(),
                tick: riffra_core::TimelineTick(240),
                value: -3.0,
            }],
        });

        let first = project_graph(&session, &resources());
        session.arrangement.tracks[0].name = "Renamed".into();
        session.arrangement.tracks[0].color = Some("#123456".into());
        midi.name = "Renamed MIDI".into();
        midi.notes[0].id = "note:changed".into();
        session.arrangement.midi_clips[0] = midi;
        let second = project_graph(&session, &resources());

        assert_eq!(first, second);
        assert_eq!(first.0.tracks[0].effects.len(), 1);
        assert_eq!(
            first.0.tracks[0]
                .audio_input
                .as_ref()
                .unwrap()
                .channel_index,
            3
        );
        assert_eq!(first.0.tracks[0].volume_automation[0].value, -3.0);
        assert_eq!(first.0.tracks[1].midi_clips[0].notes[0].note, 60);
        assert_eq!(first.0.tracks[1].midi_clips[0].events[0].data2, 64);
    }

    #[test]
    fn excludes_unresolved_and_disabled_resources_with_ordered_diagnostics() {
        let mut session = CreativeSession::new(1);
        let mut instrument = Track::instrument("track:instrument".into(), "Keys".into());
        instrument.instrument = Some(
            TrackInstrument::built_in(
                "instrument:missing".into(),
                "Missing".into(),
                "not-resolved".into(),
                "{}".into(),
            )
            .unwrap(),
        );
        instrument.rack.devices = vec![
            plugin_device("device:missing", "Missing", Some("missing.vst3"), false),
            plugin_device("device:disabled", "Disabled", Some("absent.vst3"), true),
        ];
        instrument.rack.devices[1].disabled_placeholder = true;
        session.arrangement.tracks.push(instrument);
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
        if let Some(TrackInstrumentSource::Vst3 {
            disabled_placeholder,
            ..
        }) = disabled_vst
            .instrument
            .as_mut()
            .map(|instrument| &mut instrument.source)
        {
            *disabled_placeholder = true;
        }
        session.arrangement.tracks.push(disabled_vst);

        let (graph, diagnostics) = project_graph(&session, &resources());

        assert_eq!(graph.tracks[0].instrument, None);
        assert!(graph.tracks[0].effects.is_empty());
        assert_eq!(graph.tracks[1].instrument, None);
        assert_eq!(diagnostics.unavailable_clip_ids, ["clip:missing"]);
        assert_eq!(
            diagnostics.missing_device_ids,
            ["instrument:missing", "device:missing"]
        );
    }
}
