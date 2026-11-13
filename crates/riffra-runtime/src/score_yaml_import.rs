//! Score YAML to canonical production state.
//!
//! A Score document is read once, compiled with Riffra analysis, resolved
//! against the library, the VST3 plugin catalog, and Sonalloy inspection, and
//! only then converted into canonical production state. Rebuilding a linked
//! Project reuses tracks, devices, and instrument snapshots whose sources did
//! not change.

mod library;
mod parameters;

use crate::api::output::{
    ScoreAudioClipInspection, ScoreBusInspection, ScoreChordInspection, ScoreDiagnostic,
    ScoreDiagnosticSeverity, ScoreInspection, ScoreInstrumentInspection, ScoreInstrumentKind,
    ScoreLaneInspection, ScoreSectionInspection, ScoreSendInspection, ScoreSidechainInspection,
    ScoreSummary, ScoreValidation,
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

/// A production source or effect resolved from the library and plugin catalog.
enum ResolvedProduction {
    Source {
        plugin: library::ResolvedPlugin,
        velocity: Option<Vec<(u8, u8)>>,
    },
    Effect {
        plugin: library::ResolvedPlugin,
        bypassed: bool,
    },
}

impl ResolvedProduction {
    fn plugin(&self) -> &library::ResolvedPlugin {
        match self {
            Self::Source { plugin, .. } | Self::Effect { plugin, .. } => plugin,
        }
    }

    fn velocity(&self) -> Option<&[(u8, u8)]> {
        match self {
            Self::Source { velocity, .. } => velocity.as_deref(),
            Self::Effect { .. } => None,
        }
    }
}

/// One validated Score file with every instrument source resolved.
struct ScoreDocument {
    path: PathBuf,
    yaml: String,
    plan: ScorePlan,
    bindings: BTreeMap<String, PathBuf>,
    sources: BTreeMap<String, Source>,
    production: BTreeMap<String, ResolvedProduction>,
}

fn is_production_instrument(plan: &ScorePlan, id: &str) -> bool {
    plan.production_sources.contains_key(id)
        || plan.kit_members.contains_key(id)
        || plan.audio_instruments.contains(id)
}

fn validate_audio(plan: &ScorePlan, base: &Path) -> Result<(), ImportError> {
    use num_traits::ToPrimitive;
    for (section, clips) in &plan.audio_clips {
        for (index, clip) in clips.iter().enumerate() {
            let field = format!("sections.{section}.audio_clips[{index}]");
            let fail = |message: String| ImportError {
                details: plan.source_detail(&field, &message),
                message,
            };
            let path = base.join(&clip.file);
            let (rate, total) = riffra_host::read_wav_metadata(&path)
                .map_err(|error| fail(format!("{}: {error}", path.display())))?;
            let frames = |seconds: f64| (seconds * f64::from(rate)).round().to_u64();
            let start = frames(clip.offset_seconds)
                .ok_or_else(|| fail("audio offset exceeds supported frame range".to_owned()))?;
            let duration = match clip.duration_seconds {
                Some(seconds) => frames(seconds).ok_or_else(|| {
                    fail("audio duration exceeds supported frame range".to_owned())
                })?,
                None => total.saturating_sub(start),
            };
            if duration == 0 || start.checked_add(duration).is_none_or(|end| end > total) {
                return Err(fail(
                    "audio clip range is outside the source file".to_owned(),
                ));
            }
            if [clip.fade_in_seconds, clip.fade_out_seconds]
                .into_iter()
                .any(|seconds| frames(seconds).is_none_or(|fade| fade > duration))
            {
                return Err(fail("audio clip fade exceeds its duration".to_owned()));
            }
        }
    }
    Ok(())
}

fn resolve_production(
    plan: &ScorePlan,
    base: &Path,
    data_root: &Path,
    scanner: &Path,
) -> Result<BTreeMap<String, ResolvedProduction>, ImportError> {
    let library = library::Library::new(base);
    let mut plugins = BTreeMap::new();
    let mut cache = parameters::ProbeCache::default();
    let mut insert_source =
        |field: String, source: &score::production::SourceSpec| -> Result<(), ImportError> {
            let mut resolve = || -> Result<ResolvedProduction, String> {
                let resolved = library.instrument(source)?;
                let vst3 = resolved
                    .source
                    .vst3
                    .as_ref()
                    .ok_or_else(|| "source requires a VST3 plugin".to_owned())?;
                let mut plugin = library::resolve_plugin(
                    data_root,
                    vst3,
                    crate::api::output::PluginRole::Instrument,
                    scanner,
                    &mut cache,
                )?;
                plugin.range = resolved.range;
                plugin.notes = resolved.notes;
                Ok(ResolvedProduction::Source {
                    plugin,
                    velocity: resolved.source.velocity,
                })
            };
            let result = resolve().map_err(|message| ImportError {
                details: plan.source_detail(&field, &message),
                message,
            })?;
            plugins.insert(field, result);
            Ok(())
        };
    for (id, source) in &plan.production_sources {
        insert_source(format!("instruments.{id}.source"), source)?;
    }
    for (id, kit) in &plan.kits {
        insert_source(format!("kits.{id}.source"), &kit.source)?;
    }
    let mut insert_effects =
        |field: String, effects: &[score::production::EffectSpec]| -> Result<(), ImportError> {
            for (index, effect) in effects.iter().enumerate() {
                let path = format!("{field}[{index}]");
                let mut resolve = || -> Result<ResolvedProduction, String> {
                    let effect = library.effect(effect)?;
                    let vst3 = effect
                        .vst3
                        .as_ref()
                        .ok_or_else(|| "effect requires a VST3 plugin".to_owned())?;
                    let plugin = library::resolve_plugin(
                        data_root,
                        vst3,
                        crate::api::output::PluginRole::Effect,
                        scanner,
                        &mut cache,
                    )?;
                    Ok(ResolvedProduction::Effect {
                        plugin,
                        bypassed: effect.bypassed.unwrap_or(false),
                    })
                };
                let result = resolve().map_err(|message| ImportError {
                    details: plan.source_detail(&path, &message),
                    message,
                })?;
                plugins.insert(path, result);
            }
            Ok(())
        };
    for (id, effects) in &plan.production_effects {
        insert_effects(format!("instruments.{id}.effects"), effects)?;
    }
    for (id, kit) in &plan.kits {
        insert_effects(format!("kits.{id}.effects"), &kit.effects)?;
    }
    if let Some(mix) = &plan.mix {
        for (id, bus) in &mix.buses {
            insert_effects(format!("mix.buses.{id}.effects"), &bus.effects)?;
        }
        insert_effects("mix.master.effects".to_owned(), &mix.master_effects)?;
    }
    Ok(plugins)
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
    staged: StagedResources,
}

impl ScoreOutcome {
    /// Returns whether accepting this Score changes anything the Project stores.
    fn has_changes(&self) -> bool {
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

    /// Keeps the staged resources because canonical state now references them.
    pub(crate) fn retain(&mut self) {
        self.staged.clear();
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
    scanner: &Path,
    sonalloy: &Path,
) -> Result<ScoreOutcome, ImportError> {
    let mut document = read(
        &params.path,
        &params.bindings,
        &params.bindings.keys().cloned().collect(),
        data_root,
        catalog,
        scanner,
        sonalloy,
    )?;
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = StagedResources::new(data_root);
    let session = build(&mut document, None, &store, &mut staged, BuildMode::Stage)?;
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
    scanner: &Path,
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
        data_root,
        catalog,
        scanner,
        sonalloy,
    )?;
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = StagedResources::new(data_root);
    let session = build(
        &mut document,
        Some(previous),
        &store,
        &mut staged,
        BuildMode::Stage,
    )?;
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
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    scanner: &Path,
    sonalloy: &Path,
) -> ScoreValidation {
    let mut document = match read(
        path,
        bindings,
        &bindings.keys().cloned().collect(),
        data_root,
        catalog,
        scanner,
        sonalloy,
    ) {
        Ok(document) => document,
        Err(error) => {
            return ScoreValidation {
                valid: false,
                diagnostics: vec![diagnostic(&error)],
                summary: None,
            };
        }
    };
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = StagedResources::new(data_root);
    match build(
        &mut document,
        None,
        &store,
        &mut staged,
        BuildMode::Validate,
    ) {
        Ok(_) => ScoreValidation {
            valid: true,
            diagnostics: warnings(&document.plan),
            summary: Some(summary(&document.plan)),
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
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    scanner: &Path,
    sonalloy: &Path,
) -> Result<ScoreInspection, ImportError> {
    let mut document = read(
        path,
        bindings,
        &bindings.keys().cloned().collect(),
        data_root,
        catalog,
        scanner,
        sonalloy,
    )?;
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = StagedResources::new(data_root);
    let _session = build(
        &mut document,
        None,
        &store,
        &mut staged,
        BuildMode::Validate,
    )?;
    validate_performance(&document.plan, &document.sources)?;
    let plan = &document.plan;
    use score::production::mix::{BusKind, Owner, SendPosition};
    let position = |value| {
        match value {
            SendPosition::Pre => "pre",
            SendPosition::Post => "post",
        }
        .to_owned()
    };
    let mut buses = Vec::new();
    let mut sends = Vec::new();
    let mut sidechains = Vec::new();
    if let Some(mix) = &plan.mix {
        for (id, bus) in &mix.buses {
            buses.push(ScoreBusInspection {
                id: id.clone(),
                kind: match bus.kind {
                    BusKind::Group => "group",
                    BusKind::Return => "return",
                }
                .into(),
                gain_db: bus.gain_db,
                pan: bus.pan,
            });
        }
        for (from, track) in &mix.tracks {
            for (bus, send) in &track.sends {
                sends.push(ScoreSendInspection {
                    from: from.clone(),
                    bus: bus.clone(),
                    level_db: send.level_db,
                    position: position(send.position),
                    enabled: send.enabled,
                });
            }
        }
        for route in &mix.sidechain {
            let (kind, id) = match &route.to.owner {
                Owner::Instrument(id) => ("instrument", Some(id.clone())),
                Owner::Kit(id) => ("kit", Some(id.clone())),
                Owner::Bus(id) => ("bus", Some(id.clone())),
                Owner::Master => ("master", None),
            };
            sidechains.push(ScoreSidechainInspection {
                from: route.from.clone(),
                owner_kind: kind.into(),
                owner_id: id,
                effect: route.to.id.clone(),
                level_db: route.send.level_db,
                position: position(route.send.position),
                enabled: route.send.enabled,
            });
        }
    }
    let audio_clips = plan
        .audio_clips
        .iter()
        .flat_map(|(section, clips)| {
            clips.iter().map(move |clip| ScoreAudioClipInspection {
                id: clip.id.clone(),
                section: section.clone(),
                instrument: clip.instrument.clone(),
                file: clip.file.clone(),
                start_ticks: plan
                    .sections
                    .iter()
                    .filter(|item| item.section == *section)
                    .map(|item| plan.position_tick(item, &clip.at))
                    .collect(),
                offset_seconds: clip.offset_seconds,
                duration_seconds: clip.duration_seconds,
                gain_db: clip.gain_db,
                pan: clip.pan,
                fade_in_seconds: clip.fade_in_seconds,
                fade_out_seconds: clip.fade_out_seconds,
            })
        })
        .collect();
    Ok(ScoreInspection {
        summary: summary(plan),
        diagnostics: warnings(plan),
        buses,
        sends,
        sidechains,
        audio_clips,
        master_gain_db: plan.mix.as_ref().map(|mix| mix.master_gain_db),
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
                if let Some(source) = document.sources.get(id) {
                    ScoreInstrumentInspection {
                        id: id.clone(),
                        name: source.name.clone(),
                        kind: ScoreInstrumentKind::Builtin,
                        definition: Some(source.definition.to_string_lossy().into_owned()),
                        built_in: source.built_in.is_some(),
                        plugin_name: None,
                        plugin_identifier: None,
                        plugin_path: None,
                        kit: None,
                        library_range: None,
                        library_notes: None,
                    }
                } else {
                    let path = plan.kit_members.get(id).map_or_else(
                        || format!("instruments.{id}.source"),
                        |kit| format!("kits.{kit}.source"),
                    );
                    let plugin = document
                        .production
                        .get(&path)
                        .map(ResolvedProduction::plugin);
                    ScoreInstrumentInspection {
                        id: id.clone(),
                        name: plugin.map_or_else(|| id.clone(), |plugin| plugin.entry.name.clone()),
                        kind: if plan.kit_members.contains_key(id) {
                            ScoreInstrumentKind::KitMember
                        } else if plan.audio_instruments.contains(id) {
                            ScoreInstrumentKind::Audio
                        } else {
                            ScoreInstrumentKind::Vst3
                        },
                        definition: None,
                        built_in: false,
                        plugin_name: plugin.map(|plugin| plugin.entry.name.clone()),
                        plugin_identifier: plugin
                            .and_then(|plugin| plugin.entry.identifier.clone()),
                        plugin_path: plugin.map(|plugin| plugin.plugin.path.clone()),
                        kit: plan.kit_members.get(id).cloned(),
                        library_range: plugin.and_then(|plugin| plugin.range),
                        library_notes: plugin.and_then(|plugin| plugin.notes.clone()),
                    }
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
        severity: ScoreDiagnosticSeverity::Error,
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

fn warnings(plan: &ScorePlan) -> Vec<ScoreDiagnostic> {
    plan.production_sources
        .keys()
        .filter(|id| plan.lanes.iter().filter(|lane| &lane.id == *id).count() > 1)
        .map(|id| {
            let path = format!("instruments.{id}.source");
            let message = "performance gain lanes require separate VST3 instances".to_owned();
            let detail = plan.source_detail(&path, &message);
            ScoreDiagnostic {
                severity: ScoreDiagnosticSeverity::Warning,
                path,
                line: detail["line"].as_u64().unwrap_or(0) as usize,
                column: detail["column"].as_u64().unwrap_or(0) as usize,
                message,
                detail: Some(detail),
            }
        })
        .collect()
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
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    scanner: &Path,
    sonalloy: &Path,
) -> Result<ScoreDocument, ImportError> {
    let yaml = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read score: {}: {error}", path.display()))?;
    let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let plan = score::compile(&yaml).map_err(score_error)?;
    let base = path.parent().expect("canonical file has a parent");
    validate_audio(&plan, base)?;
    let production = resolve_production(&plan, base, data_root, scanner)?;
    if plan
        .mix
        .as_ref()
        .is_some_and(|mix| !mix.automation.is_empty())
    {
        let message = "Score mix automation is not implemented".to_owned();
        return Err(ImportError {
            details: plan.source_detail("mix.automation", &message),
            message,
        });
    }
    let missing: Vec<_> = plan
        .instruments
        .iter()
        .filter(|(id, spec)| {
            spec["external"] == true
                && !bindings.contains_key(*id)
                && !is_production_instrument(&plan, id)
        })
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
    let presets = base
        .ancestors()
        .map(|ancestor| ancestor.join("presets"))
        .find(|root| root.is_dir());
    let mut resolved = BTreeMap::<PathBuf, Source>::new();
    let mut sources = BTreeMap::new();
    let mut effective = BTreeMap::new();
    for (id, spec) in &plan.instruments {
        if is_production_instrument(&plan, id) {
            continue;
        }
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
        production,
    })
}

/// How a Score build treats resources that outlive the candidate Session.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildMode {
    /// Stage snapshots and assets for a candidate that may be accepted.
    Stage,
    /// Resolve the same structure without writing persistent resources.
    Validate,
}

/// Snapshots staged for a candidate and discarded unless it is accepted.
struct StagedResources {
    snapshots: BTreeSet<PathBuf>,
}

impl StagedResources {
    fn new(_data_root: &Path) -> Self {
        Self {
            snapshots: BTreeSet::new(),
        }
    }

    fn clear(&mut self) {
        self.snapshots.clear();
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }
}

impl Drop for StagedResources {
    fn drop(&mut self) {
        for snapshot in &self.snapshots {
            if let Err(error) = std::fs::remove_dir_all(snapshot) {
                tracing::warn!(%error, "score instrument snapshot cleanup failed");
            }
        }
    }
}

/// Builds the canonical Session for one resolved Score document.
fn build(
    document: &mut ScoreDocument,
    previous: Option<&CreativeSession>,
    store: &UserInstrumentStore,
    staged: &mut StagedResources,
    mode: BuildMode,
) -> Result<CreativeSession, ImportError> {
    validate_performance(&document.plan, &document.sources)?;
    if !document.plan.audio_clips.is_empty() {
        return Err("Score audio clip import is not implemented"
            .to_owned()
            .into());
    }
    if document
        .plan
        .mix
        .as_ref()
        .is_some_and(|mix| !mix.automation.is_empty())
    {
        return Err("Score mix automation is not implemented".to_owned().into());
    }
    let ScoreDocument {
        path,
        yaml,
        plan,
        bindings,
        sources,
        production,
    } = document;
    let sources = sources.clone();
    for (id, spec) in &plan.instruments {
        if plan.audio_instruments.contains(id) || plan.kit_members.contains_key(id) {
            continue;
        }
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
    let mut claimed_tracks = claims
        .values()
        .map(|track| track.id.clone())
        .collect::<BTreeSet<_>>();
    let mut assignment = BTreeMap::<PathBuf, TrackInstrument>::new();
    let mut tracks = Vec::with_capacity(
        plan.lanes.len() + plan.kits.len() + plan.audio_instruments.len() + previous_tracks.len(),
    );
    let mut clips = Vec::with_capacity(plan.lanes.len() + plan.kits.len());
    let mut automation = Vec::new();
    let mut instrument_tracks = BTreeMap::<String, Vec<String>>::new();
    let mut kit_tracks = BTreeMap::<String, String>::new();
    for (index, lane) in plan.lanes.iter().enumerate() {
        if plan.kit_members.contains_key(&lane.id) || plan.audio_instruments.contains(&lane.id) {
            continue;
        }
        let spec = &plan.instruments[&lane.id];
        let claimed = claims.get(&index).copied();
        let resolved = production.get(&format!("instruments.{}.source", lane.id));
        let velocity = resolved.and_then(ResolvedProduction::velocity);
        let mut instrument = if let Some(ResolvedProduction::Source { plugin, .. }) = resolved {
            let (instrument, _) = vst3_instrument(plugin, claimed)?;
            instrument
        } else {
            let source = sources
                .get(&lane.id)
                .ok_or_else(|| format!("instrument '{}' has no resolved source", lane.id))?;
            let (mut instrument, reused) = match claimed.and_then(|track| track.instrument.as_ref())
            {
                Some(existing) if compatible(existing, source, store)? => (existing.clone(), true),
                _ => (
                    assigned(source, store, staged, mode, &mut assignment)?,
                    false,
                ),
            };
            if !reused {
                instrument.id = format!("score:device:{}", riffra_control::new_instance_id());
            }
            instrument
        };
        instrument.name = if let Some(ResolvedProduction::Source { plugin, .. }) = resolved {
            plugin.entry.name.clone()
        } else {
            sources[&lane.id].name.clone()
        };
        let mut track = match claimed {
            Some(previous) => previous.clone(),
            None => Track::instrument(
                format!("score:track:{}", riffra_control::new_instance_id()),
                lane.id.clone(),
            ),
        };
        track.instrument = Some(instrument);
        track.kind = TrackKind::Instrument;
        track.gain_db = lane.gain_db;
        track.pan = spec["pan"].as_f64().unwrap_or(0.0);
        track.pan_law = PanLaw::LinearBalance;
        track.effects.clear();
        track.score_instrument = Some(lane.id.clone());
        if let Some(input) = spec["audio_input"].as_str() {
            let source_tracks = instrument_tracks
                .get(input)
                .ok_or_else(|| format!("unknown audio input part '{input}'"))?
                .clone();
            track.external_audio_source_track_ids = source_tracks;
        } else {
            track.external_audio_source_track_ids.clear();
        }
        if resolved.is_none() {
            let source = &sources[&lane.id];
            let required_channels = source.report["external_audio"]["required_input_channels"]
                .as_u64()
                .unwrap_or(0);
            if (required_channels > 0) != !track.external_audio_source_track_ids.is_empty() {
                return Err(format!(
                    "instrument '{}' audio input does not match its source routes",
                    lane.id
                )
                .into());
            }
        }
        let previous_clip = claimed.and_then(|claimed| {
            previous.and_then(|session| {
                session
                    .arrangement
                    .midi_clips
                    .iter()
                    .find(|clip| clip.track_id == claimed.id)
            })
        });
        let events = if resolved.is_some() {
            lane.pattern.events.clone()
        } else {
            reset_events(lane, plan, &sources[&lane.id].report)?
        };
        let clip = midi_clip(
            previous_clip,
            &lane.id,
            &track.id,
            plan.length_ticks,
            &events,
            velocity,
        )?;
        instrument_tracks
            .entry(lane.id.clone())
            .or_default()
            .push(track.id.clone());
        tracks.push(track);
        clips.push(clip);
        automation.push(auto_volume_lane(
            plan,
            previous,
            claimed,
            lane,
            &clips.last().expect("clip was inserted").track_id,
        ));
    }
    for (kit_id, kit) in &plan.kits {
        let score_key = format!("kit:{kit_id}");
        let claimed = claim_track(&previous_tracks, &mut claimed_tracks, |track| {
            track.kind == TrackKind::Instrument
                && track.score_instrument.as_deref() == Some(score_key.as_str())
        });
        let key = format!("kits.{kit_id}.source");
        let Some(ResolvedProduction::Source { plugin, velocity }) = production.get(&key) else {
            let message = format!("kit {kit_id} has no resolved VST3 source");
            return Err(ImportError {
                details: plan.source_detail(&key, &message),
                message,
            });
        };
        let (mut instrument, _) = vst3_instrument(plugin, claimed)?;
        instrument.name = plugin.entry.name.clone();
        let mut track = match claimed {
            Some(previous) => previous.clone(),
            None => Track::instrument(
                format!("score:track:{}", riffra_control::new_instance_id()),
                kit_id.clone(),
            ),
        };
        track.name = kit_id.clone();
        track.instrument = Some(instrument);
        track.kind = TrackKind::Instrument;
        let track_mix = plan.mix.as_ref().and_then(|mix| mix.tracks.get(kit_id));
        track.gain_db = track_mix.and_then(|track| track.gain_db).unwrap_or(0.0);
        track.pan = track_mix.and_then(|track| track.pan).unwrap_or(0.0);
        track.pan_law = PanLaw::LinearBalance;
        track.effects.clear();
        track.external_audio_source_track_ids.clear();
        track.score_instrument = Some(score_key.clone());
        let mut events = Vec::new();
        for lane in plan
            .lanes
            .iter()
            .filter(|lane| plan.kit_members.get(&lane.id) == Some(kit_id))
        {
            events.extend(
                lane.pattern
                    .events
                    .iter()
                    .filter(|event| matches!(event, MusicalEvent::Note { .. }))
                    .cloned(),
            );
        }
        events.sort_by_key(|event| match event {
            MusicalEvent::Note { tick, .. } => *tick,
            _ => 0,
        });
        let previous_clip = claimed.and_then(|claimed| {
            previous.and_then(|session| {
                session
                    .arrangement
                    .midi_clips
                    .iter()
                    .find(|clip| clip.track_id == claimed.id)
            })
        });
        let clip = midi_clip(
            previous_clip,
            kit_id,
            &track.id,
            plan.length_ticks,
            &events,
            velocity.as_deref(),
        )?;
        kit_tracks.insert(kit_id.clone(), track.id.clone());
        tracks.push(track);
        clips.push(clip);
        let _ = kit;
    }
    for id in &plan.audio_instruments {
        let spec = &plan.instruments[id];
        let claimed = claim_track(&previous_tracks, &mut claimed_tracks, |track| {
            track.kind == TrackKind::Audio && track.score_instrument.as_deref() == Some(id.as_str())
        });
        let mut track = claimed.map_or_else(
            || {
                Track::audio(
                    format!("score:track:{}", riffra_control::new_instance_id()),
                    id.clone(),
                )
            },
            |track| track.clone(),
        );
        track.name = id.clone();
        track.kind = TrackKind::Audio;
        track.instrument = None;
        track.gain_db = spec["gain_db"].as_f64().unwrap_or(0.0);
        track.pan = spec["pan"].as_f64().unwrap_or(0.0);
        track.pan_law = PanLaw::LinearBalance;
        track.effects.clear();
        track.external_audio_source_track_ids.clear();
        track.score_instrument = Some(id.clone());
        instrument_tracks
            .entry(id.clone())
            .or_default()
            .push(track.id.clone());
        tracks.push(track);
    }
    attach_track_effects(
        plan,
        production,
        &mut tracks,
        &instrument_tracks,
        &kit_tracks,
    )?;
    let mixer = build_mixer(
        plan,
        production,
        previous,
        &mut tracks,
        &instrument_tracks,
        &kit_tracks,
    )?;

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
    session.settings.master_db = plan.mix.as_ref().map_or(0.0, |mix| mix.master_gain_db);
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
    let (retained_clips, retained_audio_clips) = previous.map_or_else(
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
                    .audio_clips
                    .iter()
                    .filter(|clip| retained_track_ids.contains(clip.track_id.as_str()))
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        },
    );
    tracks.extend(retained_tracks);
    clips.extend(retained_clips);
    if let Some(previous) = previous {
        let track_ids = tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect::<BTreeSet<_>>();
        let bus_ids = mixer
            .buses
            .iter()
            .map(|bus| bus.id.as_str())
            .collect::<BTreeSet<_>>();
        let send_ids = mixer
            .sends
            .iter()
            .map(|send| send.id.as_str())
            .collect::<BTreeSet<_>>();
        let sidechain_ids = mixer
            .sidechain_sends
            .iter()
            .map(|send| send.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut device_ids = tracks
            .iter()
            .flat_map(|track| track.effects.iter().map(|effect| effect.id.as_str()))
            .collect::<BTreeSet<_>>();
        device_ids.extend(
            tracks
                .iter()
                .filter_map(|track| track.instrument.as_ref())
                .map(|instrument| instrument.id.as_str()),
        );
        device_ids.extend(
            mixer
                .buses
                .iter()
                .flat_map(|bus| bus.effects.iter().map(|effect| effect.id.as_str())),
        );
        device_ids.extend(mixer.master_effects.iter().map(|effect| effect.id.as_str()));
        automation.extend(
            previous
                .arrangement
                .automation_lanes
                .iter()
                .filter(|lane| {
                    let score_owned = targets_track(&lane.target, &previous_track_ids);
                    !score_owned
                        && match &lane.target {
                            AutomationTarget::TrackMix { track_id, .. } => {
                                track_ids.contains(track_id.as_str())
                            }
                            AutomationTarget::BusMix { bus_id, .. } => {
                                !bus_id.starts_with("score:bus:")
                                    && bus_ids.contains(bus_id.as_str())
                            }
                            AutomationTarget::MasterGain => false,
                            AutomationTarget::SendLevel { send_id } => {
                                !send_id.starts_with("score:send:")
                                    && send_ids.contains(send_id.as_str())
                            }
                            AutomationTarget::SidechainSendLevel { sidechain_send_id } => {
                                !sidechain_send_id.starts_with("score:sidechain:")
                                    && sidechain_ids.contains(sidechain_send_id.as_str())
                            }
                            AutomationTarget::DeviceParameter { device_id, .. } => {
                                !device_id.starts_with("score:")
                                    && device_ids.contains(device_id.as_str())
                            }
                        }
                })
                .cloned(),
        );
    }
    session.arrangement.tracks = tracks;
    session.arrangement.midi_clips = clips;
    session.arrangement.audio_clips = retained_audio_clips;
    session.arrangement.automation_lanes = automation;
    session.arrangement.regions = regions;
    session.arrangement.harmony_events = harmony;
    session.arrangement.mixer = mixer;
    session.validate_and_normalize().map_err(ImportError::from)
}

fn targets_track(target: &AutomationTarget, track_ids: &BTreeSet<&str>) -> bool {
    match target {
        AutomationTarget::TrackMix { track_id, .. } => track_ids.contains(track_id.as_str()),
        AutomationTarget::DeviceParameter { device_id, .. } => device_id.starts_with("score:"),
        AutomationTarget::BusMix { bus_id, .. } => bus_id.starts_with("score:bus:"),
        AutomationTarget::SendLevel { send_id } => send_id.starts_with("score:send:"),
        AutomationTarget::SidechainSendLevel { sidechain_send_id } => {
            sidechain_send_id.starts_with("score:sidechain:")
        }
        AutomationTarget::MasterGain => true,
    }
}

/// Claims one previous Score track matching a semantic key.
fn claim_track<'a>(
    previous: &[&'a Track],
    claimed: &mut BTreeSet<String>,
    predicate: impl Fn(&Track) -> bool,
) -> Option<&'a Track> {
    previous
        .iter()
        .copied()
        .find(|track| !claimed.contains(&track.id) && predicate(track))
        .inspect(|track| {
            claimed.insert(track.id.clone());
        })
}

/// Builds a VST3 instrument, keeping the previous device when the plugin matches.
fn vst3_instrument(
    plugin: &library::ResolvedPlugin,
    previous: Option<&Track>,
) -> Result<(TrackInstrument, bool), ImportError> {
    let reused = previous
        .and_then(|track| track.instrument.as_ref())
        .and_then(TrackInstrument::as_vst3)
        .is_some_and(|existing| existing.path == plugin.plugin.path);
    let mut instrument = if reused {
        previous
            .and_then(|track| track.instrument.clone())
            .expect("a reused instrument exists")
    } else {
        TrackInstrument::vst3(
            format!("score:device:{}", riffra_control::new_instance_id()),
            plugin.entry.name.clone(),
            plugin.plugin.path.clone(),
        )?
    };
    instrument.name = plugin.entry.name.clone();
    match &mut instrument.source {
        TrackInstrumentSource::Vst3(state) => *state = plugin.plugin.clone(),
        TrackInstrumentSource::Internal { .. } => {
            unreachable!("VST3 instrument source was created or reused")
        }
    }
    Ok((instrument, reused))
}

/// Creates one full-length MIDI clip from expanded Score events.
fn midi_clip(
    previous: Option<&MidiClip>,
    name: &str,
    track_id: &str,
    duration_ticks: u64,
    events: &[MusicalEvent],
    velocity_map: Option<&[(u8, u8)]>,
) -> Result<MidiClip, ImportError> {
    let mut clip = previous.cloned().unwrap_or_else(|| MidiClip {
        id: format!("score:clip:{}", riffra_control::new_instance_id()),
        name: name.to_owned(),
        track_id: track_id.to_owned(),
        asset_id: None,
        start_tick: TimelineTick(0),
        duration_ticks,
        notes: Vec::new(),
        events: Vec::new(),
        instrument_control_events: Vec::new(),
        muted: false,
        loop_enabled: false,
        recording_take_id: None,
    });
    clip.name = name.to_owned();
    clip.track_id = track_id.to_owned();
    clip.start_tick = TimelineTick(0);
    clip.duration_ticks = duration_ticks;
    clip.notes.clear();
    clip.events.clear();
    clip.instrument_control_events.clear();
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
                    velocity: mapped_velocity(velocity_map, *velocity),
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
            source_order: u32::try_from(order).map_err(|_| "too many score events".to_string())?,
            kind: control.1,
        });
    }
    Ok(clip)
}

/// Applies a monotonic Score velocity map once to one expanded note.
fn mapped_velocity(map: Option<&[(u8, u8)]>, velocity: u8) -> u8 {
    let Some(map) = map else {
        return velocity;
    };
    let value = f64::from(velocity);
    for pair in map.windows(2) {
        let [(input_a, output_a), (input_b, output_b)] = [pair[0], pair[1]];
        if velocity <= input_b {
            if input_b == input_a {
                return output_b.clamp(1, 127);
            }
            let t = (value - f64::from(input_a)) / f64::from(input_b - input_a);
            let mapped = f64::from(output_a) + t * f64::from(output_b - output_a);
            return mapped.round().clamp(1.0, 127.0) as u8;
        }
    }
    map.last()
        .map_or(velocity, |(_, output)| (*output).clamp(1, 127))
}

/// Builds the volume lane generated from the Score's section gains.
fn auto_volume_lane(
    plan: &ScorePlan,
    previous: Option<&CreativeSession>,
    claimed: Option<&Track>,
    lane: &ScoreLane,
    track_id: &str,
) -> AutomationLane {
    let previous_lane = claimed.and_then(|track| {
        previous.and_then(|session| {
            session.arrangement.automation_lanes.iter().find(|lane| {
                lane.target
                    == AutomationTarget::TrackMix {
                        track_id: track.id.clone(),
                        parameter: TrackMixParameter::Volume,
                    }
            })
        })
    });
    AutomationLane {
        id: previous_lane.map_or_else(
            || format!("score:volume:{}", riffra_control::new_instance_id()),
            |lane| lane.id.clone(),
        ),
        target: AutomationTarget::TrackMix {
            track_id: track_id.to_owned(),
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
    }
}

fn owner_instrument(id: &str) -> String {
    format!("instrument:{id}")
}

fn owner_kit(id: &str) -> String {
    format!("kit:{id}")
}

fn owner_bus(id: &str) -> String {
    format!("bus:{id}")
}

fn effect_device_id(owner: &str, id: &str) -> String {
    format!("score:effect:{owner}:{id}")
}

/// Builds one ordered effect chain from resolved production effects.
fn effect_chain(
    plan: &ScorePlan,
    production: &BTreeMap<String, ResolvedProduction>,
    owner: &str,
    field: &str,
    effects: &[score::production::EffectSpec],
) -> Result<Vec<EffectDevice>, ImportError> {
    let mut chain = Vec::with_capacity(effects.len());
    for (index, effect) in effects.iter().enumerate() {
        let key = format!("{field}[{index}]");
        let Some(ResolvedProduction::Effect { plugin, bypassed }) = production.get(&key) else {
            let message = format!("effect {key} was not resolved");
            return Err(ImportError {
                details: plan.source_detail(&key, &message),
                message,
            });
        };
        chain.push(EffectDevice {
            id: effect_device_id(owner, &effect.id),
            name: plugin.entry.name.clone(),
            bypassed: *bypassed,
            plugin: plugin.plugin.clone(),
        });
    }
    Ok(chain)
}

/// Attaches Score-owned effect chains to their owning tracks.
fn attach_track_effects(
    plan: &ScorePlan,
    production: &BTreeMap<String, ResolvedProduction>,
    tracks: &mut [Track],
    instrument_tracks: &BTreeMap<String, Vec<String>>,
    kit_tracks: &BTreeMap<String, String>,
) -> Result<(), ImportError> {
    for (id, effects) in &plan.production_effects {
        let (owner, field, track_ids) = if let Some(track_ids) = instrument_tracks.get(id) {
            (
                owner_instrument(id),
                format!("instruments.{id}.effects"),
                track_ids.clone(),
            )
        } else {
            let track_id = kit_tracks
                .get(id)
                .ok_or_else(|| format!("effect owner '{id}' has no track"))?;
            (
                owner_kit(id),
                format!("kits.{id}.effects"),
                vec![track_id.clone()],
            )
        };
        if track_ids.len() > 1 && !effects.is_empty() {
            let message = "effects on a multi-lane instrument have no single owner".to_owned();
            return Err(ImportError {
                details: plan.source_detail(&field, &message),
                message,
            });
        }
        let chain = effect_chain(plan, production, &owner, &field, effects)?;
        if let Some(track) = tracks.iter_mut().find(|track| track.id == track_ids[0]) {
            track.effects = chain;
        }
    }
    for (id, kit) in &plan.kits {
        let track_id = kit_tracks
            .get(id)
            .ok_or_else(|| format!("kit '{id}' has no track"))?;
        let chain = effect_chain(
            plan,
            production,
            &owner_kit(id),
            &format!("kits.{id}.effects"),
            &kit.effects,
        )?;
        if let Some(track) = tracks.iter_mut().find(|track| track.id == *track_id) {
            track.effects = chain;
        }
    }
    Ok(())
}

/// Builds the mixer, routing, and sidechains owned by the Score.
fn build_mixer(
    plan: &ScorePlan,
    production: &BTreeMap<String, ResolvedProduction>,
    previous: Option<&CreativeSession>,
    tracks: &mut [Track],
    instrument_tracks: &BTreeMap<String, Vec<String>>,
    kit_tracks: &BTreeMap<String, String>,
) -> Result<Mixer, ImportError> {
    let empty = score::production::mix::Mix {
        tracks: BTreeMap::new(),
        buses: BTreeMap::new(),
        sidechain: Vec::new(),
        automation: Vec::new(),
        master_gain_db: 0.0,
        master_effects: Vec::new(),
    };
    let mix = plan.mix.as_ref().unwrap_or(&empty);
    let mut buses = BTreeMap::<String, MixBus>::new();
    let mut bus_ids = BTreeMap::<String, String>::new();
    for (id, bus) in &mix.buses {
        let canonical = format!(
            "score:bus:{}:{id}",
            match bus.kind {
                score::production::mix::BusKind::Group => "group",
                score::production::mix::BusKind::Return => "return",
            }
        );
        let (muted, solo) = previous
            .and_then(|session| {
                session.arrangement.mixer.buses.iter().find(|candidate| {
                    candidate.name == *id
                        && candidate.kind
                            == match bus.kind {
                                score::production::mix::BusKind::Group => MixBusKind::Group,
                                score::production::mix::BusKind::Return => MixBusKind::Return,
                            }
                })
            })
            .map_or((false, false), |bus| (bus.muted, bus.solo));
        buses.insert(
            canonical.clone(),
            MixBus {
                id: canonical.clone(),
                name: id.clone(),
                kind: match bus.kind {
                    score::production::mix::BusKind::Group => MixBusKind::Group,
                    score::production::mix::BusKind::Return => MixBusKind::Return,
                },
                gain_db: bus.gain_db,
                pan: bus.pan,
                pan_law: PanLaw::LinearBalance,
                muted,
                solo,
                effects: effect_chain(
                    plan,
                    production,
                    &owner_bus(id),
                    &format!("mix.buses.{id}.effects"),
                    &bus.effects,
                )?,
            },
        );
        bus_ids.insert(id.clone(), canonical);
    }
    if let Some(previous) = previous {
        for bus in &previous.arrangement.mixer.buses {
            if bus.id.starts_with("score:bus:") {
                continue;
            }
            buses.insert(bus.id.clone(), bus.clone());
            bus_ids
                .entry(bus.name.clone())
                .or_insert_with(|| bus.id.clone());
        }
    }
    let mut sends = Vec::new();
    for track in tracks
        .iter_mut()
        .filter(|track| track.score_instrument.is_some())
    {
        let score_id = track
            .score_instrument
            .as_deref()
            .expect("score track")
            .strip_prefix("kit:")
            .unwrap_or_else(|| track.score_instrument.as_deref().expect("score track"));
        let Some(track_mix) = mix.tracks.get(score_id) else {
            track.output_bus_id = None;
            continue;
        };
        track.output_bus_id = track_mix.output.as_deref().and_then(|output| {
            if output == "master" {
                None
            } else {
                bus_ids.get(output).cloned()
            }
        });
        for (bus, send) in &track_mix.sends {
            let Some(return_bus_id) = bus_ids.get(bus) else {
                continue;
            };
            sends.push(Send {
                id: format!("score:send:{}:{return_bus_id}", track.id),
                track_id: track.id.clone(),
                return_bus_id: return_bus_id.clone(),
                level_db: send.level_db,
                position: match send.position {
                    score::production::mix::SendPosition::Pre => SendPosition::PreFader,
                    score::production::mix::SendPosition::Post => SendPosition::PostFader,
                },
                enabled: send.enabled,
            });
        }
    }
    let master_effects = effect_chain(
        plan,
        production,
        "master",
        "mix.master.effects",
        &mix.master_effects,
    )?;
    let previous_score_tracks = previous
        .map(|previous| {
            previous
                .arrangement
                .tracks
                .iter()
                .filter(|track| track.score_instrument.is_some())
                .map(|track| track.id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut all_track_ids = tracks
        .iter()
        .map(|track| track.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut all_effect_ids = tracks
        .iter()
        .flat_map(|track| &track.effects)
        .chain(buses.values().flat_map(|bus| &bus.effects))
        .chain(&master_effects)
        .map(|effect| effect.id.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(previous) = previous {
        all_track_ids.extend(
            previous
                .arrangement
                .tracks
                .iter()
                .filter(|track| track.score_instrument.is_none())
                .map(|track| track.id.as_str()),
        );
        all_effect_ids.extend(
            previous
                .arrangement
                .tracks
                .iter()
                .filter(|track| track.score_instrument.is_none())
                .flat_map(|track| &track.effects)
                .map(|effect| effect.id.as_str()),
        );
        let bus_ids = buses.keys().cloned().collect::<BTreeSet<_>>();
        for send in &previous.arrangement.mixer.sends {
            if send.id.starts_with("score:send:")
                || previous_score_tracks.contains(send.track_id.as_str())
                || !all_track_ids.contains(send.track_id.as_str())
                || !bus_ids.contains(&send.return_bus_id)
            {
                continue;
            }
            sends.push(send.clone());
        }
    }
    let mut sidechain_sends = Vec::new();
    for route in &mix.sidechain {
        let source_tracks = if let Some(track) = kit_tracks.get(&route.from) {
            vec![track.clone()]
        } else if let Some(tracks) = instrument_tracks.get(&route.from) {
            tracks.clone()
        } else {
            let message = format!("undefined sidechain source '{}'", route.from);
            return Err(ImportError {
                details: plan.source_detail("mix.sidechain", &message),
                message,
            });
        };
        let target = match &route.to.owner {
            score::production::mix::Owner::Instrument(id) => {
                effect_device_id(&owner_instrument(id), &route.to.id)
            }
            score::production::mix::Owner::Kit(id) => {
                effect_device_id(&owner_kit(id), &route.to.id)
            }
            score::production::mix::Owner::Bus(id) => {
                effect_device_id(&owner_bus(id), &route.to.id)
            }
            score::production::mix::Owner::Master => effect_device_id("master", &route.to.id),
        };
        for source in source_tracks {
            sidechain_sends.push(SidechainSend {
                id: format!("score:sidechain:{source}:{target}"),
                source_track_id: source,
                target_device_id: target.clone(),
                level_db: route.send.level_db,
                position: match route.send.position {
                    score::production::mix::SendPosition::Pre => SendPosition::PreFader,
                    score::production::mix::SendPosition::Post => SendPosition::PostFader,
                },
                enabled: route.send.enabled,
            });
        }
    }
    if let Some(previous) = previous {
        for send in &previous.arrangement.mixer.sidechain_sends {
            if send.id.starts_with("score:sidechain:")
                || previous_score_tracks.contains(send.source_track_id.as_str())
                || !all_track_ids.contains(send.source_track_id.as_str())
                || !all_effect_ids.contains(send.target_device_id.as_str())
            {
                continue;
            }
            sidechain_sends.push(send.clone());
        }
    }
    Ok(Mixer {
        master_effects,
        buses: buses.into_values().collect(),
        sends,
        sidechain_sends,
    })
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
    staged: &mut StagedResources,
    mode: BuildMode,
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
    } else if mode == BuildMode::Validate {
        let definition_json = std::fs::read_to_string(&source.definition)
            .map_err(|error| format!("instrument definition could not be read: {error}"))?;
        TrackInstrument::user_snapshot(
            "score:instrument".into(),
            source.name.clone(),
            format!("user:{}", riffra_control::new_instance_id()),
            riffra_control::new_instance_id(),
            definition_json,
        )?
    } else {
        let snapshot = store.snapshot_definition(&source.definition)?;
        staged.snapshots.insert(snapshot.package_root.clone());
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
        if is_production_instrument(plan, &lane.id) {
            for (index, event) in lane.pattern.events.iter().enumerate() {
                if matches!(
                    event,
                    MusicalEvent::ParameterChange { .. } | MusicalEvent::ParameterRamp { .. }
                ) {
                    let message = "Sonalloy instrument parameter events cannot target a production VST3; use mix automation".to_owned();
                    let path = lane
                        .sources
                        .get(index)
                        .map_or("", |source| source.path.as_str());
                    return Err(ImportError {
                        details: plan.source_detail(path, &message),
                        message,
                    });
                }
            }
            continue;
        }
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
            production: BTreeMap::new(),
        }
    }

    fn resolved_plugin(name: &str) -> library::ResolvedPlugin {
        use crate::api::output::{PluginEntry, PluginFormat, PluginRole, PluginScanState};
        library::ResolvedPlugin {
            entry: PluginEntry {
                id: name.to_lowercase(),
                name: name.into(),
                vendor: None,
                version: None,
                identifier: Some(format!("{name}-class")),
                format: PluginFormat::Vst3,
                role: Some(PluginRole::Instrument),
                path: format!("/plugins/{name}.vst3"),
                bundle: true,
                modified_at_ms: None,
                scan_state: PluginScanState::Validated,
            },
            plugin: Vst3Plugin::new(format!("/plugins/{name}.vst3")),
            range: None,
            notes: None,
        }
    }

    fn effect_name(effect: &score::production::EffectSpec) -> String {
        effect
            .vst3
            .as_ref()
            .and_then(|vst3| vst3.plugin.clone())
            .unwrap_or_else(|| "Effect".into())
    }

    fn insert_effects(
        production: &mut BTreeMap<String, ResolvedProduction>,
        field: &str,
        effects: &[score::production::EffectSpec],
    ) {
        for (index, effect) in effects.iter().enumerate() {
            production.insert(
                format!("{field}[{index}]"),
                ResolvedProduction::Effect {
                    plugin: resolved_plugin(&effect_name(effect)),
                    bypassed: effect.bypassed.unwrap_or(false),
                },
            );
        }
    }

    fn production_document(yaml: &str) -> ScoreDocument {
        let plan = score::compile(yaml).expect("production score compiles");
        let mut production = BTreeMap::new();
        for (id, source) in &plan.production_sources {
            let name = source
                .vst3
                .as_ref()
                .and_then(|vst3| vst3.plugin.clone())
                .expect("inline plugin");
            production.insert(
                format!("instruments.{id}.source"),
                ResolvedProduction::Source {
                    plugin: resolved_plugin(&name),
                    velocity: source.velocity.clone(),
                },
            );
        }
        for (id, kit) in &plan.kits {
            let name = kit
                .source
                .vst3
                .as_ref()
                .and_then(|vst3| vst3.plugin.clone())
                .expect("kit plugin");
            production.insert(
                format!("kits.{id}.source"),
                ResolvedProduction::Source {
                    plugin: resolved_plugin(&name),
                    velocity: kit.source.velocity.clone(),
                },
            );
            insert_effects(&mut production, &format!("kits.{id}.effects"), &kit.effects);
        }
        for (id, effects) in &plan.production_effects {
            insert_effects(
                &mut production,
                &format!("instruments.{id}.effects"),
                effects,
            );
        }
        if let Some(mix) = &plan.mix {
            for (id, bus) in &mix.buses {
                insert_effects(
                    &mut production,
                    &format!("mix.buses.{id}.effects"),
                    &bus.effects,
                );
            }
            insert_effects(&mut production, "mix.master.effects", &mix.master_effects);
        }
        ScoreDocument {
            path: PathBuf::from("/scores/production.yaml"),
            yaml: yaml.into(),
            plan,
            bindings: BTreeMap::new(),
            sources: BTreeMap::new(),
            production,
        }
    }

    fn store(root: &Path) -> UserInstrumentStore {
        UserInstrumentStore::new(root, Path::new("sonalloy"))
    }

    #[test]
    fn validation_and_inspection_resolve_vst_without_sonalloy_or_persistent_assets() {
        use crate::api::output::{
            PluginEntry, PluginFormat, PluginRole, PluginScanState, ScanReport,
        };
        let root =
            std::env::temp_dir().join(format!("riffra-score-production-{}", uuid::Uuid::now_v7()));
        let score_dir = root.join("music/song");
        let library_dir = root.join("library/instruments");
        let vst_path = root.join("Synth.vst3");
        std::fs::create_dir_all(&score_dir).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        std::fs::create_dir_all(&vst_path).unwrap();
        std::fs::write(
            library_dir.join("lead.yaml"),
            "source:\n  vst3: {plugin: Synth}\nrange: [40, 90]\nnotes: lead sound\n",
        )
        .unwrap();
        let report = ScanReport {
            root: root.to_string_lossy().into_owned(),
            started_at_ms: 0,
            finished_at_ms: 0,
            issues: Vec::new(),
            plugins: vec![PluginEntry {
                id: "synth".into(),
                name: "Synth".into(),
                vendor: None,
                version: None,
                identifier: Some("class-id".into()),
                format: PluginFormat::Vst3,
                role: Some(PluginRole::Instrument),
                path: vst_path.to_string_lossy().into_owned(),
                bundle: true,
                modified_at_ms: None,
                scan_state: PluginScanState::Validated,
            }],
        };
        crate::plugins::catalog::save(&root, &report).unwrap();
        let mut score: Value = serde_json::from_str(SCORE).unwrap();
        score["instruments"]["lead"] = json!({"external":true, "source":{"use":"lead"}});
        let score_path = score_dir.join("score.yaml");
        std::fs::write(&score_path, score.to_string()).unwrap();
        let catalog = crate::test_support::empty_built_in_catalog();
        let validation = validate(
            &score_path,
            &BTreeMap::new(),
            &root,
            catalog,
            Path::new("unused-scanner"),
            Path::new("unavailable-sonalloy"),
        );
        assert!(validation.valid, "{:?}", validation.diagnostics);
        let inspection = inspect(
            &score_path,
            &BTreeMap::new(),
            false,
            &root,
            catalog,
            Path::new("unused-scanner"),
            Path::new("unavailable-sonalloy"),
        )
        .unwrap();
        assert!(matches!(
            inspection.instruments[0].kind,
            ScoreInstrumentKind::Vst3
        ));
        assert_eq!(inspection.instruments[0].definition, None);
        assert_eq!(
            inspection.instruments[0].plugin_identifier.as_deref(),
            Some("class-id")
        );
        assert_eq!(inspection.instruments[0].library_range, Some((40, 90)));
        assert!(!root.join("assets").exists());
        assert!(!root.join("instruments").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn notes(session: &CreativeSession) -> Vec<(u64, u8)> {
        session.arrangement.midi_clips[0]
            .notes
            .iter()
            .map(|note| (note.start_tick.0, note.note))
            .collect()
    }

    const PRODUCTION: &str = r#"{
        "meta":{"title":"Production","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"verse","bars":1,"tonic":"C","scale":"ionian"}],
        "instruments":{
            "bass":{"source":{"vst3":{"plugin":"Bass"},"velocity":[[1,64],[127,64]]},"effects":[{"id":"duck","vst3":{"plugin":"Compressor"}}]},
            "kick":{"kit":"drums","trigger_note_midi":36},
            "snare":{"kit":"drums","trigger_note_midi":38},
            "ambience":{"audio":true,"gain_db":-5}
        },
        "kits":{"drums":{"source":{"vst3":{"plugin":"Drums"},"velocity":[[1,40],[127,40]]},"effects":[{"id":"kit-eq","vst3":{"plugin":"EQ"}}]}},
        "drums":{"beat":{"parts":{"kick":"x...|....|....|....","snare":"....|x...|....|...."}}},
        "sections":{"verse":{"drums":"beat","lines":{"main":{"instrument":"bass","melody":"theme"}}}},
        "melodies":{"theme":["1 - - - | 3 - - - | 5 - - - | 1' - - -"]},
        "mix":{
            "tracks":{
                "bass":{"output":"rhythm","sends":{"room":-18}},
                "drums":{"gain_db":-2,"pan":0.0,"output":"rhythm","sends":{"room":{"level_db":-14,"position":"pre","enabled":true}}}
            },
            "buses":{
                "rhythm":{"kind":"group","gain_db":-2,"pan":0},
                "room":{"kind":"return","effects":[{"id":"reverb","vst3":{"plugin":"Reverb"}}]}
            },
            "sidechain":[{"from":"drums","to":{"instrument":"bass","effect":"duck"},"level_db":0,"position":"post"}],
            "master":{"gain_db":-1,"effects":[{"id":"limiter","vst3":{"plugin":"Limiter"}}]}
        }
    }"#;

    #[test]
    fn production_build_aggregates_kits_applies_velocity_and_builds_the_mixer() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-production-build-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let mut document = production_document(PRODUCTION);
        let mut staged = StagedResources::new(&root);
        let session = build(&mut document, None, &store, &mut staged, BuildMode::Stage)
            .expect("production build");

        assert_eq!(session.arrangement.tracks.len(), 3);
        let bass = &session.arrangement.tracks[0];
        assert_eq!(bass.score_instrument.as_deref(), Some("bass"));
        assert!(matches!(
            bass.instrument.as_ref().expect("bass device").source,
            TrackInstrumentSource::Vst3(_)
        ));
        assert_eq!(bass.effects[0].id, "score:effect:instrument:bass:duck");
        assert_eq!(session.arrangement.midi_clips[0].notes.len(), 4);
        assert!(
            session.arrangement.midi_clips[0]
                .notes
                .iter()
                .all(|note| note.velocity == 64)
        );

        let drums = &session.arrangement.tracks[1];
        assert_eq!(drums.score_instrument.as_deref(), Some("kit:drums"));
        assert_eq!(drums.gain_db, -2.0);
        assert_eq!(drums.effects[0].id, "score:effect:kit:drums:kit-eq");
        let kit_clip = &session.arrangement.midi_clips[1];
        assert_eq!(kit_clip.notes.len(), 2);
        assert!(kit_clip.notes.iter().all(|note| note.velocity == 40));
        assert_eq!(
            kit_clip
                .notes
                .iter()
                .map(|note| note.note)
                .collect::<Vec<_>>(),
            [36, 38]
        );

        let ambience = &session.arrangement.tracks[2];
        assert_eq!(ambience.kind, TrackKind::Audio);
        assert_eq!(ambience.score_instrument.as_deref(), Some("ambience"));
        assert_eq!(ambience.gain_db, -5.0);

        let mixer = &session.arrangement.mixer;
        assert_eq!(mixer.buses.len(), 2);
        assert_eq!(mixer.sends.len(), 2);
        assert_eq!(mixer.sidechain_sends.len(), 1);
        assert_eq!(mixer.master_effects.len(), 1);
        assert_eq!(session.settings.master_db, -1.0);
        assert_eq!(
            bass.output_bus_id.as_deref(),
            Some("score:bus:group:rhythm")
        );
        assert_eq!(
            mixer.sidechain_sends[0].target_device_id,
            "score:effect:instrument:bass:duck"
        );
        assert!(staged.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn production_rebuild_keeps_semantic_ids_and_updates_only_the_change() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-production-rebuild-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let mut first = production_document(PRODUCTION);
        let mut staged = StagedResources::new(&root);
        let session = build(&mut first, None, &store, &mut staged, BuildMode::Stage)
            .expect("initial production build");
        let track_ids = session
            .arrangement
            .tracks
            .iter()
            .map(|track| track.id.clone())
            .collect::<Vec<_>>();
        let device_ids = session
            .arrangement
            .tracks
            .iter()
            .filter_map(|track| track.instrument.as_ref())
            .map(|instrument| instrument.id.clone())
            .collect::<Vec<_>>();
        let clip_ids = session
            .arrangement
            .midi_clips
            .iter()
            .map(|clip| clip.id.clone())
            .collect::<Vec<_>>();
        let effect_ids = session
            .arrangement
            .tracks
            .iter()
            .flat_map(|track| &track.effects)
            .map(|effect| effect.id.clone())
            .collect::<Vec<_>>();
        let bus_ids = session
            .arrangement
            .mixer
            .buses
            .iter()
            .map(|bus| bus.id.clone())
            .collect::<Vec<_>>();
        let send_ids = session
            .arrangement
            .mixer
            .sends
            .iter()
            .map(|send| send.id.clone())
            .collect::<Vec<_>>();
        let sidechain_ids = session
            .arrangement
            .mixer
            .sidechain_sends
            .iter()
            .map(|send| send.id.clone())
            .collect::<Vec<_>>();

        let changed = PRODUCTION
            .replace("3 - - -", "4 - - -")
            .replace(r#""drums":{"gain_db":-2"#, r#""drums":{"gain_db":-4"#);
        let mut second = production_document(&changed);
        let rebuilt = build(
            &mut second,
            Some(&session),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
        .expect("production rebuild");

        assert_eq!(
            rebuilt
                .arrangement
                .tracks
                .iter()
                .map(|track| track.id.clone())
                .collect::<Vec<_>>(),
            track_ids
        );
        assert_eq!(
            rebuilt
                .arrangement
                .tracks
                .iter()
                .filter_map(|track| track.instrument.as_ref())
                .map(|instrument| instrument.id.clone())
                .collect::<Vec<_>>(),
            device_ids
        );
        assert_eq!(
            rebuilt
                .arrangement
                .midi_clips
                .iter()
                .map(|clip| clip.id.clone())
                .collect::<Vec<_>>(),
            clip_ids
        );
        assert_eq!(
            rebuilt
                .arrangement
                .tracks
                .iter()
                .flat_map(|track| &track.effects)
                .map(|effect| effect.id.clone())
                .collect::<Vec<_>>(),
            effect_ids
        );
        assert_eq!(
            rebuilt
                .arrangement
                .mixer
                .buses
                .iter()
                .map(|bus| bus.id.clone())
                .collect::<Vec<_>>(),
            bus_ids
        );
        assert_eq!(
            rebuilt
                .arrangement
                .mixer
                .sends
                .iter()
                .map(|send| send.id.clone())
                .collect::<Vec<_>>(),
            send_ids
        );
        assert_eq!(
            rebuilt
                .arrangement
                .mixer
                .sidechain_sends
                .iter()
                .map(|send| send.id.clone())
                .collect::<Vec<_>>(),
            sidechain_ids
        );
        assert_eq!(rebuilt.arrangement.midi_clips[0].notes[1].note, 65);
        assert_eq!(rebuilt.arrangement.tracks[1].gain_db, -4.0);
        assert!(rebuilt.score_performance_equals(&rebuilt));
        assert!(!session.score_performance_equals(&rebuilt));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rebuilding_keeps_project_identity_work_state_and_device_ids() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-rebuild-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let mut first = document(SCORE);
        let mut staged = StagedResources::new(&root);
        let mut session =
            build(&mut first, None, &store, &mut staged, BuildMode::Stage).expect("initial build");
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
        let rebuilt = build(
            &mut second,
            Some(&session),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
        .expect("rebuild");

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
        let mut staged = StagedResources::new(&root);
        let session =
            build(&mut first, None, &store, &mut staged, BuildMode::Stage).expect("initial build");

        let mut second = document(&format!(
            "{SCORE}
# Only the text changed."
        ));
        let rebuilt = build(
            &mut second,
            Some(&session),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
        .expect("text-only rebuild");

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
        let mut staged = StagedResources::new(&root);
        let session =
            build(&mut first, None, &store, &mut staged, BuildMode::Stage).expect("initial build");
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
        let rebuilt = build(
            &mut inserted,
            Some(&session),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
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
        let rebuilt = build(
            &mut changed_gain,
            Some(&rebuilt),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
        .expect("gain rebuild");
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
        let mut staged = StagedResources::new(&root);
        let mut session =
            build(&mut first, None, &store, &mut staged, BuildMode::Stage).expect("initial build");
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
        let rebuilt = build(
            &mut second,
            Some(&session),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
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
            staged: StagedResources::new(Path::new("")),
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
            staged: StagedResources::new(Path::new("")),
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
