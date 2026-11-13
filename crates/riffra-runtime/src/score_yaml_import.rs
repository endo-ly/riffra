//! Score YAML to canonical production state.
//!
//! A Score document is read once, compiled with Riffra analysis, resolved
//! against the built-in catalog and Sonalloy inspection, and only then
//! converted into canonical production state. Rebuilding a linked Project
//! reuses tracks, devices, and instrument snapshots whose sources did not
//! change.

use crate::api::output::{
    ScoreChordInspection, ScoreDiagnostic, ScoreInspection, ScoreInstrumentInspection,
    ScoreLaneInspection, ScoreSectionInspection, ScoreSummary, ScoreValidation,
};
use crate::api::params::ProjectImportScoreParams;
use crate::instrument::{BuiltInInstrumentCatalog, UserInstrumentStore};
use crate::score::{self, ScoreLane, ScorePlan, events::MusicalEvent, syntax};
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

/// A built-in source a Score instrument resolves to.
#[derive(Clone)]
struct BuiltIn {
    preset_id: String,
    definition_json: String,
}

/// A resolved Score instrument source.
#[derive(Clone)]
struct Source {
    definition: PathBuf,
    name: String,
    report: Value,
    built_in: Option<BuiltIn>,
}

/// One validated Score file with every instrument source resolved.
struct ScoreDocument {
    path: PathBuf,
    yaml: String,
    plan: ScorePlan,
    bindings: BTreeMap<String, PathBuf>,
    sources: BTreeMap<String, Source>,
}

/// A canonical Session candidate and the snapshots it staged.
pub(crate) struct ScoreOutcome {
    pub(crate) session: CreativeSession,
    /// Whether the Score body differs from the Project's stored body.
    pub(crate) yaml_changed: bool,
    /// Whether the Score-owned performance differs from the Project.
    pub(crate) changed: bool,
    /// Whether the linked file path or external instrument bindings differ.
    pub(crate) link_changed: bool,
    staged: BTreeSet<PathBuf>,
}

impl ScoreOutcome {
    /// Returns whether accepting this Score changes anything the Project stores.
    pub(crate) fn has_changes(&self) -> bool {
        self.changed || self.yaml_changed || self.link_changed
    }

    /// Returns the Session to commit, or `None` when the Project already matches.
    ///
    /// A Score that only changes its body or link keeps the current performance
    /// and arrangement revision; only a performance change adopts the rebuilt
    /// Session.
    pub(crate) fn candidate(&self, current: &CreativeSession) -> Option<CreativeSession> {
        if !self.has_changes() {
            return None;
        }
        Some(if self.changed {
            self.session.clone()
        } else {
            let mut candidate = current.clone();
            candidate.score_source = self.session.score_source.clone();
            candidate
        })
    }

    /// Keeps the staged snapshots because canonical state now references them.
    pub(crate) fn retain(&mut self) {
        self.staged.clear();
    }
}

impl Drop for ScoreOutcome {
    fn drop(&mut self) {
        for snapshot in &self.staged {
            if let Err(error) = std::fs::remove_dir_all(snapshot) {
                tracing::warn!(%error, "score instrument snapshot cleanup failed");
            }
        }
    }
}

/// Imports a Score YAML file as a new Session.
///
/// # Errors
/// Returns structured diagnostics for Score, source, and instrument failures.
pub(crate) fn import(
    params: &ProjectImportScoreParams,
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    sonalloy: &Path,
) -> Result<ScoreOutcome, ImportError> {
    let mut document = read(
        &params.path,
        &params.bindings,
        &params.bindings.keys().cloned().collect(),
        catalog,
        sonalloy,
    )?;
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = BTreeSet::new();
    let session = build(&mut document, None, &store, &mut staged)?;
    Ok(ScoreOutcome {
        session,
        yaml_changed: true,
        changed: true,
        link_changed: true,
        staged,
    })
}

/// Rebuilds a linked Project from its Score file.
///
/// `bindings` replaces or adds to the Project's stored bindings. Bindings the
/// Score no longer uses are dropped.
///
/// # Errors
/// Returns structured diagnostics for Score, source, and instrument failures.
pub(crate) fn rebuild(
    previous: &CreativeSession,
    path: Option<&Path>,
    bindings: &BTreeMap<String, PathBuf>,
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    sonalloy: &Path,
) -> Result<ScoreOutcome, ImportError> {
    let source = previous.score_source.as_ref().ok_or_else(|| {
        ImportError::from("the active Project is not linked to a Score".to_string())
    })?;
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => source.path.as_ref().map(PathBuf::from).ok_or_else(|| {
            ImportError::from(
                "the active Project has no linked Score file; rebuild it with --path".to_string(),
            )
        })?,
    };
    let mut effective: BTreeMap<String, PathBuf> = source
        .bindings
        .iter()
        .map(|(id, path)| (id.clone(), PathBuf::from(path)))
        .collect();
    effective.extend(bindings.iter().map(|(id, path)| (id.clone(), path.clone())));
    let mut document = read(
        &path,
        &effective,
        &bindings.keys().cloned().collect(),
        catalog,
        sonalloy,
    )?;
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = BTreeSet::new();
    let session = build(&mut document, Some(previous), &store, &mut staged)?;
    let yaml_changed = previous
        .score_source
        .as_ref()
        .is_none_or(|source| source.yaml != document.yaml);
    Ok(ScoreOutcome {
        changed: !previous.score_performance_equals(&session),
        link_changed: link_changed(previous, &session),
        session,
        yaml_changed,
        staged,
    })
}

fn link_changed(previous: &CreativeSession, next: &CreativeSession) -> bool {
    match (previous.score_source.as_ref(), next.score_source.as_ref()) {
        (Some(previous), Some(next)) => {
            previous.path != next.path || previous.bindings != next.bindings
        }
        _ => true,
    }
}

/// Reports whether the linked Score file no longer matches the accepted body.
pub(crate) fn source_changed(session: &CreativeSession) -> bool {
    let Some(source) = session.score_source.as_ref() else {
        return false;
    };
    let Some(path) = source.path.as_deref() else {
        return false;
    };
    std::fs::read_to_string(path).map_or(true, |text| text != source.yaml)
}

/// Validates a Score document without creating a Project or snapshot.
///
/// # Errors
/// Never returns an error; Score problems become diagnostics.
pub(crate) fn validate(
    path: &Path,
    bindings: &BTreeMap<String, PathBuf>,
    catalog: &BuiltInInstrumentCatalog,
    sonalloy: &Path,
) -> ScoreValidation {
    match read(
        path,
        bindings,
        &bindings.keys().cloned().collect(),
        catalog,
        sonalloy,
    )
    .and_then(|document| {
        validate_performance(&document.plan, &document.sources)?;
        Ok(summary(&document.plan))
    }) {
        Ok(summary) => ScoreValidation {
            valid: true,
            diagnostics: Vec::new(),
            summary: Some(summary),
        },
        Err(error) => ScoreValidation {
            valid: false,
            diagnostics: vec![diagnostic(&error)],
            summary: None,
        },
    }
}

/// Expands a Score document into its resolved structure without creating a Project.
///
/// # Errors
/// Returns structured diagnostics for Score, source, and instrument failures.
pub(crate) fn inspect(
    path: &Path,
    bindings: &BTreeMap<String, PathBuf>,
    events: bool,
    catalog: &BuiltInInstrumentCatalog,
    sonalloy: &Path,
) -> Result<ScoreInspection, ImportError> {
    let document = read(
        path,
        bindings,
        &bindings.keys().cloned().collect(),
        catalog,
        sonalloy,
    )?;
    validate_performance(&document.plan, &document.sources)?;
    let plan = &document.plan;
    Ok(ScoreInspection {
        summary: summary(plan),
        sections: plan
            .sections
            .iter()
            .map(|section| ScoreSectionInspection {
                name: section.section.clone(),
                bars: section.bars,
                tonic: section.tonic.clone(),
                scale: section.scale.clone(),
                bpm: section.bpm,
                meter: section.meter.clone(),
                start_tick: section.start_tick,
                end_tick: section.end_tick,
                chords: section
                    .chords
                    .iter()
                    .map(|chord| ScoreChordInspection {
                        tick: chord.tick,
                        chord: chord.chord.clone(),
                    })
                    .collect(),
            })
            .collect(),
        instruments: plan
            .instruments
            .keys()
            .map(|id| {
                let source = &document.sources[id];
                ScoreInstrumentInspection {
                    id: id.clone(),
                    name: source.name.clone(),
                    definition: source.definition.to_string_lossy().into_owned(),
                    built_in: source.built_in.is_some(),
                }
            })
            .collect(),
        lanes: plan
            .lanes
            .iter()
            .map(|lane| ScoreLaneInspection {
                id: lane.id.clone(),
                gain_db: lane.gain_db,
                notes: lane
                    .pattern
                    .events
                    .iter()
                    .filter(|event| matches!(event, MusicalEvent::Note { .. }))
                    .count(),
                controls: lane
                    .pattern
                    .events
                    .iter()
                    .filter(|event| !matches!(event, MusicalEvent::Note { .. }))
                    .count(),
                events: if events {
                    lane.pattern
                        .events
                        .iter()
                        .enumerate()
                        .map(|(index, event)| {
                            let origin = &lane.sources[index];
                            json!({
                                "event": event,
                                "path": origin.path,
                                "line": origin.line,
                                "column": origin.column,
                                "section": origin.section,
                                "occurrence": origin.occurrence,
                                "bar": origin.bar,
                                "step": origin.step,
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                },
            })
            .collect(),
    })
}

fn score_error(error: score::Error) -> ImportError {
    ImportError {
        message: error.to_string(),
        details: *error.detail,
    }
}

fn diagnostic(error: &ImportError) -> ScoreDiagnostic {
    ScoreDiagnostic {
        path: error.details["path"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        message: error.message.clone(),
        line: error.details["line"].as_u64().unwrap_or(0) as usize,
        column: error.details["column"].as_u64().unwrap_or(0) as usize,
        detail: Some(error.details.clone()),
    }
}

fn summary(plan: &ScorePlan) -> ScoreSummary {
    ScoreSummary {
        title: plan.title.clone(),
        ticks_per_beat: plan.ticks_per_beat,
        length_ticks: plan.length_ticks,
        section_count: plan.sections.len(),
        instrument_count: plan.instruments.len(),
        lane_count: plan.lanes.len(),
        note_count: plan
            .lanes
            .iter()
            .flat_map(|lane| &lane.pattern.events)
            .filter(|event| matches!(event, MusicalEvent::Note { .. }))
            .count(),
        control_count: plan
            .lanes
            .iter()
            .flat_map(|lane| &lane.pattern.events)
            .filter(|event| !matches!(event, MusicalEvent::Note { .. }))
            .count(),
    }
}

fn read(
    path: &Path,
    bindings: &BTreeMap<String, PathBuf>,
    explicit: &BTreeSet<String>,
    catalog: &BuiltInInstrumentCatalog,
    sonalloy: &Path,
) -> Result<ScoreDocument, ImportError> {
    let yaml = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read score: {}: {error}", path.display()))?;
    let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let plan = score::compile(&yaml).map_err(score_error)?;
    let missing: Vec<_> = plan
        .instruments
        .iter()
        .filter(|(id, spec)| spec["external"] == true && !bindings.contains_key(*id))
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
    for id in bindings.keys() {
        if explicit.contains(id)
            && plan
                .instruments
                .get(id)
                .is_none_or(|spec| spec["external"] != true)
        {
            return Err(format!("binding '{id}' does not identify an external instrument").into());
        }
    }
    let base = path.parent().expect("canonical file has a parent");
    let presets = base
        .ancestors()
        .map(|ancestor| ancestor.join("presets"))
        .find(|root| root.is_dir());
    let mut resolved = BTreeMap::<PathBuf, Source>::new();
    let mut sources = BTreeMap::new();
    let mut effective = BTreeMap::new();
    for (id, spec) in &plan.instruments {
        let field = format!("instruments.{id}");
        let definition =
            resolve_definition(spec, bindings.get(id), base, presets.as_deref(), catalog).map_err(
                |message| ImportError {
                    details: plan.source_detail(&field, &message),
                    message,
                },
            )?;
        let canonical =
            std::fs::canonicalize(&definition).map_err(|error| format!("{field}: {error}"))?;
        if spec["external"] == true {
            effective.insert(id.clone(), canonical.clone());
        }
        if !resolved.contains_key(&canonical) {
            let report = inspect_definition(sonalloy, &canonical)?;
            let built_in = spec["preset"].as_str().and_then(|preset| {
                let preset_id = preset_id(preset);
                let builtin = catalog.resolve(&preset_id).ok()?;
                let source = std::fs::read_to_string(&canonical).ok()?;
                (source == builtin.definition_json
                    && package_matches(canonical.parent()?, &builtin.base_dir))
                .then_some(BuiltIn {
                    preset_id,
                    definition_json: builtin.definition_json.clone(),
                })
            });
            let name = report["name"].as_str().unwrap_or(id).to_owned();
            resolved.insert(
                canonical.clone(),
                Source {
                    definition: canonical.clone(),
                    name,
                    report,
                    built_in,
                },
            );
        }
        sources.insert(id.clone(), resolved[&canonical].clone());
    }
    Ok(ScoreDocument {
        path,
        yaml,
        plan,
        bindings: effective,
        sources,
    })
}

/// Builds the canonical Session for one resolved Score document.
fn build(
    document: &mut ScoreDocument,
    previous: Option<&CreativeSession>,
    store: &UserInstrumentStore,
    staged: &mut BTreeSet<PathBuf>,
) -> Result<CreativeSession, ImportError> {
    validate_performance(&document.plan, &document.sources)?;
    let ScoreDocument {
        path,
        yaml,
        plan,
        bindings,
        sources,
    } = document;
    let sources = sources.clone();
    for (id, spec) in &plan.instruments {
        if plan.lanes.iter().any(|lane| lane.id == *id) {
            continue;
        }
        let mut pattern = plan.timeline.clone();
        pattern.events.clear();
        plan.lanes.push(ScoreLane {
            id: id.clone(),
            gain_db: spec["gain_db"].as_f64().unwrap_or(0.0),
            pattern,
            sources: Vec::new(),
        });
    }
    plan.lanes.sort_by(|first, second| {
        (first.id.as_str(), first.gain_db.to_bits())
            .cmp(&(second.id.as_str(), second.gain_db.to_bits()))
    });
    let previous_tracks: Vec<&Track> = previous
        .map(|session| {
            session
                .arrangement
                .tracks
                .iter()
                .filter(|track| track.score_instrument.is_some())
                .collect()
        })
        .unwrap_or_default();
    let claims = match_tracks(plan, &previous_tracks);
    let mut assignment = BTreeMap::<PathBuf, TrackInstrument>::new();
    let mut tracks = Vec::with_capacity(plan.lanes.len());
    let mut clips = Vec::with_capacity(plan.lanes.len());
    let mut automation = Vec::new();
    let mut tracks_by_part = BTreeMap::<String, Vec<String>>::new();
    for (index, lane) in plan.lanes.iter().enumerate() {
        let spec = &plan.instruments[&lane.id];
        let source = &sources[&lane.id];
        let claimed = claims.get(&index).copied();
        let (mut instrument, reused) = match claimed.and_then(|track| track.instrument.as_ref()) {
            Some(existing) if compatible(existing, source, store)? => (existing.clone(), true),
            _ => (assigned(source, store, staged, &mut assignment)?, false),
        };
        let mut track = match claimed {
            Some(previous) => previous.clone(),
            None => Track::instrument(
                format!("score:track:{}", riffra_control::new_instance_id()),
                lane.id.clone(),
            ),
        };
        if !reused {
            instrument.id = format!("score:device:{}", riffra_control::new_instance_id());
        }
        track.instrument = Some(instrument);
        track.kind = TrackKind::Instrument;
        track.gain_db = lane.gain_db;
        track.pan = spec["pan"].as_f64().unwrap_or(0.0);
        track.pan_law = PanLaw::LinearBalance;
        track.score_instrument = Some(lane.id.clone());
        tracks_by_part
            .entry(lane.id.clone())
            .or_default()
            .push(track.id.clone());
        tracks.push(track);
    }
    for (index, lane) in plan.lanes.iter().enumerate() {
        let spec = &plan.instruments[&lane.id];
        let source = &sources[&lane.id];
        let track_id = tracks[index].id.clone();
        if let Some(input) = spec["audio_input"].as_str() {
            let source_tracks = tracks_by_part
                .get(input)
                .ok_or_else(|| format!("unknown audio input part '{input}'"))?
                .clone();
            tracks[index].external_audio_source_track_ids = source_tracks;
        } else {
            tracks[index].external_audio_source_track_ids.clear();
        }
        let required_channels = source.report["external_audio"]["required_input_channels"]
            .as_u64()
            .unwrap_or(0);
        if (required_channels > 0) != !tracks[index].external_audio_source_track_ids.is_empty() {
            return Err(format!(
                "instrument '{}' audio input does not match its source routes",
                lane.id
            )
            .into());
        }
        let previous_clip = claims.get(&index).and_then(|claimed| {
            previous.and_then(|session| {
                session
                    .arrangement
                    .midi_clips
                    .iter()
                    .find(|clip| clip.track_id == claimed.id)
            })
        });
        let mut clip = previous_clip.cloned().unwrap_or_else(|| MidiClip {
            id: format!("score:clip:{}", riffra_control::new_instance_id()),
            name: lane.id.clone(),
            track_id: track_id.clone(),
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: plan.length_ticks,
            notes: Vec::new(),
            events: Vec::new(),
            instrument_control_events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });
        clip.name = lane.id.clone();
        clip.track_id = track_id.clone();
        clip.start_tick = TimelineTick(0);
        clip.duration_ticks = plan.length_ticks;
        clip.notes.clear();
        clip.events.clear();
        clip.instrument_control_events.clear();
        let events = reset_events(lane, plan, &source.report)?;
        for (order, event) in events.iter().enumerate() {
            let control = match event {
                MusicalEvent::Note {
                    tick,
                    duration_ticks,
                    note,
                    velocity,
                } => {
                    clip.notes.push(MidiNote {
                        lyric: None,
                        id: format!("{}:{order}", clip.id),
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
                } => (
                    *tick,
                    InstrumentControlEventKind::ParameterChange {
                        parameter: parameter.clone(),
                        native_value: *native_value,
                    },
                ),
                MusicalEvent::ParameterRamp {
                    tick,
                    duration_ticks,
                    parameter,
                    from_value,
                    to_value,
                } => (
                    *tick,
                    InstrumentControlEventKind::ParameterRamp {
                        parameter: parameter.clone(),
                        from_value: *from_value,
                        to_value: *to_value,
                        duration_ticks: *duration_ticks,
                    },
                ),
            };
            clip.instrument_control_events.push(InstrumentControlEvent {
                id: format!("{}:{order}", clip.id),
                tick: TimelineTick(control.0),
                source_order: u32::try_from(order)
                    .map_err(|_| "too many score events".to_string())?,
                kind: control.1,
            });
        }
        clips.push(clip);
        let previous_lane = claims.get(&index).and_then(|claimed| {
            previous.and_then(|session| {
                session.arrangement.automation_lanes.iter().find(|lane| {
                    lane.target
                        == AutomationTarget::TrackMix {
                            track_id: claimed.id.clone(),
                            parameter: TrackMixParameter::Volume,
                        }
                })
            })
        });
        automation.push(AutomationLane {
            id: previous_lane.map_or_else(
                || format!("score:volume:{}", riffra_control::new_instance_id()),
                |lane| lane.id.clone(),
            ),
            target: AutomationTarget::TrackMix {
                track_id,
                parameter: TrackMixParameter::Volume,
            },
            interpolation: AutomationInterpolation::LinearAmplitude {
                duration_seconds: 0.005,
            },
            points: plan
                .sections
                .iter()
                .enumerate()
                .map(|(section_index, section)| AutomationPoint {
                    id: format!("score:gain:{}:{section_index}", lane.id),
                    tick: TimelineTick(section.start_tick),
                    value: lane.gain_db + section.part_gains.get(&lane.id).copied().unwrap_or(0.0),
                })
                .collect(),
        });
    }
    let regions = plan
        .sections
        .iter()
        .enumerate()
        .map(|(index, section)| TimelineRegion {
            id: previous
                .and_then(|session| session.arrangement.regions.get(index))
                .filter(|region| {
                    region.name == section.section && region.start_tick.0 == section.start_tick
                })
                .map_or_else(
                    || format!("score:region:{}", riffra_control::new_instance_id()),
                    |region| region.id.clone(),
                ),
            name: section.section.clone(),
            start_tick: TimelineTick(section.start_tick),
            end_tick: TimelineTick(section.end_tick),
        })
        .collect::<Vec<_>>();
    let harmony = harmony_events(plan)?;

    let mut session = previous
        .cloned()
        .unwrap_or_else(|| CreativeSession::new(riffra_host::now_ms()));
    if session.arrangement.timebase.ppq != u32::from(plan.ticks_per_beat) {
        let timebase = ProjectTimebase {
            ppq: u32::from(plan.ticks_per_beat),
            tempo_changes: session.arrangement.timebase.tempo_changes.clone(),
            time_signature_changes: session.arrangement.timebase.time_signature_changes.clone(),
        };
        session
            .arrangement
            .update_timebase(timebase)
            .map_err(|error| error.to_string())?;
    }
    if previous.is_some() {
        session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    }
    session.project_name = Some(plan.title.clone());
    session.score_source = Some(ScoreSource {
        file_name: path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        yaml: yaml.clone(),
        path: Some(path.to_string_lossy().into_owned()),
        bindings: bindings
            .iter()
            .map(|(id, path)| (id.clone(), path.to_string_lossy().into_owned()))
            .collect(),
    });
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
    let previous_track_ids = previous_tracks
        .iter()
        .map(|track| track.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut retained_tracks = Vec::new();
    if let Some(previous) = previous {
        for track in &previous.arrangement.tracks {
            if track.score_instrument.is_none() {
                retained_tracks.push(track.clone());
            }
        }
    }
    let retained_track_ids = retained_tracks
        .iter()
        .map(|track| track.id.as_str())
        .collect::<BTreeSet<_>>();
    let (retained_clips, retained_automation) = previous.map_or_else(
        || (Vec::new(), Vec::new()),
        |previous| {
            (
                previous
                    .arrangement
                    .midi_clips
                    .iter()
                    .filter(|clip| retained_track_ids.contains(clip.track_id.as_str()))
                    .cloned()
                    .collect::<Vec<_>>(),
                previous
                    .arrangement
                    .automation_lanes
                    .iter()
                    .filter(|lane| !targets_track(&lane.target, &previous_track_ids))
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        },
    );
    tracks.extend(retained_tracks);
    clips.extend(retained_clips);
    automation.extend(retained_automation);
    session.arrangement.tracks = tracks;
    session.arrangement.midi_clips = clips;
    session.arrangement.automation_lanes = automation;
    session.arrangement.regions = regions;
    session.arrangement.harmony_events = harmony;
    session.validate_and_normalize().map_err(ImportError::from)
}

fn targets_track(target: &AutomationTarget, track_ids: &BTreeSet<&str>) -> bool {
    match target {
        AutomationTarget::TrackMix { track_id, .. } => track_ids.contains(track_id.as_str()),
        AutomationTarget::DeviceParameter { device_id, .. } => {
            track_ids.contains(device_id.as_str())
        }
        _ => false,
    }
}

/// Pairs previous Score tracks with new lanes, preferring exact lane keys.
fn match_tracks<'a>(plan: &ScorePlan, previous: &[&'a Track]) -> BTreeMap<usize, &'a Track> {
    let mut claims = BTreeMap::new();
    let mut claimed = BTreeSet::new();
    for (index, lane) in plan.lanes.iter().enumerate() {
        if let Some(track) = previous.iter().find(|track| {
            !claimed.contains(&track.id)
                && track.score_instrument.as_deref() == Some(lane.id.as_str())
                && track.gain_db.to_bits() == lane.gain_db.to_bits()
        }) {
            claimed.insert(track.id.clone());
            claims.insert(index, *track);
        }
    }
    for (index, lane) in plan.lanes.iter().enumerate() {
        if claims.contains_key(&index) {
            continue;
        }
        let remaining_lanes = plan
            .lanes
            .iter()
            .enumerate()
            .filter(|(other, lane)| !claims.contains_key(other) && lane.id == plan.lanes[index].id)
            .count();
        let remaining_tracks = previous
            .iter()
            .filter(|track| {
                !claimed.contains(&track.id)
                    && track.score_instrument.as_deref() == Some(lane.id.as_str())
            })
            .collect::<Vec<_>>();
        if remaining_lanes == 1
            && let [track] = remaining_tracks.as_slice()
        {
            claimed.insert(track.id.clone());
            claims.insert(index, *track);
        }
    }
    claims
}

/// Reports whether an existing instrument assignment still matches its source.
fn compatible(
    instrument: &TrackInstrument,
    source: &Source,
    store: &UserInstrumentStore,
) -> Result<bool, ImportError> {
    match &instrument.source {
        TrackInstrumentSource::Internal {
            definition_json,
            resource: InternalInstrumentResource::BuiltInPreset { preset_id },
        } => Ok(source.built_in.as_ref().is_some_and(|built_in| {
            built_in.preset_id == *preset_id && built_in.definition_json == *definition_json
        })),
        TrackInstrumentSource::Internal {
            resource: InternalInstrumentResource::UserSnapshot { snapshot_id, .. },
            ..
        } => Ok(source.built_in.is_none()
            && store
                .snapshot_matches(snapshot_id, &source.definition)
                .map_err(ImportError::from)?),
        TrackInstrumentSource::Vst3(_) => Ok(false),
    }
}

/// Builds the instrument assignment for one resolved source.
fn assigned(
    source: &Source,
    store: &UserInstrumentStore,
    staged: &mut BTreeSet<PathBuf>,
    assignments: &mut BTreeMap<PathBuf, TrackInstrument>,
) -> Result<TrackInstrument, ImportError> {
    if let Some(instrument) = assignments.get(&source.definition) {
        return Ok(instrument.clone());
    }
    let instrument = if let Some(built_in) = &source.built_in {
        TrackInstrument::built_in(
            "score:instrument".into(),
            source.name.clone(),
            built_in.preset_id.clone(),
            built_in.definition_json.clone(),
        )?
    } else {
        let snapshot = store.snapshot_definition(&source.definition)?;
        staged.insert(snapshot.package_root.clone());
        TrackInstrument::user_snapshot(
            "score:instrument".into(),
            source.name.clone(),
            format!("user:{}", riffra_control::new_instance_id()),
            snapshot.snapshot_id,
            snapshot.definition_json,
        )?
    };
    assignments.insert(source.definition.clone(), instrument.clone());
    Ok(instrument)
}

/// Validates the playable content of a resolved Score against its sources.
fn validate_performance(
    plan: &ScorePlan,
    sources: &BTreeMap<String, Source>,
) -> Result<(), ImportError> {
    for lane in &plan.lanes {
        let source = &sources[&lane.id];
        let spec = &plan.instruments[&lane.id];
        let required_channels = source.report["external_audio"]["required_input_channels"]
            .as_u64()
            .unwrap_or(0);
        let routed = spec["audio_input"]
            .as_str()
            .is_some_and(|input| plan.instruments.contains_key(input));
        if (required_channels > 0) != routed {
            let field = format!("instruments.{}.audio_input", lane.id);
            return Err(ImportError {
                message: format!(
                    "instrument '{}' audio input does not match its source routes",
                    lane.id
                ),
                details: plan.source_detail(&field, "audio input mismatch"),
            });
        }
        for (order, event) in lane.pattern.events.iter().enumerate() {
            let path = lane
                .sources
                .get(order)
                .map_or_else(String::new, |source| source.path.clone());
            let result = match event {
                MusicalEvent::Note { note, velocity, .. } => {
                    validate_note(&source.report, *note, *velocity)
                }
                MusicalEvent::ParameterChange {
                    parameter,
                    native_value,
                    ..
                } => validate_parameter(&source.report, parameter, &[*native_value]),
                MusicalEvent::ParameterRamp {
                    parameter,
                    from_value,
                    to_value,
                    ..
                } => validate_parameter(&source.report, parameter, &[*from_value, *to_value]),
                _ => Ok(()),
            };
            if let Err(message) = result {
                return Err(ImportError {
                    details: plan.source_detail(&path, &message),
                    message,
                });
            }
        }
    }
    Ok(())
}

fn harmony_events(plan: &ScorePlan) -> Result<Vec<HarmonyEvent>, String> {
    let note = |pitch: i32| {
        MusicalNoteName::new(
            [
                "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
            ][pitch.rem_euclid(12) as usize],
        )
        .map_err(|error| error.to_string())
    };
    let mut events = Vec::new();
    for (section_index, section) in plan.sections.iter().enumerate() {
        let tonic = syntax::tonic(&section.tonic).map_err(|error| error.to_string())?;
        for (index, change) in section.chords.iter().enumerate() {
            let Some(chord) = syntax::chord(&change.chord).map_err(|error| error.to_string())?
            else {
                continue;
            };
            let tones = syntax::voicing(&chord, tonic, 48)
                .into_iter()
                .map(note)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            events.push(HarmonyEvent {
                id: format!("score:harmony:{section_index}:{index}"),
                start_tick: TimelineTick(change.tick),
                end_tick: TimelineTick(
                    section
                        .chords
                        .get(index + 1)
                        .map_or(section.end_tick, |next| next.tick),
                ),
                chord: HarmonyChord::from_explicit(
                    tones,
                    Some(note(tonic + chord.root)?),
                    Some(note(tonic + chord.bass)?),
                    Some(change.chord.clone()),
                )
                .map_err(|error| error.to_string())?,
            });
        }
    }
    Ok(events)
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

fn inspect_definition(sonalloy: &Path, definition: &Path) -> Result<Value, String> {
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

    const SCORE: &str = r#"{
        "meta":{"title":"Rebuild","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"verse","bars":1,"tonic":"C","scale":"ionian"}],
        "sections":{"verse":{"lines":{"main":{"instrument":"lead","melody":"theme"}}}},
        "melodies":{"theme":["1 - - - | 3 - - - | 5 - - - | 1' - - -"]},
        "instruments":{"lead":{"preset":"TEST/001-lead"}}
    }"#;

    fn report() -> Value {
        serde_json::json!({
            "layers":[{"trigger":{"key_min":0,"key_max":127,"velocity_min":1,"velocity_max":127}}]
        })
    }

    fn document(yaml: &str) -> ScoreDocument {
        let plan = score::compile(yaml).expect("score compiles");
        let sources = plan
            .instruments
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    Source {
                        definition: PathBuf::from(format!("/definitions/{id}.json")),
                        name: id.clone(),
                        report: report(),
                        built_in: Some(BuiltIn {
                            preset_id: format!("TEST-{id}"),
                            definition_json: "{}".into(),
                        }),
                    },
                )
            })
            .collect();
        ScoreDocument {
            path: PathBuf::from("/scores/score.yaml"),
            yaml: yaml.into(),
            plan,
            bindings: BTreeMap::new(),
            sources,
        }
    }

    fn store(root: &Path) -> UserInstrumentStore {
        UserInstrumentStore::new(root, Path::new("sonalloy"))
    }

    fn notes(session: &CreativeSession) -> Vec<(u64, u8)> {
        session.arrangement.midi_clips[0]
            .notes
            .iter()
            .map(|note| (note.start_tick.0, note.note))
            .collect()
    }

    #[test]
    fn rebuilding_keeps_project_identity_work_state_and_device_ids() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-rebuild-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let mut first = document(SCORE);
        let mut staged = BTreeSet::new();
        let mut session = build(&mut first, None, &store, &mut staged).expect("initial build");
        let session_id = session.session_id.clone();
        let track = session.arrangement.tracks[0].clone();
        let device = track.instrument.as_ref().expect("device").id.clone();
        let clip = session.arrangement.midi_clips[0].id.clone();
        session.arrangement.loop_range.enabled = true;
        session.arrangement.loop_range.start_tick = TimelineTick(480);
        session.arrangement.loop_range.end_tick = TimelineTick(960);
        session.arrangement.markers.push(Marker {
            id: "marker:1".into(),
            name: "Intro".into(),
            tick: 0,
        });
        session.arrangement.tracks[0].muted = true;
        session.arrangement.tracks[0].solo = true;
        session.arrangement.tracks[0].color = Some("#123456".into());
        session.settings.count_in_beats = 2;

        let changed = SCORE.replace("3 - - -", "4 - - -");
        let mut second = document(&changed);
        let rebuilt = build(&mut second, Some(&session), &store, &mut staged).expect("rebuild");

        assert_eq!(rebuilt.session_id, session_id);
        assert_eq!(rebuilt.arrangement.tracks[0].id, track.id);
        assert_eq!(
            rebuilt.arrangement.tracks[0]
                .instrument
                .as_ref()
                .expect("device")
                .id,
            device
        );
        assert_eq!(rebuilt.arrangement.midi_clips[0].id, clip);
        assert_eq!(notes(&rebuilt)[1].1, 65);
        assert_ne!(notes(&session)[1].1, 65);
        assert!(rebuilt.arrangement.loop_range.enabled);
        assert_eq!(rebuilt.arrangement.loop_range.start_tick.0, 480);
        assert_eq!(rebuilt.arrangement.markers, session.arrangement.markers);
        assert!(rebuilt.arrangement.tracks[0].muted);
        assert!(rebuilt.arrangement.tracks[0].solo);
        assert_eq!(
            rebuilt.arrangement.tracks[0].color.as_deref(),
            Some("#123456")
        );
        assert_eq!(rebuilt.settings.count_in_beats, 2);
        assert_eq!(rebuilt.score_source.as_ref().unwrap().yaml, changed);
        assert!(staged.is_empty(), "built-in sources stage no snapshot");
        assert!(!session.score_performance_equals(&rebuilt));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rebuilding_only_the_score_text_keeps_the_performance_identical() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-text-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let mut first = document(SCORE);
        let mut staged = BTreeSet::new();
        let session = build(&mut first, None, &store, &mut staged).expect("initial build");

        let mut second = document(&format!(
            "{SCORE}
# Only the text changed."
        ));
        let rebuilt =
            build(&mut second, Some(&session), &store, &mut staged).expect("text-only rebuild");

        assert!(session.score_performance_equals(&rebuilt));
        assert_eq!(rebuilt.arrangement.regions, session.arrangement.regions);
        assert_eq!(
            rebuilt.arrangement.midi_clips[0].id,
            session.arrangement.midi_clips[0].id
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rebuild_pairs_lanes_by_instrument_and_gain_change() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-pair-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let yaml = SCORE.replace(
            r#""instruments":{"lead":{"preset":"TEST/001-lead"}}"#,
            r#""instruments":{"lead":{"preset":"TEST/001-lead"},"pad":{"preset":"TEST/002-pad"}}"#,
        );
        let mut first = document(&yaml);
        let mut staged = BTreeSet::new();
        let session = build(&mut first, None, &store, &mut staged).expect("initial build");
        let lead = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("lead"))
            .expect("lead track")
            .clone();
        let lead_device = lead.instrument.as_ref().expect("device").id.clone();

        // Moving the lead lane behind a new instrument keeps its identity.
        let mut inserted = document(&yaml.replace(
            r#""instruments":{"lead""#,
            r#""instruments":{"bass":{"preset":"TEST/003-bass"},"lead""#,
        ));
        let rebuilt = build(&mut inserted, Some(&session), &store, &mut staged)
            .expect("lane insertion rebuild");
        let rebuilt_lead = rebuilt
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("lead"))
            .expect("lead track");
        assert_eq!(rebuilt_lead.id, lead.id);
        assert_eq!(
            rebuilt_lead.instrument.as_ref().expect("device").id,
            lead_device
        );
        assert_eq!(rebuilt.arrangement.tracks.len(), 3);

        // Changing the lane gain reuses the single remaining lane of its instrument.
        let mut changed_gain = document(&yaml.replace(
            r#"{"instrument":"lead","melody":"theme"}"#,
            r#"{"instrument":"lead","melody":"theme","gain_db":-6}"#,
        ));
        let rebuilt =
            build(&mut changed_gain, Some(&rebuilt), &store, &mut staged).expect("gain rebuild");
        let rebuilt_lead = rebuilt
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("lead"))
            .expect("lead track");
        assert_eq!(rebuilt_lead.id, lead.id);
        assert_eq!(rebuilt_lead.gain_db, -6.0);
        assert_eq!(
            rebuilt_lead.instrument.as_ref().expect("device").id,
            lead_device
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rebuild_keeps_a_user_snapshot_whose_definition_is_unchanged() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-snapshot-{}",
            riffra_control::new_instance_id()
        ));
        let definitions = root.join("definitions");
        let snapshot_id = riffra_control::new_instance_id();
        let snapshot = root.join("project-instruments").join(&snapshot_id);
        std::fs::create_dir_all(&definitions).unwrap();
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(definitions.join("lead.json"), r#"{"name":"Lead"}"#).unwrap();
        std::fs::write(snapshot.join("definition.json"), r#"{"name":"Lead"}"#).unwrap();
        let store = store(&root);
        let mut first = document(SCORE);
        let mut staged = BTreeSet::new();
        let mut session = build(&mut first, None, &store, &mut staged).expect("initial build");
        let snapshot_name = snapshot_id.clone();
        session.arrangement.tracks[0].instrument = Some(
            TrackInstrument::user_snapshot(
                "score:instrument".into(),
                "Lead".into(),
                format!("user:{}", riffra_control::new_instance_id()),
                snapshot_id.clone(),
                r#"{"name":"Lead"}"#.into(),
            )
            .unwrap(),
        );
        let device = session.arrangement.tracks[0]
            .instrument
            .as_ref()
            .unwrap()
            .id
            .clone();

        let mut second = document(SCORE);
        second.sources.insert(
            "lead".into(),
            Source {
                definition: definitions.join("lead.json"),
                name: "Lead".into(),
                report: report(),
                built_in: None,
            },
        );
        let rebuilt = build(&mut second, Some(&session), &store, &mut staged)
            .expect("a matching snapshot is reused");

        assert!(staged.is_empty(), "no new snapshot is staged");
        assert_eq!(
            rebuilt.arrangement.tracks[0]
                .instrument
                .as_ref()
                .unwrap()
                .id,
            device
        );
        assert!(matches!(
            rebuilt.arrangement.tracks[0].instrument.as_ref().unwrap().source,
            TrackInstrumentSource::Internal {
                resource: InternalInstrumentResource::UserSnapshot { ref snapshot_id, .. },
                ..
            } if *snapshot_id == snapshot_name
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rebuild_decisions_distinguish_performance_body_and_link_changes() {
        let mut session = CreativeSession::new(1);
        session.score_source = Some(ScoreSource {
            file_name: "song.yaml".into(),
            yaml: "meta: {}".into(),
            path: Some("/scores/song.yaml".into()),
            bindings: BTreeMap::from([("lead".into(), "/definitions/lead.json".into())]),
        });

        // A rebuild that finds no change stores nothing.
        let unchanged = ScoreOutcome {
            session: session.clone(),
            yaml_changed: false,
            changed: false,
            link_changed: false,
            staged: BTreeSet::new(),
        };
        assert!(unchanged.candidate(&session).is_none());

        // Relinking to an identical body keeps the performance and revision.
        let mut relinked = session.clone();
        relinked.score_source.as_mut().unwrap().path = Some("/scores/moved.yaml".into());
        assert!(link_changed(&session, &relinked));
        let outcome = ScoreOutcome {
            session: relinked,
            yaml_changed: false,
            changed: false,
            link_changed: true,
            staged: BTreeSet::new(),
        };
        let candidate = outcome.candidate(&session).expect("a link change commits");
        assert_eq!(candidate.arrangement.revision, session.arrangement.revision);
        assert_eq!(
            candidate.score_source.as_ref().unwrap().path.as_deref(),
            Some("/scores/moved.yaml")
        );
        assert!(candidate.score_performance_equals(&session));
    }
}
