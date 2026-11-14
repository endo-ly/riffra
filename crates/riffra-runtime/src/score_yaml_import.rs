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
    ScoreAudioClipInspection, ScoreAutomationInspection, ScoreBusInspection, ScoreChordInspection,
    ScoreDiagnostic, ScoreDiagnosticSeverity, ScoreEffectInspection, ScoreInspection,
    ScoreInstrumentInspection, ScoreInstrumentKind, ScoreKitInspection, ScoreLaneInspection,
    ScoreSectionInspection, ScoreSendInspection, ScoreSidechainInspection, ScoreSummary,
    ScoreTrackInspection, ScoreValidation,
};
use crate::api::params::ProjectImportScoreParams;
use crate::instrument::{BuiltInInstrumentCatalog, UserInstrumentStore};
use crate::plugins::validation::DisplayRequest;
use crate::score::{self, ScoreLane, ScorePlan, events::MusicalEvent, syntax};
use riffra_core::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Fingerprint {
    bytes: u64,
    digest: [u8; 32],
}

type Dependencies = BTreeMap<PathBuf, Fingerprint>;

fn fingerprint(path: &Path) -> Result<Fingerprint, String> {
    let mut file =
        std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok(Fingerprint {
        bytes,
        digest: hasher.finalize().into(),
    })
}

fn record_dependency(dependencies: &mut Dependencies, path: &Path) -> Result<(), String> {
    let canonical =
        std::fs::canonicalize(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let value = fingerprint(&canonical)?;
    dependencies.insert(canonical.clone(), value.clone());
    if path != canonical {
        dependencies.insert(path.to_path_buf(), value);
    }
    Ok(())
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

    fn plugin_mut(&mut self) -> &mut library::ResolvedPlugin {
        match self {
            Self::Source { plugin, .. } | Self::Effect { plugin, .. } => plugin,
        }
    }
}

type ResolvedProductionMap = BTreeMap<String, ResolvedProduction>;
type ResolvedAutomationMap = BTreeMap<usize, ResolvedAutomation>;
type SourceResolution = (
    library::ResolvedPlugin,
    score::production::Vst3Spec,
    Option<Vec<(u8, u8)>>,
);
type EffectResolution = (library::ResolvedPlugin, score::production::Vst3Spec, bool);

/// One validated Score file with every instrument source resolved.
struct ScoreDocument {
    path: PathBuf,
    yaml: String,
    plan: ScorePlan,
    bindings: BTreeMap<String, PathBuf>,
    sources: BTreeMap<String, Source>,
    production: ResolvedProductionMap,
    automation: ResolvedAutomationMap,
    production_inputs: BTreeMap<String, String>,
    dependencies: Dependencies,
}

fn is_production_instrument(plan: &ScorePlan, id: &str) -> bool {
    plan.production_sources.contains_key(id)
        || plan.kit_members.contains_key(id)
        || plan.audio_instruments.contains(id)
}

fn validate_audio(
    plan: &ScorePlan,
    base: &Path,
    dependencies: &mut Dependencies,
) -> Result<(), ImportError> {
    use num_traits::ToPrimitive;
    for (section, clips) in &plan.audio_clips {
        for (index, clip) in clips.iter().enumerate() {
            let field = format!("sections.{section}.audio_clips[{index}]");
            let fail = |message: String| ImportError {
                details: plan.source_detail(&field, &message),
                message,
            };
            let path = base.join(&clip.file);
            record_dependency(dependencies, &path).map_err(fail)?;
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

/// Device-parameter automation resolved against the plugin's final state.
#[derive(Clone)]
struct ResolvedAutomation {
    parameter_id: String,
    values: Vec<f64>,
}

/// One production use while its parameters and automation are still pending.
struct PendingUse {
    production: ResolvedProduction,
    spec: score::production::Vst3Spec,
    displays: Vec<DisplayRequest>,
    needs_probe: bool,
    has_automation: bool,
    previous_plugin: Option<Vst3Plugin>,
    previous_automation: BTreeMap<usize, ResolvedAutomation>,
}

impl PendingUse {
    fn base_plugin(&self) -> &Vst3Plugin {
        &self.production.plugin().plugin
    }

    fn base_key(&self) -> Result<String, String> {
        serde_json::to_string(self.base_plugin()).map_err(|error| error.to_string())
    }
}

/// One automation lane whose plugin parameter is resolved after the probe.
struct PendingAutomation {
    use_key: String,
    parameter: String,
    interpolation: score::production::mix::Interpolation,
    points: Vec<score::production::ParameterValue>,
    requests: Vec<Option<usize>>,
}

fn effect_field(plan: &ScorePlan, effect: &score::production::mix::EffectRef) -> Option<String> {
    use score::production::mix::Owner;
    let index = match &effect.owner {
        Owner::Instrument(id) => plan
            .production_effects
            .get(id)?
            .iter()
            .position(|item| item.id == effect.id)?,
        Owner::Kit(id) => plan
            .kits
            .get(id)?
            .effects
            .iter()
            .position(|item| item.id == effect.id)?,
        Owner::Bus(id) => plan
            .mix
            .as_ref()?
            .buses
            .get(id)?
            .effects
            .iter()
            .position(|item| item.id == effect.id)?,
        Owner::Master => plan
            .mix
            .as_ref()?
            .master_effects
            .iter()
            .position(|item| item.id == effect.id)?,
    };
    Some(match &effect.owner {
        Owner::Instrument(id) => format!("instruments.{id}.effects[{index}]"),
        Owner::Kit(id) => format!("kits.{id}.effects[{index}]"),
        Owner::Bus(id) => format!("mix.buses.{id}.effects[{index}]"),
        Owner::Master => format!("mix.master.effects[{index}]"),
    })
}

fn automation_use_key(
    plan: &ScorePlan,
    target: &score::production::mix::AutomationTarget,
) -> Option<String> {
    match target {
        score::production::mix::AutomationTarget::Effect { effect, .. } => {
            effect_field(plan, effect)
        }
        score::production::mix::AutomationTarget::Source { instrument, .. } => {
            Some(format!("instruments.{instrument}.source"))
        }
        _ => None,
    }
}

fn automation_score_target(use_key: &str, parameter: &str) -> String {
    format!("{use_key}|{parameter}")
}

fn previous_source_plugin<'a>(
    session: &'a CreativeSession,
    score_key: &str,
) -> Option<&'a Vst3Plugin> {
    session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.score_instrument.as_deref() == Some(score_key))
        .and_then(|track| track.instrument.as_ref())
        .and_then(TrackInstrument::as_vst3)
}

fn previous_effect_plugin<'a>(
    session: &'a CreativeSession,
    device_id: &str,
) -> Option<&'a Vst3Plugin> {
    session
        .arrangement
        .tracks
        .iter()
        .flat_map(|track| &track.effects)
        .chain(
            session
                .arrangement
                .mixer
                .buses
                .iter()
                .flat_map(|bus| &bus.effects),
        )
        .chain(&session.arrangement.mixer.master_effects)
        .find(|effect| effect.id == device_id)
        .map(|effect| &effect.plugin)
}

fn parameter_value_json(value: &score::production::ParameterValue) -> Value {
    match value {
        score::production::ParameterValue::Normalized(value) => json!({"normalized": value}),
        score::production::ParameterValue::Display(value) => json!({"display": value}),
    }
}

fn production_input_fingerprint(
    spec: &score::production::Vst3Spec,
    plugin: &Vst3Plugin,
    automations: &[&PendingAutomation],
) -> String {
    let params = spec
        .params
        .iter()
        .map(|(name, value)| (name.clone(), parameter_value_json(value)))
        .collect::<BTreeMap<_, _>>();
    let automations = automations
        .iter()
        .map(|automation| {
            json!({
                "parameter": automation.parameter,
                "interpolation": format!("{:?}", automation.interpolation),
                "points": automation.points.iter().map(parameter_value_json).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "plugin": spec.plugin,
        "identifier": spec.identifier,
        "state": plugin.state_data,
        "parameterValues": plugin.parameter_values,
        "params": params,
        "automation": automations,
    });
    let digest = Sha256::digest(serde_json::to_vec(&payload).expect("fingerprint serializes"));
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct PreviousUse {
    plugin: Vst3Plugin,
    automation: BTreeMap<usize, ResolvedAutomation>,
}

fn resolve_production(
    plan: &ScorePlan,
    base: &Path,
    data_root: &Path,
    scanner: &Path,
    dependencies: &mut Dependencies,
    previous: Option<&CreativeSession>,
) -> Result<
    (
        ResolvedProductionMap,
        ResolvedAutomationMap,
        BTreeMap<String, String>,
    ),
    ImportError,
> {
    let library = library::Library::new(base);
    let mut uses = BTreeMap::<String, PendingUse>::new();
    let mut insert_source = |field: String,
                             source: &score::production::SourceSpec,
                             score_key: String|
     -> Result<(), ImportError> {
        let mut resolve = || -> Result<SourceResolution, String> {
            let resolved = library.instrument(source, dependencies)?;
            let vst3 = resolved
                .source
                .vst3
                .clone()
                .ok_or_else(|| "source requires a VST3 plugin".to_owned())?;
            let mut plugin = library::resolve_plugin(
                data_root,
                &vst3,
                crate::api::output::PluginRole::Instrument,
                dependencies,
            )?;
            plugin.range = resolved.range;
            plugin.notes = resolved.notes;
            Ok((plugin, vst3, resolved.source.velocity))
        };
        let (plugin, spec, velocity) = resolve().map_err(|message| ImportError {
            details: plan.source_detail(&field, &message),
            message,
        })?;
        let displays =
            parameters::display_requests(&spec.params).map_err(|message| ImportError {
                details: plan.source_detail(&field, &message),
                message,
            })?;
        uses.insert(
            field,
            PendingUse {
                production: ResolvedProduction::Source { plugin, velocity },
                displays,
                needs_probe: !spec.params.is_empty(),
                has_automation: false,
                previous_plugin: previous
                    .and_then(|session| previous_source_plugin(session, &score_key))
                    .cloned(),
                previous_automation: BTreeMap::new(),
                spec,
            },
        );
        Ok(())
    };
    for (id, source) in &plan.production_sources {
        insert_source(format!("instruments.{id}.source"), source, id.clone())?;
    }
    for (id, kit) in &plan.kits {
        insert_source(
            format!("kits.{id}.source"),
            &kit.source,
            format!("kit:{id}"),
        )?;
    }
    let mut insert_effects = |field: String,
                              owner: &str,
                              effects: &[score::production::EffectSpec]|
     -> Result<(), ImportError> {
        for (index, effect) in effects.iter().enumerate() {
            let path = format!("{field}[{index}]");
            let mut resolve = || -> Result<EffectResolution, String> {
                let effect = library.effect(effect, dependencies)?;
                let vst3 = effect
                    .vst3
                    .clone()
                    .ok_or_else(|| "effect requires a VST3 plugin".to_owned())?;
                let plugin = library::resolve_plugin(
                    data_root,
                    &vst3,
                    crate::api::output::PluginRole::Effect,
                    dependencies,
                )?;
                Ok((plugin, vst3, effect.bypassed.unwrap_or(false)))
            };
            let (plugin, spec, bypassed) = resolve().map_err(|message| ImportError {
                details: plan.source_detail(&path, &message),
                message,
            })?;
            uses.insert(
                path.clone(),
                PendingUse {
                    production: ResolvedProduction::Effect { plugin, bypassed },
                    displays: parameters::display_requests(&spec.params).map_err(|message| {
                        ImportError {
                            details: plan.source_detail(&path, &message),
                            message,
                        }
                    })?,
                    needs_probe: !spec.params.is_empty(),
                    has_automation: false,
                    previous_plugin: previous
                        .and_then(|session| {
                            previous_effect_plugin(session, &effect_device_id(owner, &effect.id))
                        })
                        .cloned(),
                    previous_automation: BTreeMap::new(),
                    spec,
                },
            );
        }
        Ok(())
    };
    for (id, effects) in &plan.production_effects {
        insert_effects(
            format!("instruments.{id}.effects"),
            &owner_instrument(id),
            effects,
        )?;
    }
    for (id, kit) in &plan.kits {
        insert_effects(format!("kits.{id}.effects"), &owner_kit(id), &kit.effects)?;
    }
    if let Some(mix) = &plan.mix {
        for (id, bus) in &mix.buses {
            insert_effects(
                format!("mix.buses.{id}.effects"),
                &owner_bus(id),
                &bus.effects,
            )?;
        }
        insert_effects(
            "mix.master.effects".to_owned(),
            "master",
            &mix.master_effects,
        )?;
    }
    let mut automations = BTreeMap::<usize, PendingAutomation>::new();
    if let Some(mix) = &plan.mix {
        for (index, lane) in mix.automation.iter().enumerate() {
            if !matches!(
                lane.target,
                score::production::mix::AutomationTarget::Effect { .. }
                    | score::production::mix::AutomationTarget::Source { .. }
            ) {
                continue;
            }
            let Some(use_key) = automation_use_key(plan, &lane.target) else {
                let message = "plugin automation target was not resolved".to_owned();
                return Err(ImportError {
                    details: plan.source_detail(&format!("mix.automation[{index}]"), &message),
                    message,
                });
            };
            let parameter = automation_parameter(lane, index, plan)?;
            let score_target = automation_score_target(&use_key, &parameter);
            let previous_automation = previous
                .and_then(|session| {
                    session
                        .arrangement
                        .automation_lanes
                        .iter()
                        .find(|lane| lane.score_target.as_deref() == Some(&score_target))
                })
                .and_then(|lane| {
                    let AutomationTarget::DeviceParameter { parameter_id, .. } = &lane.target
                    else {
                        return None;
                    };
                    Some(ResolvedAutomation {
                        parameter_id: parameter_id.clone(),
                        values: lane.points.iter().map(|point| point.value).collect(),
                    })
                });
            let Some(pending) = uses.get_mut(&use_key) else {
                let message = format!("automation target {use_key} was not resolved");
                return Err(ImportError {
                    details: plan.source_detail("mix.automation", &message),
                    message,
                });
            };
            if let Some(previous_automation) = previous_automation {
                pending
                    .previous_automation
                    .insert(index, previous_automation);
            }
            pending.needs_probe = true;
            pending.has_automation = true;
            let mut requests = Vec::with_capacity(lane.points.len());
            for point in &lane.points {
                requests.push(match &point.value {
                    score::production::ParameterValue::Normalized(_) => None,
                    score::production::ParameterValue::Display(display) => {
                        let (name, index) =
                            parameters::selector(&parameter).map_err(|message| ImportError {
                                details: plan.source_detail("mix.automation", &message),
                                message,
                            })?;
                        pending.displays.push(DisplayRequest {
                            name: name.into(),
                            index,
                            display: display.clone(),
                        });
                        Some(pending.displays.len() - 1)
                    }
                });
            }
            automations.insert(
                index,
                PendingAutomation {
                    use_key,
                    parameter,
                    interpolation: lane.interpolation,
                    points: lane
                        .points
                        .iter()
                        .map(|point| point.value.clone())
                        .collect(),
                    requests,
                },
            );
        }
    }
    if let Some(mix) = &plan.mix {
        let mut capabilities =
            BTreeMap::<String, crate::api::output::DeviceSidechainCapability>::new();
        for (index, route) in mix.sidechain.iter().enumerate() {
            let path = format!("mix.sidechain[{index}].to");
            let field = effect_field(plan, &route.to).ok_or_else(|| ImportError {
                details: plan.source_detail(&path, "sidechain target was not resolved"),
                message: "sidechain target was not resolved".to_owned(),
            })?;
            let Some(pending) = uses.get(&field) else {
                return Err(ImportError {
                    details: plan.source_detail(&path, "sidechain target was not resolved"),
                    message: "sidechain target was not resolved".to_owned(),
                });
            };
            let ResolvedProduction::Effect { plugin, .. } = &pending.production else {
                return Err(ImportError {
                    details: plan.source_detail(&path, "sidechain target is not an effect"),
                    message: "sidechain target is not an effect".to_owned(),
                });
            };
            let target_device = effect_device_id_for(&route.to);
            if previous.is_some_and(|session| {
                session
                    .arrangement
                    .mixer
                    .sidechain_sends
                    .iter()
                    .any(|send| send.target_device_id == target_device)
            }) {
                continue;
            }
            let plugin_path = plugin.plugin.path.clone();
            let capability = if let Some(capability) = capabilities.get(&plugin_path) {
                capability.clone()
            } else {
                let capability = crate::plugins::validation::probe_sidechain_capability(
                    scanner,
                    Path::new(&plugin_path),
                )
                .map_err(|message| ImportError {
                    details: plan.source_detail(&path, &message),
                    message,
                })?;
                capabilities.insert(plugin_path.clone(), capability.clone());
                capability
            };
            if !capability.available {
                let message = capability
                    .reason
                    .unwrap_or_else(|| "plugin has no auxiliary sidechain input".to_owned());
                return Err(ImportError {
                    details: plan.source_detail(&path, &message),
                    message,
                });
            }
        }
    }
    let production_inputs = uses
        .iter()
        .map(|(key, pending)| {
            let automations = automations
                .values()
                .filter(|automation| automation.use_key == *key)
                .collect::<Vec<_>>();
            (
                key.clone(),
                production_input_fingerprint(&pending.spec, pending.base_plugin(), &automations),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut reused = BTreeMap::<String, PreviousUse>::new();
    if let Some(previous) = previous
        && let Some(source) = previous.score_source.as_ref()
    {
        for (key, pending) in &uses {
            if !pending.needs_probe
                || source.production_inputs.get(key) != production_inputs.get(key)
            {
                continue;
            }
            let Some(plugin) = &pending.previous_plugin else {
                continue;
            };
            let mut automation = BTreeMap::new();
            let mut complete = true;
            for (index, lane) in &automations {
                if lane.use_key != *key {
                    continue;
                }
                let Some(previous) = pending.previous_automation.get(index).cloned() else {
                    complete = false;
                    break;
                };
                if previous.values.len() != lane.points.len() {
                    complete = false;
                    break;
                }
                automation.insert(*index, previous);
            }
            if complete {
                reused.insert(
                    key.clone(),
                    PreviousUse {
                        plugin: plugin.clone(),
                        automation,
                    },
                );
            }
        }
    }
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for (key, pending) in &uses {
        if !pending.needs_probe || reused.contains_key(key) {
            continue;
        }
        let base_key = pending.base_key().map_err(|error| ImportError {
            details: plan.source_detail(key, &error),
            message: error,
        })?;
        groups.entry(base_key).or_default().push(key.clone());
    }
    let mut values = BTreeMap::<String, Vst3Plugin>::new();
    let mut resolved_automation = BTreeMap::new();
    for keys in groups.into_values() {
        let mut displays = Vec::new();
        let mut ranges = BTreeMap::<String, (usize, usize)>::new();
        for key in &keys {
            let pending = &uses[key];
            let start = displays.len();
            displays.extend(pending.displays.iter().cloned());
            ranges.insert(key.clone(), (start, pending.displays.len()));
        }
        let base_plugin = uses[&keys[0]].base_plugin();
        let description =
            crate::plugins::validation::describe_parameters(scanner, base_plugin, &displays)
                .map_err(|message| ImportError {
                    details: plan.source_detail(&keys[0], &message),
                    message,
                })?;
        for key in &keys {
            let pending = &uses[key];
            let (start, len) = ranges[key];
            let conversions = &description.conversions[start..start + len];
            let mut plugin = pending.production.plugin().plugin.clone();
            if !pending.spec.params.is_empty() {
                plugin.parameter_values = parameters::assign_values(
                    &description.parameters,
                    &description.parameter_values,
                    conversions,
                    &pending.spec.params,
                )
                .map_err(|message| ImportError {
                    details: plan.source_detail(key, &message),
                    message,
                })?;
            } else if pending.has_automation {
                plugin.parameter_values = description.parameter_values.clone();
            }
            values.insert(key.clone(), plugin);
            for (index, automation) in &automations {
                if automation.use_key != *key {
                    continue;
                }
                let parameter =
                    parameters::find_parameter(&description.parameters, &automation.parameter)
                        .map_err(|message| ImportError {
                            details: plan
                                .source_detail(&format!("mix.automation[{index}]"), &message),
                            message,
                        })?;
                let parameter_id = parameter.parameter_id.clone().ok_or_else(|| {
                    let message = format!(
                        "plugin parameter {:?} has no stable identifier for automation",
                        parameter.name
                    );
                    ImportError {
                        details: plan.source_detail(&format!("mix.automation[{index}]"), &message),
                        message,
                    }
                })?;
                if !parameter.automatable {
                    let message =
                        format!("plugin parameter {:?} is not automatable", parameter.name);
                    return Err(ImportError {
                        details: plan.source_detail(&format!("mix.automation[{index}]"), &message),
                        message,
                    });
                }
                if parameter.discrete
                    && automation.interpolation != score::production::mix::Interpolation::Step
                {
                    let message = format!(
                        "plugin parameter {:?} is discrete and requires step interpolation",
                        parameter.name
                    );
                    return Err(ImportError {
                        details: plan.source_detail(&format!("mix.automation[{index}]"), &message),
                        message,
                    });
                }
                let mut points = Vec::with_capacity(automation.points.len());
                for (point_index, point) in automation.points.iter().enumerate() {
                    points.push(match point {
                        score::production::ParameterValue::Normalized(value) => *value,
                        score::production::ParameterValue::Display(display) => {
                            let request = automation.requests[point_index]
                                .expect("display values have a request");
                            f64::from(
                                parameters::automation_value(
                                    parameter,
                                    conversions,
                                    request,
                                    display.as_str(),
                                )
                                .map_err(|message| {
                                    ImportError {
                                        details: plan.source_detail(
                                            &format!(
                                                "mix.automation[{index}].points[{point_index}]"
                                            ),
                                            &message,
                                        ),
                                        message,
                                    }
                                })?,
                            )
                        }
                    });
                }
                resolved_automation.insert(
                    *index,
                    ResolvedAutomation {
                        parameter_id,
                        values: points,
                    },
                );
            }
        }
    }
    for (key, previous) in reused {
        values.insert(key, previous.plugin);
        resolved_automation.extend(previous.automation);
    }
    let mut production = BTreeMap::new();
    for (key, pending) in uses {
        let mut production_value = pending.production;
        if let Some(plugin) = values.remove(&key) {
            production_value.plugin_mut().plugin = plugin;
        }
        production.insert(key, production_value);
    }
    Ok((production, resolved_automation, production_inputs))
}

fn automation_parameter(
    lane: &score::production::mix::Automation,
    index: usize,
    plan: &ScorePlan,
) -> Result<String, ImportError> {
    let path = format!("mix.automation[{index}]");
    match &lane.target {
        score::production::mix::AutomationTarget::Effect { parameter, .. }
        | score::production::mix::AutomationTarget::Source { parameter, .. } => {
            Ok(parameter.clone())
        }
        _ => {
            let message = "automation target has no plugin parameter".to_owned();
            Err(ImportError {
                details: plan.source_detail(&path, &message),
                message,
            })
        }
    }
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
    /// Whether the resolved plugin inputs differ from the accepted fingerprint.
    pub(crate) inputs_changed: bool,
    /// Warnings produced while resolving the Score.
    pub(crate) diagnostics: Vec<ScoreDiagnostic>,
    dependencies: Dependencies,
    staged: StagedResources,
}

impl ScoreOutcome {
    /// Returns whether accepting this Score changes anything the Project stores.
    fn has_changes(&self) -> bool {
        self.changed || self.yaml_changed || self.link_changed || self.inputs_changed
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

    /// Reports whether a dependency changed while the candidate was prepared.
    pub(crate) fn source_changed(&self) -> bool {
        if let Some(source) = self.session.score_source.as_ref()
            && let Some(path) = source.path.as_deref()
            && std::fs::read_to_string(path).map_or(true, |text| text != source.yaml)
        {
            return true;
        }
        self.dependencies
            .iter()
            .any(|(path, expected)| fingerprint(path).map_or(true, |current| current != *expected))
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
    let mut document = read(ReadInput {
        path: &params.path,
        bindings: &params.bindings,
        explicit: &params.bindings.keys().cloned().collect(),
        data_root,
        catalog,
        scanner,
        sonalloy,
        previous: None,
    })?;
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = StagedResources::new(data_root);
    let session = build(&mut document, None, &store, &mut staged, BuildMode::Stage)?;
    Ok(ScoreOutcome {
        session,
        yaml_changed: true,
        changed: true,
        link_changed: true,
        inputs_changed: true,
        diagnostics: warnings(&document.plan),
        dependencies: std::mem::take(&mut document.dependencies),
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
    let mut document = read(ReadInput {
        path: &path,
        bindings: &effective,
        explicit: &bindings.keys().cloned().collect(),
        data_root,
        catalog,
        scanner,
        sonalloy,
        previous: Some(previous),
    })?;
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = StagedResources::new(data_root);
    let session = build(
        &mut document,
        Some(previous),
        &store,
        &mut staged,
        BuildMode::Stage,
    )?;
    let diagnostics = warnings(&document.plan);
    let dependencies = std::mem::take(&mut document.dependencies);
    let yaml_changed = previous
        .score_source
        .as_ref()
        .is_none_or(|source| source.yaml != document.yaml);
    let inputs_changed = previous
        .score_source
        .as_ref()
        .is_none_or(|source| source.production_inputs != document.production_inputs);
    Ok(ScoreOutcome {
        changed: !previous.score_performance_equals(&session),
        link_changed: link_changed(previous, &session),
        inputs_changed,
        session,
        yaml_changed,
        diagnostics,
        dependencies,
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
    let mut document = match read(ReadInput {
        path,
        bindings,
        explicit: &bindings.keys().cloned().collect(),
        data_root,
        catalog,
        scanner,
        sonalloy,
        previous: None,
    }) {
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
    let mut document = read(ReadInput {
        path,
        bindings,
        explicit: &bindings.keys().cloned().collect(),
        data_root,
        catalog,
        scanner,
        sonalloy,
        previous: None,
    })?;
    let store = UserInstrumentStore::new(data_root, sonalloy);
    let mut staged = StagedResources::new(data_root);
    let session = build(
        &mut document,
        None,
        &store,
        &mut staged,
        BuildMode::Validate,
    )?;
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
    let mut plugins_by_path = BTreeMap::new();
    for resolved in document.production.values() {
        let plugin = resolved.plugin();
        plugins_by_path
            .entry(plugin.plugin.path.clone())
            .or_insert(&plugin.entry);
    }
    let yaml_effect_ids = score_effect_identifiers(plan);
    let track_inspections = session
        .arrangement
        .tracks
        .iter()
        .filter_map(|track| {
            let (id, kind) = score_track_identity(plan, track)?;
            let (device_id, plugin_name, plugin_identifier) =
                track
                    .instrument
                    .as_ref()
                    .map_or((None, None, None), |instrument| {
                        (
                            Some(instrument.id.clone()),
                            Some(instrument.name.clone()),
                            instrument
                                .as_vst3()
                                .and_then(|plugin| plugins_by_path.get(&plugin.path))
                                .and_then(|entry| entry.identifier.clone()),
                        )
                    });
            let output = track.output_bus_id.as_deref().and_then(|id| {
                session
                    .arrangement
                    .mixer
                    .buses
                    .iter()
                    .find(|bus| bus.id == id)
                    .map(|bus| bus.name.clone())
            });
            Some(ScoreTrackInspection {
                id,
                kind: kind.into(),
                track_id: track.id.clone(),
                name: track.name.clone(),
                gain_db: track.gain_db,
                pan: track.pan,
                output,
                device_id,
                plugin_name,
                plugin_identifier,
                effect_ids: track
                    .effects
                    .iter()
                    .map(|effect| {
                        yaml_effect_ids
                            .get(&effect.id)
                            .cloned()
                            .unwrap_or_else(|| effect.id.clone())
                    })
                    .collect(),
                note_count: notes_on_track(&session, &track.id),
            })
        })
        .collect::<Vec<_>>();
    let kit_inspections =
        plan.kits
            .iter()
            .filter_map(|(id, kit)| {
                let score_key = format!("kit:{id}");
                let track =
                    session.arrangement.tracks.iter().find(|track| {
                        track.score_instrument.as_deref() == Some(score_key.as_str())
                    })?;
                let instrument = track.instrument.as_ref()?;
                let plugin = instrument.as_vst3();
                Some(ScoreKitInspection {
                    id: id.clone(),
                    track_id: track.id.clone(),
                    device_id: instrument.id.clone(),
                    plugin_name: instrument.name.clone(),
                    plugin_identifier: plugin
                        .and_then(|plugin| plugins_by_path.get(&plugin.path))
                        .and_then(|entry| entry.identifier.clone()),
                    members: plan
                        .kit_members
                        .iter()
                        .filter(|(_, kit_id)| *kit_id == id)
                        .map(|(member, _)| member.clone())
                        .collect(),
                    effect_ids: kit.effects.iter().map(|effect| effect.id.clone()).collect(),
                    note_count: notes_on_track(&session, &track.id),
                })
            })
            .collect::<Vec<_>>();
    let mut effect_inspections = Vec::new();
    let mut push_effects =
        |owner_kind: &str, owner_id: Option<String>, effects: &[EffectDevice]| {
            for effect in effects {
                effect_inspections.push(ScoreEffectInspection {
                    owner_kind: owner_kind.into(),
                    owner_id: owner_id.clone(),
                    id: yaml_effect_ids
                        .get(&effect.id)
                        .cloned()
                        .unwrap_or_else(|| effect.id.clone()),
                    plugin_name: effect.name.clone(),
                    plugin_identifier: plugins_by_path
                        .get(&effect.plugin.path)
                        .and_then(|entry| entry.identifier.clone()),
                    plugin_path: effect.plugin.path.clone(),
                    bypassed: effect.bypassed,
                });
            }
        };
    for track in &session.arrangement.tracks {
        if let Some((id, kind)) = score_track_identity(plan, track) {
            push_effects(kind, Some(id), &track.effects);
        }
    }
    for bus in &session.arrangement.mixer.buses {
        push_effects("bus", Some(bus.name.clone()), &bus.effects);
    }
    push_effects("master", None, &session.arrangement.mixer.master_effects);
    let automation = session
        .arrangement
        .automation_lanes
        .iter()
        .map(|lane| ScoreAutomationInspection {
            target: automation_target_label(plan, &session, &lane.target),
            interpolation: match lane.interpolation {
                AutomationInterpolation::Linear => "linear",
                AutomationInterpolation::Step => "step",
                AutomationInterpolation::LinearAmplitude { .. } => "linearAmplitude",
            }
            .into(),
            points: lane.points.len(),
        })
        .collect();
    Ok(ScoreInspection {
        tracks: track_inspections,
        kits: kit_inspections,
        effects: effect_inspections,
        automation,
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

fn score_track_identity(plan: &ScorePlan, track: &Track) -> Option<(String, &'static str)> {
    let score = track.score_instrument.as_deref()?;
    if let Some(kit) = score
        .strip_prefix("kit:")
        .filter(|kit| plan.kits.contains_key(*kit))
    {
        Some((kit.to_owned(), "kit"))
    } else if plan.audio_instruments.contains(score) {
        Some((score.to_owned(), "audio"))
    } else {
        Some((score.to_owned(), "instrument"))
    }
}

fn score_effect_identifiers(plan: &ScorePlan) -> BTreeMap<String, String> {
    let mut identifiers = BTreeMap::new();
    for (id, effects) in &plan.production_effects {
        for effect in effects {
            identifiers.insert(
                effect_device_id(&owner_instrument(id), &effect.id),
                effect.id.clone(),
            );
        }
    }
    for (id, kit) in &plan.kits {
        for effect in &kit.effects {
            identifiers.insert(
                effect_device_id(&owner_kit(id), &effect.id),
                effect.id.clone(),
            );
        }
    }
    if let Some(mix) = &plan.mix {
        for (id, bus) in &mix.buses {
            for effect in &bus.effects {
                identifiers.insert(
                    effect_device_id(&owner_bus(id), &effect.id),
                    effect.id.clone(),
                );
            }
        }
        for effect in &mix.master_effects {
            identifiers.insert(effect_device_id("master", &effect.id), effect.id.clone());
        }
    }
    identifiers
}

fn notes_on_track(session: &CreativeSession, track_id: &str) -> usize {
    session
        .arrangement
        .midi_clips
        .iter()
        .filter(|clip| clip.track_id == track_id)
        .map(|clip| clip.notes.len())
        .sum()
}

fn automation_target_label(
    plan: &ScorePlan,
    session: &CreativeSession,
    target: &AutomationTarget,
) -> String {
    let track_label = |track_id: &str| {
        session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .and_then(|track| score_track_identity(plan, track))
            .map_or_else(
                || format!("track:{track_id}"),
                |(id, kind)| format!("{kind}:{id}"),
            )
    };
    match target {
        AutomationTarget::TrackMix {
            track_id,
            parameter,
        } => format!(
            "{}:{}",
            track_label(track_id),
            match parameter {
                TrackMixParameter::Volume => "volume",
                TrackMixParameter::Pan => "pan",
            }
        ),
        AutomationTarget::BusMix { bus_id, parameter } => format!(
            "bus:{}:{}",
            session
                .arrangement
                .mixer
                .buses
                .iter()
                .find(|bus| bus.id == *bus_id)
                .map_or_else(|| bus_id.clone(), |bus| bus.name.clone()),
            match parameter {
                BusMixParameter::Gain => "gain",
                BusMixParameter::Pan => "pan",
            }
        ),
        AutomationTarget::MasterGain => "master:gain".to_owned(),
        AutomationTarget::SendLevel { send_id } => {
            let send = session
                .arrangement
                .mixer
                .sends
                .iter()
                .find(|send| send.id == *send_id);
            send.map_or_else(
                || format!("send:{send_id}"),
                |send| {
                    let bus = session
                        .arrangement
                        .mixer
                        .buses
                        .iter()
                        .find(|bus| bus.id == send.return_bus_id)
                        .map_or_else(|| send.return_bus_id.clone(), |bus| bus.name.clone());
                    format!("{}:send:{bus}", track_label(&send.track_id))
                },
            )
        }
        AutomationTarget::SidechainSendLevel { sidechain_send_id } => {
            let send = session
                .arrangement
                .mixer
                .sidechain_sends
                .iter()
                .find(|send| send.id == *sidechain_send_id);
            send.map_or_else(
                || format!("sidechain:{sidechain_send_id}"),
                |send| {
                    format!(
                        "{}:sidechain:{}",
                        track_label(&send.source_track_id),
                        send.target_device_id
                    )
                },
            )
        }
        AutomationTarget::DeviceParameter {
            device_id,
            parameter_id,
        } => format!("device:{device_id}:{parameter_id}"),
    }
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

struct ReadInput<'a> {
    path: &'a Path,
    bindings: &'a BTreeMap<String, PathBuf>,
    explicit: &'a BTreeSet<String>,
    data_root: &'a Path,
    catalog: &'a BuiltInInstrumentCatalog,
    scanner: &'a Path,
    sonalloy: &'a Path,
    previous: Option<&'a CreativeSession>,
}

fn read(input: ReadInput<'_>) -> Result<ScoreDocument, ImportError> {
    let ReadInput {
        path,
        bindings,
        explicit,
        data_root,
        catalog,
        scanner,
        sonalloy,
        previous,
    } = input;
    let yaml = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read score: {}: {error}", path.display()))?;
    let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let plan = score::compile(&yaml).map_err(score_error)?;
    let base = path.parent().expect("canonical file has a parent");
    let mut dependencies = Dependencies::new();
    record_dependency(&mut dependencies, &path).map_err(ImportError::from)?;
    validate_audio(&plan, base, &mut dependencies)?;
    let (production, automation, production_inputs) =
        resolve_production(&plan, base, data_root, scanner, &mut dependencies, previous)?;
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
        record_dependency(&mut dependencies, &canonical).map_err(|message| ImportError {
            details: plan.source_detail(&field, &message),
            message,
        })?;
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
        automation,
        production_inputs,
        dependencies,
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
    data_root: PathBuf,
    snapshots: BTreeSet<PathBuf>,
    assets: Vec<(AssetId, String)>,
}

impl StagedResources {
    fn new(data_root: &Path) -> Self {
        Self {
            data_root: data_root.to_path_buf(),
            snapshots: BTreeSet::new(),
            assets: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.snapshots.clear();
        self.assets.clear();
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.snapshots.is_empty() && self.assets.is_empty()
    }
}

impl Drop for StagedResources {
    fn drop(&mut self) {
        for snapshot in &self.snapshots {
            if let Err(error) = std::fs::remove_dir_all(snapshot) {
                tracing::warn!(%error, "score instrument snapshot cleanup failed");
            }
        }
        for (id, location) in &self.assets {
            if let Err(error) =
                riffra_host::discard_staged_audio_import(&self.data_root, id, location)
            {
                tracing::warn!(%error, %id, "score audio asset cleanup failed");
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
    let ScoreDocument {
        path,
        yaml,
        plan,
        bindings,
        sources,
        production,
        automation: resolved_automation,
        production_inputs: _,
        dependencies: _,
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
    let mut volume_tracks = Vec::new();
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
        let instrument = if let Some(ResolvedProduction::Source { plugin, .. }) = resolved {
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
        let track_id = track.id.clone();
        tracks.push(track);
        clips.push(clip);
        volume_tracks.push((index, track_id, claimed.map(|track| track.id.clone())));
    }
    for kit_id in plan.kits.keys() {
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
        if claimed.is_none() {
            track.name = kit_id.clone();
        }
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
        if claimed.is_none() {
            track.name = id.clone();
        }
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
    let structure = AutomationStructure {
        plan,
        resolved: resolved_automation,
        instrument_tracks: &instrument_tracks,
        kit_tracks: &kit_tracks,
        tracks: &tracks,
        mixer: &mixer,
    };
    let mut automation = build_automation(&structure, previous, &volume_tracks)?;
    let base = path.parent().expect("canonical file has a parent");
    let mut audio_clips =
        build_audio_clips(plan, base, previous, staged, mode, &instrument_tracks)?;

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
        production_inputs: document.production_inputs.clone(),
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
    {
        let track_ids = tracks
            .iter()
            .map(|track| track.id.clone())
            .collect::<BTreeSet<_>>();
        let group_ids = mixer
            .buses
            .iter()
            .filter(|bus| bus.kind == MixBusKind::Group)
            .map(|bus| bus.id.clone())
            .collect::<BTreeSet<_>>();
        for track in tracks
            .iter_mut()
            .filter(|track| track.score_instrument.is_none())
        {
            if track
                .output_bus_id
                .as_ref()
                .is_some_and(|id| !group_ids.contains(id))
            {
                track.output_bus_id = None;
            }
            track
                .external_audio_source_track_ids
                .retain(|id| track_ids.contains(id) && *id != track.id);
        }
    }
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
    audio_clips.extend(retained_audio_clips);
    session.arrangement.audio_clips = audio_clips;
    session.arrangement.automation_lanes = automation;
    session.arrangement.regions = regions;
    session.arrangement.harmony_events = harmony;
    session.arrangement.mixer = mixer;
    session.validate_and_normalize().map_err(ImportError::from)
}

/// Builds audio clips for every Score audio declaration and every section occurrence.
fn build_audio_clips(
    plan: &ScorePlan,
    base: &Path,
    previous: Option<&CreativeSession>,
    staged: &mut StagedResources,
    mode: BuildMode,
    instrument_tracks: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<AudioClip>, ImportError> {
    use num_traits::ToPrimitive;
    let mut clips = Vec::new();
    let mut imported = BTreeMap::<PathBuf, AssetId>::new();
    let mut imported_digest = BTreeMap::<Fingerprint, AssetId>::new();
    let previous_assets = previous
        .map(|session| {
            session
                .arrangement
                .audio_clips
                .iter()
                .map(|clip| clip.asset_id.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut previous_digests = BTreeMap::<AssetId, Option<Fingerprint>>::new();
    let mut asset_for_file = |file: &Path, name: &str| -> Result<AssetId, ImportError> {
        if let Some(asset_id) = imported.get(file) {
            return Ok(asset_id.clone());
        }
        let digest = if mode == BuildMode::Stage {
            Some(
                fingerprint(file)
                    .map_err(|error| format!("audio clip {}: {error}", file.display()))?,
            )
        } else {
            None
        };
        if let Some(digest) = &digest
            && let Some(asset_id) = imported_digest.get(digest)
        {
            imported.insert(file.to_path_buf(), asset_id.clone());
            return Ok(asset_id.clone());
        }
        if let Some(digest) = &digest {
            for asset_id in &previous_assets {
                let previous_digest =
                    previous_digests.entry(asset_id.clone()).or_insert_with(|| {
                        riffra_host::load(&staged.data_root, asset_id)
                            .ok()
                            .flatten()
                            .filter(|asset| asset.kind == AssetKind::Audio)
                            .and_then(|asset| fingerprint(Path::new(&asset.content_location)).ok())
                    });
                if previous_digest.as_ref() == Some(digest) {
                    imported.insert(file.to_path_buf(), asset_id.clone());
                    imported_digest.insert(digest.clone(), asset_id.clone());
                    return Ok(asset_id.clone());
                }
            }
        }
        if mode == BuildMode::Validate {
            return Ok(mint_asset_id());
        }
        let (asset_id, location) = riffra_host::stage_audio_import(&staged.data_root, file, name)?;
        staged.assets.push((asset_id.clone(), location));
        imported.insert(file.to_path_buf(), asset_id.clone());
        if let Some(digest) = digest {
            imported_digest.insert(digest, asset_id.clone());
        }
        Ok(asset_id)
    };
    for instrument in &plan.audio_instruments {
        let track_id = instrument_tracks
            .get(instrument)
            .and_then(|tracks| tracks.first())
            .ok_or_else(|| format!("audio instrument '{instrument}' has no track"))?
            .clone();
        for (section, specs) in &plan.audio_clips {
            let mut occurrence_number = 0;
            for occurrence in plan
                .sections
                .iter()
                .filter(|occurrence| occurrence.section == *section)
            {
                occurrence_number += 1;
                for spec in specs {
                    if spec.instrument != *instrument {
                        continue;
                    }
                    let id = format!(
                        "score:audio:{}:{}:{occurrence_number}:{}",
                        score_id_component(instrument),
                        score_id_component(section),
                        score_id_component(&spec.id)
                    );
                    let file = base.join(&spec.file);
                    let file = std::fs::canonicalize(&file).map_err(|error| {
                        format!(
                            "audio clip {} could not be resolved: {error}",
                            file.display()
                        )
                    })?;
                    let (rate, total) = riffra_host::read_wav_metadata(&file)
                        .map_err(|error| format!("{}: {error}", file.display()))?;
                    let frames = |seconds: f64| -> Result<u64, ImportError> {
                        (seconds * f64::from(rate)).round().to_u64().ok_or_else(|| {
                            ImportError::from(
                                "audio time exceeds the supported frame range".to_owned(),
                            )
                        })
                    };
                    let start = frames(spec.offset_seconds)?;
                    let duration = match spec.duration_seconds {
                        Some(seconds) => frames(seconds)?,
                        None => total.saturating_sub(start),
                    };
                    let name = format!("score-{instrument}-{}", spec.id);
                    let asset_id = asset_for_file(&file, &name)?;
                    clips.push(AudioClip {
                        id,
                        track_id: track_id.clone(),
                        asset_id,
                        start_tick: TimelineTick(plan.position_tick(occurrence, &spec.at)),
                        source_range: FrameRange {
                            start,
                            end: start + duration,
                        },
                        source_sample_rate: rate,
                        timeline_duration: FrameDuration {
                            frames: duration,
                            sample_rate: rate,
                        },
                        gain_db: spec.gain_db,
                        pan: spec.pan,
                        fade_in: FrameDuration {
                            frames: frames(spec.fade_in_seconds)?,
                            sample_rate: rate,
                        },
                        fade_out: FrameDuration {
                            frames: frames(spec.fade_out_seconds)?,
                            sample_rate: rate,
                        },
                        fade_shape: FadeShape::EqualPower,
                        loop_enabled: false,
                        muted: false,
                        name: spec.id.clone(),
                        recording_take_id: None,
                        take_variant: AudioTakeVariant::Raw,
                    });
                }
            }
        }
    }
    Ok(clips)
}

struct LaneAccumulator {
    target: AutomationTarget,
    interpolation: AutomationInterpolation,
    points: BTreeMap<u64, f64>,
    score_target: Option<String>,
}

fn automation_key(target: &AutomationTarget) -> String {
    match target {
        AutomationTarget::TrackMix {
            track_id,
            parameter,
        } => format!(
            "track:{track_id}:{}",
            match parameter {
                TrackMixParameter::Volume => "volume",
                TrackMixParameter::Pan => "pan",
            }
        ),
        AutomationTarget::BusMix { bus_id, parameter } => format!(
            "bus:{bus_id}:{}",
            match parameter {
                BusMixParameter::Gain => "gain",
                BusMixParameter::Pan => "pan",
            }
        ),
        AutomationTarget::MasterGain => "master:gain".to_owned(),
        AutomationTarget::SendLevel { send_id } => format!("send:{send_id}"),
        AutomationTarget::SidechainSendLevel { sidechain_send_id } => {
            format!("sidechain:{sidechain_send_id}")
        }
        AutomationTarget::DeviceParameter {
            device_id,
            parameter_id,
        } => format!("device:{device_id}:{parameter_id}"),
    }
}

/// Canonical structures that automation targets resolve against.
struct AutomationStructure<'a> {
    plan: &'a ScorePlan,
    resolved: &'a ResolvedAutomationMap,
    instrument_tracks: &'a BTreeMap<String, Vec<String>>,
    kit_tracks: &'a BTreeMap<String, String>,
    tracks: &'a [Track],
    mixer: &'a Mixer,
}

/// Maps one Score automation target to its canonical targets.
fn automation_targets(
    structure: &AutomationStructure<'_>,
    lane_index: usize,
    target: &score::production::mix::AutomationTarget,
) -> Result<Vec<AutomationTarget>, ImportError> {
    use score::production::mix::AutomationTarget as ScoreTarget;
    let plan = structure.plan;
    let resolved = structure.resolved;
    let instrument_tracks = structure.instrument_tracks;
    let kit_tracks = structure.kit_tracks;
    let tracks = structure.tracks;
    let mixer = structure.mixer;
    let path = format!("mix.automation[{lane_index}].target");
    let fail = |message: String| ImportError {
        details: plan.source_detail(&path, &message),
        message,
    };
    let track_ids = |owner: &score::production::mix::Owner| -> Option<Vec<String>> {
        match owner {
            score::production::mix::Owner::Instrument(id) => instrument_tracks.get(id).cloned(),
            score::production::mix::Owner::Kit(id) => {
                kit_tracks.get(id).map(|track| vec![track.clone()])
            }
            _ => None,
        }
    };
    let bus_id = |id: &str| {
        plan.mix
            .as_ref()
            .and_then(|mix| mix.buses.get(id))
            .map(|bus| canonical_bus_id(bus.kind, id))
            .or_else(|| {
                mixer
                    .buses
                    .iter()
                    .find(|bus| bus.name == id)
                    .map(|bus| bus.id.clone())
            })
    };
    let parameter_id = |index: usize| {
        resolved
            .get(&index)
            .map(|resolved| resolved.parameter_id.clone())
    };
    let targets = match target {
        ScoreTarget::Track { owner, parameter } => {
            let ids = track_ids(owner)
                .ok_or_else(|| fail("automation track owner is not defined".to_owned()))?;
            let parameter = match parameter.as_str() {
                "volume" => TrackMixParameter::Volume,
                "pan" => TrackMixParameter::Pan,
                _ => return Err(fail("undefined track automation parameter".to_owned())),
            };
            ids.into_iter()
                .map(|track_id| AutomationTarget::TrackMix {
                    track_id,
                    parameter,
                })
                .collect()
        }
        ScoreTarget::Bus { id, parameter } => {
            let bus_id =
                bus_id(id).ok_or_else(|| fail("automation bus is not defined".to_owned()))?;
            let parameter = match parameter.as_str() {
                "gain" => BusMixParameter::Gain,
                "pan" => BusMixParameter::Pan,
                _ => return Err(fail("undefined bus automation parameter".to_owned())),
            };
            vec![AutomationTarget::BusMix { bus_id, parameter }]
        }
        ScoreTarget::Master => vec![AutomationTarget::MasterGain],
        ScoreTarget::Send { instrument, bus } => {
            let track_ids = instrument_tracks
                .get(instrument)
                .cloned()
                .or_else(|| kit_tracks.get(instrument).map(|track| vec![track.clone()]))
                .ok_or_else(|| fail("automation send source is not defined".to_owned()))?;
            let bus_id =
                bus_id(bus).ok_or_else(|| fail("automation send bus is not defined".to_owned()))?;
            let matching = mixer
                .sends
                .iter()
                .filter(|send| track_ids.contains(&send.track_id) && send.return_bus_id == bus_id)
                .collect::<Vec<_>>();
            match matching.as_slice() {
                [send] => vec![AutomationTarget::SendLevel {
                    send_id: send.id.clone(),
                }],
                [] => return Err(fail("automation send is not defined".to_owned())),
                _ => return Err(fail("automation send is ambiguous".to_owned())),
            }
        }
        ScoreTarget::Sidechain { from, to } => {
            let ids = if let Some(track) = kit_tracks.get(from) {
                vec![track.clone()]
            } else {
                instrument_tracks
                    .get(from)
                    .ok_or_else(|| fail("automation sidechain source is not defined".to_owned()))?
                    .clone()
            };
            let target = effect_device_id_for(to);
            ids.iter()
                .filter_map(|track_id| {
                    mixer.sidechain_sends.iter().find(|send| {
                        send.source_track_id == *track_id && send.target_device_id == target
                    })
                })
                .map(|send| AutomationTarget::SidechainSendLevel {
                    sidechain_send_id: send.id.clone(),
                })
                .collect()
        }
        ScoreTarget::Effect { effect, .. } => {
            let parameter_id = parameter_id(lane_index)
                .ok_or_else(|| fail("plugin parameter was not resolved".to_owned()))?;
            vec![AutomationTarget::DeviceParameter {
                device_id: effect_device_id_for(effect),
                parameter_id,
            }]
        }
        ScoreTarget::Source { instrument, .. } => {
            let parameter_id = parameter_id(lane_index)
                .ok_or_else(|| fail("plugin parameter was not resolved".to_owned()))?;
            let ids = instrument_tracks
                .get(instrument)
                .ok_or_else(|| fail("automation source is not defined".to_owned()))?;
            ids.iter()
                .filter_map(|track_id| {
                    tracks
                        .iter()
                        .find(|track| track.id == *track_id)
                        .and_then(|track| track.instrument.as_ref())
                })
                .map(|instrument| AutomationTarget::DeviceParameter {
                    device_id: instrument.id.clone(),
                    parameter_id: parameter_id.clone(),
                })
                .collect()
        }
    };
    Ok(targets)
}

fn effect_device_id_for(effect: &score::production::mix::EffectRef) -> String {
    match &effect.owner {
        score::production::mix::Owner::Instrument(id) => {
            effect_device_id(&owner_instrument(id), &effect.id)
        }
        score::production::mix::Owner::Kit(id) => effect_device_id(&owner_kit(id), &effect.id),
        score::production::mix::Owner::Bus(id) => effect_device_id(&owner_bus(id), &effect.id),
        score::production::mix::Owner::Master => effect_device_id("master", &effect.id),
    }
}

/// Builds explicit Score automation and the section gain lanes it replaces.
fn build_automation(
    structure: &AutomationStructure<'_>,
    previous: Option<&CreativeSession>,
    volume_tracks: &[(usize, String, Option<String>)],
) -> Result<Vec<AutomationLane>, ImportError> {
    let plan = structure.plan;
    let resolved = structure.resolved;
    let mut lanes = BTreeMap::<String, LaneAccumulator>::new();
    if let Some(mix) = &plan.mix {
        for (index, automation) in mix.automation.iter().enumerate() {
            let targets = automation_targets(structure, index, &automation.target)?;
            let score_target = match automation_use_key(plan, &automation.target) {
                Some(use_key) => Some(automation_score_target(
                    &use_key,
                    &automation_parameter(automation, index, plan)?,
                )),
                None => None,
            };
            let values = resolved
                .get(&index)
                .map(|resolved| resolved.values.as_slice());
            let interpolation = match automation.interpolation {
                score::production::mix::Interpolation::Linear => AutomationInterpolation::Linear,
                score::production::mix::Interpolation::Step => AutomationInterpolation::Step,
            };
            for (occurrence_index, occurrence) in plan
                .sections
                .iter()
                .filter(|occurrence| occurrence.section == automation.section)
                .enumerate()
            {
                if automation
                    .occurrence
                    .is_some_and(|number| number != occurrence_index + 1)
                {
                    continue;
                }
                for (point_index, point) in automation.points.iter().enumerate() {
                    let tick = plan.position_tick(occurrence, &point.at);
                    let value = match values {
                        Some(values) => *values.get(point_index).ok_or_else(|| ImportError {
                            details: plan.source_detail(
                                &format!("mix.automation[{index}].points[{point_index}]"),
                                "automation value was not resolved",
                            ),
                            message: "automation value was not resolved".to_owned(),
                        })?,
                        None => match &point.value {
                            score::production::ParameterValue::Normalized(value) => *value,
                            score::production::ParameterValue::Display(_) => {
                                let message =
                                    "display values require a plugin parameter target".to_owned();
                                return Err(ImportError {
                                    details: plan.source_detail(
                                        &format!("mix.automation[{index}].points[{point_index}]"),
                                        &message,
                                    ),
                                    message,
                                });
                            }
                        },
                    };
                    for target in &targets {
                        let key = automation_key(target);
                        let entry = lanes.entry(key).or_insert_with(|| LaneAccumulator {
                            target: target.clone(),
                            interpolation,
                            points: BTreeMap::new(),
                            score_target: score_target.clone(),
                        });
                        if entry.score_target.is_none() {
                            entry.score_target = score_target.clone();
                        }
                        if entry.interpolation != interpolation {
                            return Err(ImportError {
                                details: plan.source_detail(
                                    &format!("mix.automation[{index}].interpolation"),
                                    "automation for the same target must use one interpolation",
                                ),
                                message:
                                    "automation for the same target must use one interpolation"
                                        .to_owned(),
                            });
                        }
                        if let Some(previous) = entry.points.insert(tick, value)
                            && previous.to_bits() != value.to_bits()
                        {
                            let message =
                                "conflicting automation values at the same tick".to_owned();
                            return Err(ImportError {
                                details: plan.source_detail(
                                    &format!("mix.automation[{index}].points[{point_index}]"),
                                    &message,
                                ),
                                message,
                            });
                        }
                    }
                }
            }
        }
    }
    let mut result = Vec::with_capacity(lanes.len() + volume_tracks.len());
    for (key, accumulator) in lanes {
        let previous_lane = previous.and_then(|session| {
            session
                .arrangement
                .automation_lanes
                .iter()
                .find(|lane| lane.target == accumulator.target)
        });
        let lane_id =
            previous_lane.map_or_else(|| format!("score:automation:{key}"), |lane| lane.id.clone());
        let points = accumulator
            .points
            .into_iter()
            .map(|(tick, value)| AutomationPoint {
                id: previous_lane
                    .and_then(|lane| lane.points.iter().find(|point| point.tick.0 == tick))
                    .map_or_else(|| format!("{lane_id}:{tick}"), |point| point.id.clone()),
                tick: TimelineTick(tick),
                value,
            })
            .collect();
        result.push(AutomationLane {
            id: lane_id,
            target: accumulator.target,
            interpolation: accumulator.interpolation,
            points,
            score_target: accumulator.score_target,
        });
    }
    let explicit_volume = result
        .iter()
        .filter_map(|lane| match &lane.target {
            AutomationTarget::TrackMix {
                track_id,
                parameter: TrackMixParameter::Volume,
            } => Some(track_id.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for (lane_index, track_id, claimed_id) in volume_tracks {
        if explicit_volume.contains(track_id.as_str()) {
            continue;
        }
        let lane = &plan.lanes[*lane_index];
        let claimed = claimed_id.as_deref().and_then(|id| {
            previous.and_then(|session| {
                session
                    .arrangement
                    .tracks
                    .iter()
                    .find(|track| track.id == id)
            })
        });
        result.push(auto_volume_lane(plan, previous, claimed, lane, track_id));
    }
    Ok(result)
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
        score_target: None,
    }
}

fn score_id_component(value: &str) -> String {
    value.replace('%', "%25").replace(':', "%3A")
}

fn canonical_bus_id(kind: score::production::mix::BusKind, id: &str) -> String {
    format!(
        "score:bus:{}:{}",
        match kind {
            score::production::mix::BusKind::Group => "group",
            score::production::mix::BusKind::Return => "return",
        },
        score_id_component(id)
    )
}

fn owner_instrument(id: &str) -> String {
    format!("instrument:{}", score_id_component(id))
}

fn owner_kit(id: &str) -> String {
    format!("kit:{}", score_id_component(id))
}

fn owner_bus(id: &str) -> String {
    format!("bus:{}", score_id_component(id))
}

fn effect_device_id(owner: &str, id: &str) -> String {
    format!("score:effect:{owner}:{}", score_id_component(id))
}

/// Builds one ordered effect chain from resolved production effects.
fn effect_chain(
    plan: &ScorePlan,
    production: &ResolvedProductionMap,
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
    production: &ResolvedProductionMap,
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
    production: &ResolvedProductionMap,
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
        let canonical = canonical_bus_id(bus.kind, id);
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
        let Some((score_id, _)) = score_track_identity(plan, track) else {
            continue;
        };
        let Some(track_mix) = mix.tracks.get(&score_id) else {
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
                && track.kind == TrackKind::Instrument
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
                    && track.kind == TrackKind::Instrument
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
            automation: BTreeMap::new(),
            production_inputs: BTreeMap::new(),
            dependencies: Dependencies::new(),
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
            automation: BTreeMap::new(),
            production_inputs: BTreeMap::new(),
            dependencies: Dependencies::new(),
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
        assert_eq!(inspection.tracks.len(), 1);
        assert_eq!(inspection.tracks[0].id, "lead");
        assert_eq!(inspection.tracks[0].kind, "instrument");
        assert_eq!(inspection.tracks[0].note_count, 4);
        assert_eq!(
            inspection.tracks[0].plugin_identifier.as_deref(),
            Some("class-id")
        );
        assert!(!inspection.automation.is_empty());
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
    fn velocity_maps_interpolate_and_keep_the_endpoints() {
        let map = [(1, 18), (64, 68), (127, 120)];

        assert_eq!(mapped_velocity(None, 100), 100);
        assert_eq!(mapped_velocity(Some(&map), 1), 18);
        assert_eq!(mapped_velocity(Some(&map), 32), 43);
        assert_eq!(mapped_velocity(Some(&map), 64), 68);
        assert_eq!(mapped_velocity(Some(&map), 96), 94);
        assert_eq!(mapped_velocity(Some(&map), 127), 120);
    }

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

        let mut again = production_document(PRODUCTION);
        let rebuilt = build(
            &mut again,
            Some(&session),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
        .expect("no-op production rebuild");
        assert!(session.score_performance_equals(&rebuilt));
        let _ = std::fs::remove_dir_all(root);
    }

    const KIT_SEND: &str = r#"{
        "meta":{"title":"Kit Send","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"verse","bars":1,"tonic":"C","scale":"ionian"}],
        "instruments":{
            "kit:lead":{"source":{"vst3":{"plugin":"Lead"}}},
            "kick":{"kit":"drums","trigger_note_midi":36}
        },
        "kits":{"drums":{"source":{"vst3":{"plugin":"Drums"}}}},
        "drums":{"beat":{"parts":{"kick":"x...|....|....|...."}}},
        "sections":{"verse":{"drums":"beat","lines":{"lead":{"instrument":"kit:lead","melody":"theme"}}}},
        "melodies":{"theme":["1 - - - | 3 - - - | 5 - - - | 1' - - -"]},
        "mix":{
            "tracks":{
                "kit:lead":{"output":"rhythm","sends":{"room":-12}},
                "drums":{"gain_db":-2,"output":"rhythm","sends":{"room":-14}}
            },
            "buses":{"rhythm":{"kind":"group"},"room":{"kind":"return"}},
            "automation":[
                {"target":{"send":{"instrument":"drums","bus":"room"}},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":-20}]},
                {"target":{"send":{"instrument":"kit:lead","bus":"room"}},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":-18}]}
            ]
        }
    }"#;

    #[test]
    fn kit_and_instrument_ids_keep_their_own_mix_and_send_automation() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-kit-send-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let mut document = production_document(KIT_SEND);
        let mut staged = StagedResources::new(&root);
        let session = build(&mut document, None, &store, &mut staged, BuildMode::Stage)
            .expect("kit send build");

        let lead = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("kit:lead"))
            .expect("lead track");
        let drums = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("kit:drums"))
            .expect("drums track");
        assert_eq!(
            score_track_identity(&document.plan, lead),
            Some(("kit:lead".into(), "instrument"))
        );
        assert_eq!(
            score_track_identity(&document.plan, drums),
            Some(("drums".into(), "kit"))
        );
        assert_eq!(
            lead.output_bus_id.as_deref(),
            Some("score:bus:group:rhythm")
        );
        assert_eq!(
            drums.output_bus_id.as_deref(),
            Some("score:bus:group:rhythm")
        );
        assert_eq!(lead.gain_db, 0.0);
        assert_eq!(drums.gain_db, -2.0);

        let send = |track_id: &str, level_db: f64| {
            session
                .arrangement
                .mixer
                .sends
                .iter()
                .find(|send| {
                    send.track_id == track_id
                        && send.return_bus_id == "score:bus:return:room"
                        && send.level_db == level_db
                })
                .unwrap_or_else(|| panic!("missing send for {track_id} at {level_db}"))
                .id
                .clone()
        };
        let lead_send = send(&lead.id, -12.0);
        let drums_send = send(&drums.id, -14.0);
        for (send_id, value) in [(&drums_send, -20.0), (&lead_send, -18.0)] {
            assert!(session.arrangement.automation_lanes.iter().any(|lane| {
                lane.target
                    == AutomationTarget::SendLevel {
                        send_id: (*send_id).clone(),
                    }
                    && lane.points[0].value == value
            }));
        }

        let track_ids = session
            .arrangement
            .tracks
            .iter()
            .map(|track| track.id.clone())
            .collect::<Vec<_>>();
        let mut again = production_document(KIT_SEND);
        let rebuilt = build(
            &mut again,
            Some(&session),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
        .expect("kit send rebuild");
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
                .automation_lanes
                .iter()
                .map(|lane| lane.id.clone())
                .collect::<Vec<_>>(),
            session
                .arrangement
                .automation_lanes
                .iter()
                .map(|lane| lane.id.clone())
                .collect::<Vec<_>>()
        );
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
            .replace(r#""drums":{"gain_db":-2"#, r#""drums":{"gain_db":-4"#)
            .replace(r#""master":{"gain_db":-1"#, r#""master":{"gain_db":-3"#);
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
        assert_eq!(rebuilt.settings.master_db, -3.0);
        assert!(rebuilt.score_performance_equals(&rebuilt));
        assert!(!session.score_performance_equals(&rebuilt));
        let _ = std::fs::remove_dir_all(root);
    }

    fn one_sample_wav() -> Vec<u8> {
        let mut wav = Vec::from(&b"RIFF"[..]);
        wav.extend_from_slice(&40_u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&44_100_u32.to_le_bytes());
        wav.extend_from_slice(&88_200_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&2_u32.to_le_bytes());
        wav.extend_from_slice(&[0, 0]);
        wav
    }

    fn with_audio_clip(score: &str) -> String {
        score
            .replace(
                r#""ambience":{"audio":true,"gain_db":-5}"#,
                r#""ambience":{"audio":true,"gain_db":-5},"ambience2":{"audio":true,"gain_db":-8}"#,
            )
            .replace(
                r#""sections":{"verse":{"drums":"beat","lines":{"main":{"instrument":"bass","melody":"theme"}}}}"#,
                r#""sections":{"verse":{"drums":"beat","lines":{"main":{"instrument":"bass","melody":"theme"}},"audio_clips":[{"id":"rain","instrument":"ambience","file":"audio/rain.wav","at":{"bar":1,"step":1}},{"id":"thunder","instrument":"ambience2","file":"audio/thunder.wav","at":{"bar":1,"step":1}}]}}"#,
            )
    }

    const INPUT_REUSE: &str = r#"{
        "meta":{"title":"Input Reuse","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"verse","bars":1,"tonic":"C","scale":"ionian"}],
        "instruments":{
            "bass":{"source":{"vst3":{"plugin":"Bass","state":"state.json","params":{"Cutoff":0.5}}}}
        },
        "sections":{"verse":{"lines":{"main":{"instrument":"bass","melody":"theme"}}}},
        "melodies":{"theme":["1 - - - | 3 - - - | 5 - - - | 1' - - -"]},
        "mix":{"automation":[{"target":{"source":"bass","parameter":"Cutoff"},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":0.2}]}]}
    }"#;

    #[test]
    fn unchanged_plugin_inputs_reuse_the_canonical_state_without_a_probe() {
        use crate::api::output::{
            PluginEntry, PluginFormat, PluginRole, PluginScanState, ScanReport,
        };

        let root = std::env::temp_dir().join(format!(
            "riffra-score-input-reuse-{}",
            riffra_control::new_instance_id()
        ));
        let score_dir = root.join("music/song");
        std::fs::create_dir_all(&score_dir).unwrap();
        let plugin_path = root.join("Bass.vst3");
        std::fs::create_dir_all(&plugin_path).unwrap();
        crate::plugins::catalog::save(
            &root,
            &ScanReport {
                root: root.to_string_lossy().into_owned(),
                started_at_ms: 0,
                finished_at_ms: 0,
                issues: Vec::new(),
                plugins: vec![PluginEntry {
                    id: "bass".into(),
                    name: "Bass".into(),
                    vendor: None,
                    version: None,
                    identifier: Some("bass-class".into()),
                    format: PluginFormat::Vst3,
                    role: Some(PluginRole::Instrument),
                    path: plugin_path.to_string_lossy().into_owned(),
                    bundle: true,
                    modified_at_ms: None,
                    scan_state: PluginScanState::Validated,
                }],
            },
        )
        .unwrap();
        std::fs::write(
            score_dir.join("state.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 2,
                "pluginIdentifier": "bass-class",
                "pluginPath": plugin_path.to_string_lossy(),
                "parameterValues": [0.1],
                "stateData": "c3RhdGU="
            }))
            .unwrap(),
        )
        .unwrap();
        let score_path = score_dir.join("score.yaml");
        std::fs::write(&score_path, INPUT_REUSE).unwrap();

        let plan = score::compile(INPUT_REUSE).unwrap();
        let spec = plan.production_sources["bass"]
            .vst3
            .as_ref()
            .expect("inline vst3");
        let mut plugin = Vst3Plugin::new(plugin_path.to_string_lossy().into_owned());
        plugin.parameter_values = vec![0.1];
        plugin.state_data = Some("c3RhdGU=".into());
        let lane = &plan.mix.as_ref().unwrap().automation[0];
        let pending = PendingAutomation {
            use_key: "instruments.bass.source".into(),
            parameter: "Cutoff".into(),
            interpolation: lane.interpolation,
            points: vec![score::production::ParameterValue::Normalized(0.2)],
            requests: vec![None],
        };
        let fingerprint = production_input_fingerprint(spec, &plugin, &[&pending]);
        let mut previous = CreativeSession::new(1);
        let mut track = Track::instrument("score:track:1".into(), "bass".into());
        let mut instrument = TrackInstrument::vst3(
            "score:device:1".into(),
            "Bass".into(),
            plugin_path.to_string_lossy().into_owned(),
        )
        .unwrap();
        *instrument.as_vst3_mut().unwrap() = plugin;
        track.instrument = Some(instrument);
        track.score_instrument = Some("bass".into());
        previous.arrangement.tracks.push(track);
        previous.arrangement.automation_lanes.push(AutomationLane {
            id: "score:automation:test".into(),
            target: AutomationTarget::DeviceParameter {
                device_id: "score:device:1".into(),
                parameter_id: "cutoff-id".into(),
            },
            interpolation: AutomationInterpolation::Linear,
            points: vec![AutomationPoint {
                id: "point:test".into(),
                tick: TimelineTick(0),
                value: 0.2,
            }],
            score_target: Some("instruments.bass.source|Cutoff".into()),
        });
        previous.score_source = Some(ScoreSource {
            file_name: "score.yaml".into(),
            yaml: INPUT_REUSE.into(),
            path: Some(score_path.to_string_lossy().into_owned()),
            bindings: BTreeMap::new(),
            production_inputs: BTreeMap::from([(
                "instruments.bass.source".into(),
                fingerprint.clone(),
            )]),
        });

        let bindings = BTreeMap::new();
        let explicit = BTreeSet::new();
        let catalog = crate::test_support::empty_built_in_catalog();
        let document = read(ReadInput {
            path: &score_path,
            bindings: &bindings,
            explicit: &explicit,
            data_root: &root,
            catalog,
            scanner: Path::new("unavailable-scanner"),
            sonalloy: Path::new("unavailable-sonalloy"),
            previous: Some(&previous),
        })
        .expect("unchanged inputs reuse the previous plugin without probing");
        assert_eq!(
            document.production_inputs.get("instruments.bass.source"),
            Some(&fingerprint)
        );
        assert_eq!(document.automation[&0].parameter_id, "cutoff-id");
        assert_eq!(document.automation[&0].values, [0.2]);

        let changed = INPUT_REUSE.replace("\"Cutoff\":0.5", "\"Cutoff\":0.6");
        std::fs::write(&score_path, &changed).unwrap();
        let error = match read(ReadInput {
            path: &score_path,
            bindings: &bindings,
            explicit: &explicit,
            data_root: &root,
            catalog,
            scanner: Path::new("unavailable-scanner"),
            sonalloy: Path::new("unavailable-sonalloy"),
            previous: Some(&previous),
        }) {
            Ok(_) => panic!("changed params must probe the plugin"),
            Err(error) => error,
        };
        assert!(
            error.message.contains("scanner"),
            "unexpected error: {}",
            error.message
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn production_build_imports_audio_clips_and_reuses_unchanged_assets() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-audio-{}",
            riffra_control::new_instance_id()
        ));
        let score_dir = root.join("music/song");
        std::fs::create_dir_all(score_dir.join("audio")).unwrap();
        std::fs::write(score_dir.join("audio/rain.wav"), one_sample_wav()).unwrap();
        let mut thunder = one_sample_wav();
        thunder.extend_from_slice(&[0, 0]);
        let thunder_len = thunder.len() as u32;
        thunder[40..44].copy_from_slice(&(thunder_len - 44).to_le_bytes());
        thunder[4..8].copy_from_slice(&(thunder_len - 8).to_le_bytes());
        std::fs::write(score_dir.join("audio/thunder.wav"), thunder).unwrap();
        let score_path = score_dir.join("score.yaml");
        let store = store(&root);
        let yaml = with_audio_clip(PRODUCTION);
        let mut document = production_document(&yaml);
        document.path = score_path.clone();
        let mut staged = StagedResources::new(&root);
        let session =
            build(&mut document, None, &store, &mut staged, BuildMode::Stage).expect("audio build");

        assert_eq!(session.arrangement.audio_clips.len(), 2);
        let rain = session
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.name == "rain")
            .expect("rain clip");
        let thunder = session
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.name == "thunder")
            .expect("thunder clip");
        let ambience = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.name == "ambience")
            .expect("ambience track");
        let ambience2 = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.name == "ambience2")
            .expect("ambience2 track");
        assert_eq!(rain.track_id, ambience.id);
        assert_eq!(thunder.track_id, ambience2.id);
        assert_eq!(rain.start_tick.0, 0);
        assert_eq!(rain.source_sample_rate, 44_100);
        assert_eq!(rain.source_range.start, 0);
        assert_eq!(rain.source_range.end, 1);
        assert_eq!(rain.timeline_duration.frames, 1);
        assert_eq!(thunder.timeline_duration.frames, 2);
        assert_eq!(staged.assets.len(), 2);
        let rain_asset = rain.asset_id.clone();
        let thunder_asset = thunder.asset_id.clone();
        assert!(riffra_host::load(&root, &rain_asset).unwrap().is_some());
        assert!(riffra_host::load(&root, &thunder_asset).unwrap().is_some());

        let mut second = production_document(&yaml);
        second.path = score_path.clone();
        let mut unchanged = StagedResources::new(&root);
        let rebuilt = build(
            &mut second,
            Some(&session),
            &store,
            &mut unchanged,
            BuildMode::Stage,
        )
        .expect("audio rebuild");
        assert!(unchanged.assets.is_empty());
        let rebuilt_rain = rebuilt
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.name == "rain")
            .expect("rain rebuild");
        assert_eq!(rebuilt_rain.id, rain.id);
        assert_eq!(rebuilt_rain.asset_id, rain_asset);
        let rebuilt_thunder = rebuilt
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.name == "thunder")
            .expect("thunder rebuild");
        assert_eq!(rebuilt_thunder.asset_id, thunder_asset);

        let renamed_yaml = yaml.replace("\"id\":\"rain\"", "\"id\":\"rain-renamed\"");
        assert_ne!(renamed_yaml, yaml);
        let mut renamed = production_document(&renamed_yaml);
        renamed.path = score_path.clone();
        let mut renamed_staged = StagedResources::new(&root);
        let rebuilt = build(
            &mut renamed,
            Some(&rebuilt),
            &store,
            &mut renamed_staged,
            BuildMode::Stage,
        )
        .expect("renamed clip rebuild");
        assert!(
            renamed_staged.assets.is_empty(),
            "a renamed clip reuses the existing content asset"
        );
        let renamed_rain = rebuilt
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.name == "rain-renamed")
            .expect("renamed rain");
        assert_ne!(renamed_rain.id, rain.id);
        assert_eq!(renamed_rain.asset_id, rain_asset);

        let mut changed_wav = one_sample_wav();
        changed_wav.extend_from_slice(&[1, 0]);
        let changed_data = changed_wav.len() as u32 - 44;
        changed_wav[40..44].copy_from_slice(&changed_data.to_le_bytes());
        let riff_len = changed_wav.len() as u32 - 8;
        changed_wav[4..8].copy_from_slice(&riff_len.to_le_bytes());
        std::fs::write(score_dir.join("audio/rain.wav"), changed_wav).unwrap();
        let mut third = production_document(&yaml);
        third.path = score_path.clone();
        let mut replaced = StagedResources::new(&root);
        let rebuilt = build(
            &mut third,
            Some(&rebuilt),
            &store,
            &mut replaced,
            BuildMode::Stage,
        )
        .expect("changed audio rebuild");
        assert_eq!(replaced.assets.len(), 1);
        let replaced_rain = rebuilt
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.name == "rain")
            .expect("changed rain");
        assert_eq!(replaced_rain.id, rain.id);
        assert_ne!(replaced_rain.asset_id, rain_asset);
        let replaced_thunder = rebuilt
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.name == "thunder")
            .expect("unchanged thunder");
        assert_eq!(replaced_thunder.asset_id, thunder_asset);
        assert!(riffra_host::load(&root, &rain_asset).unwrap().is_some());

        let mut validated = production_document(&yaml);
        validated.path = score_path;
        let mut dry = StagedResources::new(&root);
        build(&mut validated, None, &store, &mut dry, BuildMode::Validate)
            .expect("audio validation");
        assert!(dry.assets.is_empty(), "validation stages no asset");

        drop(replaced);
        drop(unchanged);
        drop(dry);
        drop(staged);
        let _ = std::fs::remove_dir_all(root);
    }

    fn with_automation(score: &str) -> String {
        score.replace(
            r#""master":{"gain_db":-1,"effects":[{"id":"limiter","vst3":{"plugin":"Limiter"}}]}"#,
            r#""master":{"gain_db":-1,"effects":[{"id":"limiter","vst3":{"plugin":"Limiter"}}]},
            "automation":[
                {"target":{"instrument":"bass","parameter":"volume"},"section":"verse","interpolation":"linear","points":[{"at":{"bar":1,"step":1},"value":-3},{"at":{"bar":1,"step":5},"value":-8}]},
                {"target":{"bus":"rhythm","parameter":"gain"},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":-2}]},
                {"target":{"master":true,"parameter":"gain"},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":-6}]},
                {"target":{"send":{"instrument":"bass","bus":"room"}},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":-20}]},
                {"target":{"sidechain":{"from":"drums","to":{"instrument":"bass","effect":"duck"}}},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":-4}]}
            ]"#,
        )
    }

    #[test]
    fn production_build_generates_explicit_mix_automation() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-automation-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let yaml = with_automation(PRODUCTION);
        let mut document = production_document(&yaml);
        let mut staged = StagedResources::new(&root);
        let session = build(&mut document, None, &store, &mut staged, BuildMode::Stage)
            .expect("automation build");

        let bass_track = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("bass"))
            .expect("bass track");
        let volume = session
            .arrangement
            .automation_lanes
            .iter()
            .filter(|lane| {
                lane.target
                    == AutomationTarget::TrackMix {
                        track_id: bass_track.id.clone(),
                        parameter: TrackMixParameter::Volume,
                    }
            })
            .collect::<Vec<_>>();
        assert_eq!(volume.len(), 1);
        assert_eq!(volume[0].interpolation, AutomationInterpolation::Linear);
        assert_eq!(
            volume[0]
                .points
                .iter()
                .map(|point| (point.tick.0, point.value))
                .collect::<Vec<_>>(),
            [(0, -3.0), (u64::from(document.plan.ticks_per_beat), -8.0)]
        );
        assert!(session.arrangement.automation_lanes.iter().any(|lane| {
            lane.target == AutomationTarget::MasterGain && lane.points[0].value == -6.0
        }));
        let rhythm = session
            .arrangement
            .mixer
            .buses
            .iter()
            .find(|bus| bus.name == "rhythm")
            .expect("rhythm bus");
        assert!(session.arrangement.automation_lanes.iter().any(|lane| {
            lane.target
                == AutomationTarget::BusMix {
                    bus_id: rhythm.id.clone(),
                    parameter: BusMixParameter::Gain,
                }
        }));
        assert!(session.arrangement.automation_lanes.iter().any(|lane| {
            lane.target
                == AutomationTarget::SendLevel {
                    send_id: session.arrangement.mixer.sends[0].id.clone(),
                }
        }));
        assert!(session.arrangement.automation_lanes.iter().any(|lane| {
            lane.target
                == AutomationTarget::SidechainSendLevel {
                    sidechain_send_id: session.arrangement.mixer.sidechain_sends[0].id.clone(),
                }
        }));

        let lane_id = volume[0].id.clone();
        let point_ids = volume[0]
            .points
            .iter()
            .map(|point| point.id.clone())
            .collect::<Vec<_>>();
        let mut second = production_document(&yaml.replace(
            "[{\"at\":{\"bar\":1,\"step\":1},\"value\":-3}",
            "[{\"at\":{\"bar\":1,\"step\":1},\"value\":-5}",
        ));
        let rebuilt = build(
            &mut second,
            Some(&session),
            &store,
            &mut staged,
            BuildMode::Stage,
        )
        .expect("automation rebuild");
        let changed = rebuilt
            .arrangement
            .automation_lanes
            .iter()
            .find(|lane| lane.id == lane_id)
            .expect("automation lane identity");
        assert_eq!(
            changed
                .points
                .iter()
                .map(|point| point.id.clone())
                .collect::<Vec<_>>(),
            point_ids
        );
        assert_eq!(changed.points[0].value, -5.0);
        assert!(!session.score_performance_equals(&rebuilt));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn device_automation_targets_the_canonical_instrument_and_effect() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-device-automation-{}",
            riffra_control::new_instance_id()
        ));
        let store = store(&root);
        let mut base = production_document(PRODUCTION);
        let mut staged = StagedResources::new(&root);
        let session =
            build(&mut base, None, &store, &mut staged, BuildMode::Stage).expect("base build");
        let yaml = PRODUCTION.replace(
            r#""master":{"gain_db":-1,"effects":[{"id":"limiter","vst3":{"plugin":"Limiter"}}]}"#,
            r#""master":{"gain_db":-1,"effects":[{"id":"limiter","vst3":{"plugin":"Limiter"}}]},
            "automation":[
                {"target":{"source":"bass","parameter":"Cutoff"},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":0.1}]},
                {"target":{"effect":{"instrument":"bass","id":"duck"},"parameter":"Mix"},"section":"verse","interpolation":"linear","points":[{"at":{"bar":1,"step":1},"value":0.2},{"at":{"bar":1,"step":5},"value":0.8}]}
            ]"#,
        );
        let document = production_document(&yaml);
        let mut instrument_tracks = BTreeMap::<String, Vec<String>>::new();
        let mut kit_tracks = BTreeMap::<String, String>::new();
        for track in &session.arrangement.tracks {
            let Some(id) = track.score_instrument.as_deref() else {
                continue;
            };
            if let Some(kit) = id
                .strip_prefix("kit:")
                .filter(|kit| document.plan.kits.contains_key(*kit))
            {
                kit_tracks.insert(kit.to_owned(), track.id.clone());
            } else {
                instrument_tracks
                    .entry(id.to_owned())
                    .or_default()
                    .push(track.id.clone());
            }
        }
        let bass_track = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("bass"))
            .expect("bass track");
        let bass_device = bass_track
            .instrument
            .as_ref()
            .expect("bass instrument")
            .id
            .clone();
        let mut resolved = BTreeMap::new();
        resolved.insert(
            0,
            ResolvedAutomation {
                parameter_id: "bass-cutoff".into(),
                values: vec![0.1],
            },
        );
        resolved.insert(
            1,
            ResolvedAutomation {
                parameter_id: "duck-mix".into(),
                values: vec![0.2, 0.8],
            },
        );
        let structure = AutomationStructure {
            plan: &document.plan,
            resolved: &resolved,
            instrument_tracks: &instrument_tracks,
            kit_tracks: &kit_tracks,
            tracks: &session.arrangement.tracks,
            mixer: &session.arrangement.mixer,
        };
        let lanes = build_automation(&structure, None, &[]).unwrap();
        assert!(lanes.iter().any(|lane| {
            lane.target
                == AutomationTarget::DeviceParameter {
                    device_id: bass_device.clone(),
                    parameter_id: "bass-cutoff".into(),
                }
                && lane.points[0].value == 0.1
        }));
        assert!(lanes.iter().any(|lane| {
            lane.target
                == AutomationTarget::DeviceParameter {
                    device_id: "score:effect:instrument:bass:duck".into(),
                    parameter_id: "duck-mix".into(),
                }
                && lane
                    .points
                    .iter()
                    .map(|point| point.value)
                    .collect::<Vec<_>>()
                    == [0.2, 0.8]
        }));
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
    fn source_changes_are_detected_from_dependency_content() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-dependencies-{}",
            riffra_control::new_instance_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let library = root.join("state.json");
        std::fs::write(&library, "one").unwrap();
        let mut dependencies = Dependencies::new();
        record_dependency(&mut dependencies, &library).unwrap();
        let outcome = ScoreOutcome {
            session: CreativeSession::new(1),
            yaml_changed: false,
            changed: false,
            link_changed: false,
            inputs_changed: false,
            diagnostics: Vec::new(),
            dependencies,
            staged: StagedResources::new(&root),
        };
        assert!(!outcome.source_changed());
        std::fs::write(&library, "two").unwrap();
        assert!(outcome.source_changed());
        std::fs::remove_file(&library).unwrap();
        assert!(outcome.source_changed());
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
            production_inputs: BTreeMap::new(),
        });

        // A rebuild that finds no change stores nothing.
        let unchanged = ScoreOutcome {
            session: session.clone(),
            yaml_changed: false,
            changed: false,
            link_changed: false,
            inputs_changed: false,
            diagnostics: Vec::new(),
            dependencies: Dependencies::new(),
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
            inputs_changed: false,
            diagnostics: Vec::new(),
            dependencies: Dependencies::new(),
            staged: StagedResources::new(Path::new("")),
        };
        let candidate = outcome.candidate(&session).expect("a link change commits");
        assert_eq!(candidate.arrangement.revision, session.arrangement.revision);
        assert_eq!(
            candidate.score_source.as_ref().unwrap().path.as_deref(),
            Some("/scores/moved.yaml")
        );

        // A changed input fingerprint is recorded even when the performance is identical.
        let mut inputs = session.clone();
        inputs
            .score_source
            .as_mut()
            .unwrap()
            .production_inputs
            .insert("instruments.lead.source".into(), "new-input".into());
        let outcome = ScoreOutcome {
            session: inputs,
            yaml_changed: false,
            changed: false,
            link_changed: false,
            inputs_changed: true,
            diagnostics: Vec::new(),
            dependencies: Dependencies::new(),
            staged: StagedResources::new(Path::new("")),
        };
        let candidate = outcome
            .candidate(&session)
            .expect("a fingerprint change commits");
        assert_eq!(
            candidate
                .score_source
                .as_ref()
                .unwrap()
                .production_inputs
                .get("instruments.lead.source")
                .map(String::as_str),
            Some("new-input")
        );
        assert_eq!(candidate.arrangement.revision, session.arrangement.revision);
        assert!(candidate.score_performance_equals(&session));
    }
}
