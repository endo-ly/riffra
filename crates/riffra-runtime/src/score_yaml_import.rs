//! Lossless projection of expanded Score performance into editable production state.

use crate::api::params::ProjectImportScoreParams;
use crate::instrument::{BuiltInInstrumentCatalog, UserInstrumentStore};
use crate::score::{self, ScoreLane, ScorePlan, events::MusicalEvent};
use riffra_core::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct ImportError {
    pub(crate) message: String,
    pub(crate) details: Value,
}

impl From<String> for ImportError {
    fn from(message: String) -> Self {
        Self {
            message,
            details: Value::Null,
        }
    }
}

/// Owns staged snapshots until project activation succeeds.
pub(crate) struct ImportedScore {
    pub(crate) session: CreativeSession,
    snapshots: Vec<PathBuf>,
}

impl ImportedScore {
    pub(crate) fn retain(mut self) {
        self.snapshots.clear();
    }
}

impl Drop for ImportedScore {
    fn drop(&mut self) {
        for snapshot in &self.snapshots {
            if let Err(error) = std::fs::remove_dir_all(snapshot) {
                tracing::warn!(%error, "score instrument snapshot cleanup failed");
            }
        }
    }
}

struct Instrument {
    assignment: TrackInstrument,
    report: Value,
}

pub(crate) fn import(
    params: &ProjectImportScoreParams,
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    sonalloy: &Path,
) -> Result<ImportedScore, ImportError> {
    if !params
        .path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".score.yaml"))
    {
        return Err("expected a .score.yaml file".to_string().into());
    }
    let mut plan = score::read(&params.path).map_err(|error| match error {
        score::ReadError::Parse(error) => ImportError {
            message: error.to_string(),
            details: *error.detail,
        },
        error => error.to_string().into(),
    })?;
    let missing: Vec<_> = plan
        .instruments
        .iter()
        .filter(|(id, spec)| spec["external"] == true && !params.bindings.contains_key(*id))
        .map(|(id, _)| id.clone())
        .collect();
    if !missing.is_empty() {
        return Err(ImportError {
            message: format!(
                "external instrument definitions required: {}",
                missing.join(", ")
            ),
            details: json!({"kind":"scoreBindingsRequired", "instrumentIds":missing}),
        });
    }
    for id in params.bindings.keys() {
        if plan
            .instruments
            .get(id)
            .is_none_or(|spec| spec["external"] != true)
        {
            return Err(format!("binding '{id}' does not identify an external instrument").into());
        }
    }
    let path = std::fs::canonicalize(&params.path).map_err(|error| error.to_string())?;
    let base = path.parent().expect("canonical file has a parent");
    let presets = base
        .ancestors()
        .map(|ancestor| ancestor.join("presets"))
        .find(|root| root.is_dir());
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut imported = ImportedScore {
        session: CreativeSession::new(riffra_host::now_ms()),
        snapshots: Vec::new(),
    };
    let mut instruments = BTreeMap::new();
    let mut resolved = BTreeMap::<PathBuf, Instrument>::new();
    for (id, spec) in &plan.instruments {
        let field = format!("instruments.{id}");
        let result = resolve_definition(
            spec,
            params.bindings.get(id),
            base,
            presets.as_deref(),
            catalog,
        );
        let definition = result.map_err(|message| ImportError {
            details: plan.source_detail(&field, &message),
            message,
        })?;
        let canonical =
            std::fs::canonicalize(&definition).map_err(|error| format!("{field}: {error}"))?;
        if !resolved.contains_key(&canonical) {
            let report = inspect(sonalloy, &canonical)?;
            let built_in = spec["preset"].as_str().and_then(|preset| {
                let id = preset_id(preset);
                let builtin = catalog.resolve(&id).ok()?;
                let source = std::fs::read_to_string(&canonical).ok()?;
                (source == builtin.definition_json
                    && package_matches(canonical.parent()?, &builtin.base_dir))
                .then_some((id, builtin))
            });
            let assignment = if let Some((preset_id, builtin)) = built_in {
                TrackInstrument::built_in(
                    "score:instrument".into(),
                    builtin.summary.name.clone(),
                    preset_id,
                    builtin.definition_json.clone(),
                )?
            } else {
                let snapshot = store.snapshot_definition(&definition)?;
                imported.snapshots.push(snapshot.package_root.clone());
                TrackInstrument::user_snapshot(
                    "score:instrument".into(),
                    report["name"].as_str().unwrap_or(id).into(),
                    format!("user:{}", riffra_control::new_instance_id()),
                    snapshot.snapshot_id,
                    snapshot.definition_json,
                )?
            };
            resolved.insert(canonical.clone(), Instrument { assignment, report });
        }
        instruments.insert(id.clone(), canonical);
    }
    let source = ScoreSource {
        file_name: path
            .file_name()
            .expect("score filename")
            .to_string_lossy()
            .into_owned(),
        yaml: plan.source_yaml().to_owned(),
    };
    imported.session = build_session(&mut plan, source, &instruments, &resolved)?;
    Ok(imported)
}

fn build_session(
    plan: &mut ScorePlan,
    source: ScoreSource,
    instruments: &BTreeMap<String, PathBuf>,
    resolved: &BTreeMap<PathBuf, Instrument>,
) -> Result<CreativeSession, String> {
    // Input-only and continuous instruments still need their own execution instance.
    for (id, spec) in &plan.instruments {
        if !plan.lanes.iter().any(|lane| lane.id == *id) {
            let mut pattern = plan.timeline.clone();
            pattern.events.clear();
            plan.lanes.push(ScoreLane {
                id: id.clone(),
                gain_db: spec["gain_db"].as_f64().unwrap_or(0.0),
                pattern,
                sources: Vec::new(),
            });
        }
    }
    let mut session = CreativeSession::new(riffra_host::now_ms());
    session.project_name = Some(plan.title.clone());
    session.score_source = Some(source);
    session.arrangement.timebase = ProjectTimebase {
        ppq: u32::from(plan.ticks_per_beat),
        tempo_changes: plan
            .timeline
            .tempo_changes
            .iter()
            .map(|change| TempoChange {
                tick: change.tick,
                bpm: change.bpm,
            })
            .collect(),
        time_signature_changes: plan
            .timeline
            .time_signature_changes
            .iter()
            .map(|change| TimeSignatureChange {
                tick: change.tick,
                numerator: change.numerator,
                denominator: change.denominator,
            })
            .collect(),
    };
    session.settings.mixdown.musical_end_tick = plan.length_ticks;
    session.settings.mixdown.fade_out_seconds = plan.fade_out_seconds;
    session.settings.mixdown.mastering = plan.master.as_ref().map(|master| LoudnessMastering {
        integrated_lufs: master["target_lufs"].as_f64().expect("validated loudness"),
        true_peak_db: master["max_true_peak_db"]
            .as_f64()
            .expect("validated true peak"),
        loudness_range_lu: master["target_lra_lu"].as_f64(),
    });
    let mut tracks_by_part = BTreeMap::<String, Vec<String>>::new();
    for (index, lane) in plan.lanes.iter().enumerate() {
        tracks_by_part
            .entry(lane.id.clone())
            .or_default()
            .push(format!("score:track:{index}"));
    }
    for (index, lane) in plan.lanes.iter().enumerate() {
        let spec = &plan.instruments[&lane.id];
        let instrument = &resolved[&instruments[&lane.id]];
        let mut track = Track::instrument(format!("score:track:{index}"), lane.id.clone());
        let mut assignment = instrument.assignment.clone();
        assignment.id = format!("score:device:{index}");
        track.instrument = Some(assignment);
        track.gain_db = lane.gain_db;
        track.pan = spec["pan"].as_f64().unwrap_or(0.0);
        track.pan_law = PanLaw::LinearBalance;
        if let Some(input) = spec["audio_input"].as_str() {
            track.external_audio_source_track_ids = tracks_by_part
                .get(input)
                .ok_or_else(|| format!("unknown audio input part '{input}'"))?
                .clone();
        }
        let required_channels = instrument.report["external_audio"]["required_input_channels"]
            .as_u64()
            .unwrap_or(0);
        if (required_channels > 0) != !track.external_audio_source_track_ids.is_empty()
            || (required_channels > 0 && required_channels != 2)
        {
            return Err(format!(
                "instrument '{}' audio input does not match its stereo route",
                lane.id
            ));
        }
        let events = reset_events(lane, plan, &instrument.report)?;
        let mut clip = MidiClip {
            id: format!("score:clip:{index}"),
            name: lane.id.clone(),
            track_id: track.id.clone(),
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: plan.length_ticks,
            notes: Vec::new(),
            events: Vec::new(),
            instrument_control_events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        };
        for (order, event) in events.iter().enumerate() {
            let control = match event {
                MusicalEvent::Note {
                    tick,
                    duration_ticks,
                    note,
                    velocity,
                } => {
                    validate_note(&instrument.report, *note, *velocity)?;
                    clip.notes.push(MidiNote {
                        id: format!("score:note:{index}:{order}"),
                        note: *note,
                        start_tick: TimelineTick(*tick),
                        duration_ticks: *duration_ticks,
                        velocity: *velocity,
                        channel: 1,
                    });
                    continue;
                }
                MusicalEvent::SustainPedal { tick, down } => (
                    *tick,
                    InstrumentControlEventKind::SustainPedal { down: *down },
                ),
                MusicalEvent::PitchBend { tick, value } => (
                    *tick,
                    InstrumentControlEventKind::PitchBend { value: *value },
                ),
                MusicalEvent::ModWheel { tick, value } => (
                    *tick,
                    InstrumentControlEventKind::ModWheel { value: *value },
                ),
                MusicalEvent::Aftertouch { tick, value } => (
                    *tick,
                    InstrumentControlEventKind::Aftertouch { value: *value },
                ),
                MusicalEvent::ParameterChange {
                    tick,
                    parameter,
                    native_value,
                } => {
                    validate_parameter(&instrument.report, parameter, &[*native_value])?;
                    (
                        *tick,
                        InstrumentControlEventKind::ParameterChange {
                            parameter: parameter.clone(),
                            native_value: *native_value,
                        },
                    )
                }
                MusicalEvent::ParameterRamp {
                    tick,
                    duration_ticks,
                    parameter,
                    from_value,
                    to_value,
                } => {
                    validate_parameter(&instrument.report, parameter, &[*from_value, *to_value])?;
                    (
                        *tick,
                        InstrumentControlEventKind::ParameterRamp {
                            parameter: parameter.clone(),
                            from_value: *from_value,
                            to_value: *to_value,
                            duration_ticks: *duration_ticks,
                        },
                    )
                }
            };
            clip.instrument_control_events.push(InstrumentControlEvent {
                id: format!("score:control:{index}:{order}"),
                tick: TimelineTick(control.0),
                source_order: u32::try_from(order)
                    .map_err(|_| "too many score events".to_string())?,
                kind: control.1,
            });
        }
        let points: Vec<_> = plan
            .sections
            .iter()
            .enumerate()
            .map(|(section_index, section)| AutomationPoint {
                id: format!("score:gain:{index}:{section_index}"),
                tick: TimelineTick(section.start_tick),
                value: lane.gain_db + section.part_gains.get(&lane.id).copied().unwrap_or(0.0),
            })
            .collect();
        session.arrangement.automation_lanes.push(AutomationLane {
            id: format!("score:volume:{index}"),
            target: AutomationTarget::TrackMix {
                track_id: track.id.clone(),
                parameter: TrackMixParameter::Volume,
            },
            interpolation: AutomationInterpolation::LinearAmplitude {
                duration_seconds: 0.005,
            },
            points,
        });
        session.arrangement.tracks.push(track);
        session.arrangement.midi_clips.push(clip);
    }
    for (index, section) in plan.sections.iter().enumerate() {
        session.arrangement.regions.push(TimelineRegion {
            id: format!("score:region:{index}"),
            name: section.section.clone(),
            start_tick: TimelineTick(section.start_tick),
            end_tick: TimelineTick(section.end_tick),
        });
    }
    session.arrangement.harmony_events = plan.harmony_events()?;
    session.validate_and_normalize()
}

fn resolve_definition(
    spec: &Value,
    binding: Option<&PathBuf>,
    base: &Path,
    presets: Option<&Path>,
    catalog: &BuiltInInstrumentCatalog,
) -> Result<PathBuf, String> {
    if let Some(definition) = spec["definition"].as_str() {
        return Ok(base.join(definition));
    }
    if spec["external"] == true {
        return binding
            .cloned()
            .ok_or_else(|| "external instrument requires a binding".into());
    }
    let preset = spec["preset"]
        .as_str()
        .ok_or("instrument requires a preset or definition")?;
    if preset
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':']))
    {
        return Err("unsafe preset reference".into());
    }
    if let Some(root) = presets {
        let definition = root.join(preset).join("definition.json");
        if definition.is_file() {
            return Ok(definition);
        }
    }
    let id = preset_id(preset);
    let builtin = catalog.resolve(&id)?;
    if preset.contains('/') && !builtin.base_dir.ends_with(Path::new(preset)) {
        return Err(format!(
            "preset '{preset}' does not match the bundled definition"
        ));
    }
    Ok(builtin.base_dir.join("definition.json"))
}

fn preset_id(preset: &str) -> String {
    preset
        .split_once('/')
        .and_then(|(category, name)| {
            name.split_once('-')
                .map(|(number, _)| format!("{category}-{number}"))
        })
        .unwrap_or_else(|| preset.to_owned())
}

fn package_matches(source: &Path, builtin: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(source) else {
        return false;
    };
    entries.into_iter().all(|entry| {
        let Ok(entry) = entry else {
            return false;
        };
        let Ok(kind) = entry.file_type() else {
            return false;
        };
        let target = builtin.join(entry.file_name());
        if kind.is_dir() {
            package_matches(&entry.path(), &target)
        } else if kind.is_file() {
            std::fs::read(entry.path())
                .ok()
                .zip(std::fs::read(target).ok())
                .is_some_and(|(a, b)| a == b)
        } else {
            false
        }
    })
}

fn inspect(sonalloy: &Path, definition: &Path) -> Result<Value, String> {
    let output = crate::process::sidecar_command(sonalloy)
        .args(["instrument", "inspect"])
        .arg(definition)
        .arg("--json")
        .output()
        .map_err(|error| format!("instrument inspection could not start: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "instrument inspection failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid instrument inspection: {error}"))
}

fn validate_note(report: &Value, note: u8, velocity: u8) -> Result<(), String> {
    let triggered = report["layers"].as_array().is_some_and(|layers| {
        layers.iter().any(|layer| {
            let trigger = &layer["trigger"];
            trigger["key_min"]
                .as_u64()
                .is_some_and(|min| u64::from(note) >= min)
                && trigger["key_max"]
                    .as_u64()
                    .is_some_and(|max| u64::from(note) <= max)
                && trigger["velocity_min"]
                    .as_u64()
                    .is_some_and(|min| u64::from(velocity) >= min)
                && trigger["velocity_max"]
                    .as_u64()
                    .is_some_and(|max| u64::from(velocity) <= max)
        })
    });
    if triggered {
        Ok(())
    } else {
        Err(format!(
            "note {note} velocity {velocity} cannot trigger an instrument layer"
        ))
    }
}

fn parameter<'a>(report: &'a Value, id: &str) -> Result<&'a Value, String> {
    report["parameters"]
        .as_array()
        .and_then(|parameters| parameters.iter().find(|parameter| parameter["id"] == id))
        .ok_or_else(|| format!("unknown instrument parameter '{id}'"))
}

fn validate_parameter(report: &Value, id: &str, values: &[f32]) -> Result<(), String> {
    let descriptor = parameter(report, id)?;
    let min = descriptor["min"]
        .as_f64()
        .ok_or("parameter has no minimum")?;
    let max = descriptor["max"]
        .as_f64()
        .ok_or("parameter has no maximum")?;
    if values.iter().any(|value| {
        !value.is_finite()
            || (descriptor["scale"] != "linear_unbounded"
                && !(min..=max).contains(&f64::from(*value)))
    }) {
        return Err(format!("instrument parameter '{id}' is out of range"));
    }
    Ok(())
}

fn reset_events(
    lane: &ScoreLane,
    plan: &ScorePlan,
    report: &Value,
) -> Result<Vec<MusicalEvent>, String> {
    let mut ranges = BTreeMap::<(u64, u64), (u64, u64)>::new();
    for window in plan
        .part_windows
        .iter()
        .filter(|window| window.instrument == lane.id)
    {
        ranges
            .entry((window.section_start_tick, window.section_end_tick))
            .and_modify(|(start, end)| {
                *start = (*start).max(window.start_tick);
                *end = (*end).min(window.end_tick);
            })
            .or_insert((window.start_tick, window.end_tick));
    }
    let mut ticks = BTreeSet::new();
    for ((section_start, section_end), (start, end)) in ranges {
        ticks.extend(if start >= end {
            [section_start, section_end]
        } else {
            [start, end]
        });
    }
    let mut controls = BTreeMap::<String, MusicalEvent>::new();
    for event in &lane.pattern.events {
        let (key, reset) = match event {
            MusicalEvent::Note { .. } => continue,
            MusicalEvent::SustainPedal { .. } => (
                "sustain".into(),
                MusicalEvent::SustainPedal {
                    tick: 0,
                    down: false,
                },
            ),
            MusicalEvent::PitchBend { .. } => (
                "pitch".into(),
                MusicalEvent::PitchBend {
                    tick: 0,
                    value: 0.0,
                },
            ),
            MusicalEvent::ModWheel { .. } => (
                "mod".into(),
                MusicalEvent::ModWheel {
                    tick: 0,
                    value: 0.0,
                },
            ),
            MusicalEvent::Aftertouch { .. } => (
                "aftertouch".into(),
                MusicalEvent::Aftertouch {
                    tick: 0,
                    value: 0.0,
                },
            ),
            MusicalEvent::ParameterChange { parameter: id, .. }
            | MusicalEvent::ParameterRamp { parameter: id, .. } => {
                let default = parameter(report, id)?["default"]
                    .as_f64()
                    .ok_or("parameter has no default")? as f32;
                (
                    format!("parameter:{id}"),
                    MusicalEvent::ParameterChange {
                        tick: 0,
                        parameter: id.clone(),
                        native_value: default,
                    },
                )
            }
        };
        controls.insert(key, reset);
    }
    let mut events = Vec::new();
    for tick in ticks {
        for reset in controls.values() {
            let mut reset = reset.clone();
            match &mut reset {
                MusicalEvent::SustainPedal { tick: at, .. }
                | MusicalEvent::PitchBend { tick: at, .. }
                | MusicalEvent::ModWheel { tick: at, .. }
                | MusicalEvent::Aftertouch { tick: at, .. }
                | MusicalEvent::ParameterChange { tick: at, .. } => *at = tick,
                _ => unreachable!("reset controls contain no notes or ramps"),
            }
            events.push(reset);
        }
    }
    events.extend(lane.pattern.events.iter().cloned());
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_exact_time_controls_independent_lanes_and_reference_metadata() {
        let yaml = json!({
            "meta":{"title":"Exact performance","bpm":120,"meter":"4/4","tuning":"12edo","master":{"target_lufs":-14,"max_true_peak_db":-1}},
            "form":[{"section":"a","bars":1,"tonic":"C"},{"section":"a","bars":1,"tonic":"D"}],
            "sections":{"a":{"chords":["1:m7"],"part_gains":{"lead":-6},"placements":[{"instrument":"lead","degree":"1","at":{"bar":1,"offset_quarters":"1/7"},"duration_quarters":"1/3"}]}},
            "instruments":{"lead":{"external":true,"gain_db":30,"pan":0.5},"effect":{"external":true,"audio_input":"lead"}}
        }).to_string();
        let mut plan = score::compile(&yaml).unwrap();
        plan.lanes[0].pattern.events.extend([
            MusicalEvent::ParameterRamp {
                tick: 0,
                duration_ticks: 3360,
                parameter: "cutoff".into(),
                from_value: 20.0,
                to_value: 1000.0,
            },
            MusicalEvent::ParameterChange {
                tick: 3360,
                parameter: "cutoff".into(),
                native_value: 200.0,
            },
            MusicalEvent::SustainPedal {
                tick: 0,
                down: true,
            },
            MusicalEvent::PitchBend {
                tick: 1,
                value: 0.12345,
            },
            MusicalEvent::ModWheel {
                tick: 2,
                value: 0.23456,
            },
            MusicalEvent::Aftertouch {
                tick: 3,
                value: 0.34567,
            },
        ]);
        plan.part_windows.push(score::PartWindow {
            instrument: "lead".into(),
            section_start_tick: 0,
            section_end_tick: 13440,
            start_tick: 0,
            end_tick: 13440,
        });
        let mut second = plan.lanes[0].clone();
        second.gain_db = -3.0;
        plan.lanes.push(second);
        let assignment = TrackInstrument::built_in(
            "device".into(),
            "Test".into(),
            "TEST-001".into(),
            "{}".into(),
        )
        .unwrap();
        let report = json!({"parameters":[{"id":"cutoff","min":20,"max":20000,"default":12000,"scale":"log2"}],
            "layers":[{"trigger":{"key_min":0,"key_max":127,"velocity_min":1,"velocity_max":127}}]});
        let instruments = BTreeMap::from([
            ("lead".into(), PathBuf::from("lead")),
            ("effect".into(), PathBuf::from("effect")),
        ]);
        let resolved = BTreeMap::from([
            (
                PathBuf::from("lead"),
                Instrument {
                    assignment: assignment.clone(),
                    report: report.clone(),
                },
            ),
            (
                PathBuf::from("effect"),
                Instrument {
                    assignment,
                    report: json!({"external_audio":{"required_input_channels":2}}),
                },
            ),
        ]);

        let session = build_session(
            &mut plan,
            ScoreSource {
                file_name: "exact.score.yaml".into(),
                yaml: yaml.clone(),
            },
            &instruments,
            &resolved,
        )
        .unwrap();

        assert_eq!(session.project_name.as_deref(), Some("Exact performance"));
        let arrangement = &session.arrangement;
        assert_eq!(arrangement.timebase.ppq, 3360);
        assert_eq!(arrangement.tracks.len(), 3);
        assert_eq!(arrangement.tracks[0].gain_db, 30.0);
        assert_eq!(arrangement.tracks[0].pan_law, PanLaw::LinearBalance);
        assert_eq!(
            arrangement.tracks[2].external_audio_source_track_ids,
            ["score:track:0", "score:track:1"]
        );
        assert_ne!(
            arrangement.tracks[0].instrument.as_ref().unwrap().id,
            arrangement.tracks[1].instrument.as_ref().unwrap().id
        );
        assert_eq!(
            arrangement.midi_clips[0].notes[0].start_tick,
            TimelineTick(480)
        );
        assert_eq!(arrangement.midi_clips[0].notes[0].duration_ticks, 1120);
        let controls = &arrangement.midi_clips[0].instrument_control_events;
        assert!(controls.iter().any(|event| matches!(
            event.kind,
            InstrumentControlEventKind::ParameterRamp {
                duration_ticks: 3360,
                from_value: 20.0,
                to_value: 1000.0,
                ..
            }
        )));
        assert!(controls.iter().any(|event| event.tick.0 == 13440
            && matches!(
                event.kind,
                InstrumentControlEventKind::ParameterChange {
                    native_value: 12000.0,
                    ..
                }
            )));
        assert_eq!(arrangement.automation_lanes[0].points[0].value, 24.0);
        assert_eq!(arrangement.regions.len(), 2);
        assert_ne!(arrangement.regions[0].id, arrangement.regions[1].id);
        assert_eq!(
            arrangement.harmony_events[1].chord.root,
            Some(MusicalNoteName::new("D").unwrap())
        );
        assert_eq!(
            session
                .settings
                .mixdown
                .mastering
                .as_ref()
                .unwrap()
                .loudness_range_lu,
            None
        );
        assert_eq!(session.score_source.as_ref().unwrap().yaml, yaml);
        assert_eq!(
            serde_json::from_str::<CreativeSession>(&serde_json::to_string(&session).unwrap())
                .unwrap(),
            session
        );
        assert!(validate_parameter(&report, "cutoff", &[20001.0]).is_err());
    }
}
