//! Score production content: instruments, effects, and mix.

use super::{
    Export, ExportError, PanLaw, TrackRole, add_file, copy_package, instrument_of, omit, pan_law,
    rational, track_score_id, unique_id,
};
use crate::api::output::{PluginRole, PluginStateSnapshot};
use riffra_core::{
    AutomationInterpolation, AutomationLane, AutomationTarget, BusMixParameter, EffectDevice,
    MixBusKind, SendPosition, TrackInstrumentSource, TrackMixParameter,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Writes every production declaration of the Score document.
///
/// # Errors
/// Returns an error when a plugin or instrument cannot be expressed.
pub(super) fn apply(
    export: &mut Export<'_>,
    document: &mut Map<String, Value>,
) -> Result<(), ExportError> {
    meta(export, document);
    form(export, document);
    time_changes(export, document);
    let ids = MixIds::new(export);
    let (instruments, kits) = instruments(export, &ids)?;
    if instruments.is_empty() {
        return Err(ExportError::new(
            "the Project has no instrument the Score can express",
        ));
    }
    document.insert("instruments".into(), Value::Object(instruments));
    if !kits.is_empty() {
        document.insert("kits".into(), Value::Object(kits));
    }
    mix(export, document, &ids)?;
    Ok(())
}

fn meta(export: &mut Export<'_>, document: &mut Map<String, Value>) {
    let session = export.session;
    let mut meta = Map::new();
    meta.insert(
        "title".into(),
        Value::String(
            session
                .project_name
                .clone()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "Untitled".into()),
        ),
    );
    if let Some(section) = export.sections.first() {
        meta.insert("bpm".into(), json!(section.bpm));
        meta.insert(
            "meter".into(),
            Value::String(format!("{}/{}", section.meter.0, section.meter.1)),
        );
    }
    meta.insert("tuning".into(), Value::String("12edo".into()));
    if session.settings.mixdown.fade_out_seconds > 0.0 {
        meta.insert(
            "fade_out_seconds".into(),
            json!(session.settings.mixdown.fade_out_seconds),
        );
    }
    if let Some(mastering) = &session.settings.mixdown.mastering {
        if (-70.0..=-5.0).contains(&mastering.integrated_lufs)
            && (-9.0..=0.0).contains(&mastering.true_peak_db)
            && mastering.loudness_range_lu.is_none_or(|range| range >= 0.0)
        {
            let mut master = json!({
                "target_lufs": mastering.integrated_lufs,
                "max_true_peak_db": mastering.true_peak_db,
            });
            if let Some(range) = mastering.loudness_range_lu {
                master["target_lra_lu"] = json!(range);
            }
            meta.insert("master".into(), master);
        } else {
            omit(
                &mut export.omissions,
                "mixdown mastering",
                "the loudness targets are outside the Score range",
            );
        }
    }
    if session.settings.mixdown.tail_seconds > 0.0
        || session.settings.mixdown.sample_rate.is_some()
        || session.settings.mixdown.block_size.is_some()
        || session.settings.mixdown.plugin_warm_up_seconds != 2.0
    {
        omit(
            &mut export.omissions,
            "mixdown",
            "the Score stores only the musical end, fades, and loudness targets",
        );
    }
    document.insert("meta".into(), Value::Object(meta));
}

fn form(export: &mut Export<'_>, document: &mut Map<String, Value>) {
    let ppq = export.ppq;
    let mut entries = Vec::with_capacity(export.sections.len());
    for (index, section) in export.sections.iter().enumerate() {
        let mut entry = json!({
            "section": section.name,
            "bars": section.bars(),
            "tonic": section.tonic,
        });
        if let Some(scale) = &section.scale {
            entry["scale"] = json!(scale);
        }
        let previous = index
            .checked_sub(1)
            .map(|previous| &export.sections[previous])
            .unwrap_or(section);
        if previous.bpm.to_bits() != section.bpm.to_bits() {
            entry["bpm"] = json!(section.bpm);
        }
        if previous.meter != section.meter {
            entry["meter"] = json!(format!("{}/{}", section.meter.0, section.meter.1));
        }
        if let Some(last) = section.last_bar_quarters(ppq) {
            entry["last_bar_quarters"] = Value::String(last);
        }
        entries.push(entry);
    }
    document.insert("form".into(), Value::Array(entries));
}

fn time_changes(export: &mut Export<'_>, document: &mut Map<String, Value>) {
    let starts = export
        .sections
        .iter()
        .map(|section| section.start_tick)
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();
    let mut previous = export.sections.first().map(|section| section.bpm);
    for change in &export.session.arrangement.timebase.tempo_changes {
        if change.tick == 0 {
            previous = Some(change.bpm);
            continue;
        }
        if previous.is_some_and(|bpm| bpm.to_bits() == change.bpm.to_bits()) {
            continue;
        }
        if starts.contains(&change.tick) {
            previous = Some(change.bpm);
            continue;
        }
        changes.push(json!({
            "at_quarters": rational(change.tick, export.ppq),
            "bpm": change.bpm,
        }));
        previous = Some(change.bpm);
    }
    if !changes.is_empty() {
        document.insert("time_changes".into(), Value::Array(changes));
    }
}

/// The instrument and kit mappings of one Score document.
type Declarations = (Map<String, Value>, Map<String, Value>);

/// Adds one instrument declaration, refusing to overwrite another.
fn declare(
    instruments: &mut Map<String, Value>,
    id: &str,
    value: Value,
) -> Result<(), ExportError> {
    if instruments.contains_key(id) {
        return Err(ExportError::new(format!(
            "two Score instruments share the ID {id}"
        )));
    }
    instruments.insert(id.to_owned(), value);
    Ok(())
}

/// Builds the instrument and kit declarations.
fn instruments(export: &mut Export<'_>, ids: &MixIds) -> Result<Declarations, ExportError> {
    let mut instruments = Map::new();
    let mut kits = Map::new();
    for track in export.tracks.clone() {
        match &track.role {
            TrackRole::Instrument => {
                let Some(instrument) = instrument_of(track.track) else {
                    continue;
                };
                let mut declaration = Map::new();
                source(export, &track.id, instrument, &mut declaration)?;
                if instrument.bypassed {
                    omit(
                        &mut export.omissions,
                        format!("track \"{}\" / instrument", track.track.name),
                        "a bypassed instrument cannot be written",
                    );
                }
                let (gain, pan) =
                    pan_law(track.track.pan_law, track.track.gain_db, track.track.pan);
                declaration.insert("gain_db".into(), json!(gain));
                declaration.insert("pan".into(), json!(pan));
                declaration.insert(
                    "effects".into(),
                    Value::Array(effects(
                        export,
                        ids,
                        &format!("instruments/{}", track.id),
                        &track.track.effects,
                    )?),
                );
                if !track.track.external_audio_source_track_ids.is_empty() {
                    let sources = track
                        .track
                        .external_audio_source_track_ids
                        .iter()
                        .filter_map(|id| track_score_id(export, id))
                        .collect::<Vec<_>>();
                    if sources.len() == 1 {
                        declaration.insert("audio_input".into(), json!(sources[0]));
                    } else {
                        omit(
                            &mut export.omissions,
                            format!("track \"{}\" / audio input", track.track.name),
                            "the Score can name only one external audio input",
                        );
                    }
                }
                inherited_instrument(export, &track.id, &mut declaration);
                declare(&mut instruments, &track.id, Value::Object(declaration))?;
            }
            TrackRole::Kit(kit) => {
                let Some(instrument) = instrument_of(track.track) else {
                    continue;
                };
                let TrackInstrumentSource::Vst3(plugin) = &instrument.source else {
                    return Err(ExportError::new(format!(
                        "kit \"{kit}\" does not host a VST3 instrument"
                    )));
                };
                let mut source = json!({
                    "vst3": vst3_spec(export, &format!("kits/{kit}"), kit, plugin)?,
                });
                if let Some(role) = export
                    .original
                    .as_ref()
                    .and_then(|original| original.field(&["kits", kit, "source", "role"]))
                {
                    source["role"] = role.clone();
                }
                if let Some(declarations) = export
                    .original
                    .as_ref()
                    .and_then(|original| original.articulations(kit))
                {
                    let declarations =
                        used_articulations(declarations, export.written_articulations.get(kit));
                    if declarations
                        .as_object()
                        .is_some_and(|object| !object.is_empty())
                    {
                        source["articulations"] = declarations;
                    }
                }
                kits.insert(
                    kit.clone(),
                    json!({
                        "source": source,
                        "effects": Value::Array(effects(
                            export,
                            ids,
                            &format!("kits/{kit}"),
                            &track.track.effects,
                        )?),
                    }),
                );
                for member in export.kits.get(kit).cloned().unwrap_or_default() {
                    let mut member_spec = json!({
                        "kit": kit,
                        "trigger_note_midi": member.trigger,
                    });
                    let source = member.source.as_deref().unwrap_or(&member.id);
                    if let Some(role) = export
                        .original
                        .as_ref()
                        .and_then(|original| original.field(&["instruments", source, "role"]))
                    {
                        member_spec["role"] = role.clone();
                    }
                    if let Some(sound) = export
                        .original
                        .as_ref()
                        .and_then(|original| original.field(&["instruments", source, "sound"]))
                    {
                        member_spec["sound"] = sound.clone();
                    }
                    declare(&mut instruments, &member.id, member_spec)?;
                }
            }
            TrackRole::Audio => {
                let (gain, pan) =
                    pan_law(track.track.pan_law, track.track.gain_db, track.track.pan);
                declare(
                    &mut instruments,
                    &track.id,
                    json!({
                        "audio": true,
                        "gain_db": gain,
                        "pan": pan,
                        "effects": Value::Array(effects(
                            export,
                            ids,
                            &format!("instruments/{}", track.id),
                            &track.track.effects,
                        )?),
                    }),
                )?;
            }
        }
    }
    Ok((instruments, kits))
}

/// Keeps the articulation declarations the exported events use.
fn used_articulations(declarations: &Value, used: Option<&BTreeSet<String>>) -> Value {
    let mut result = Map::new();
    if let Some(object) = declarations.as_object() {
        for (name, declaration) in object {
            if name == "default" || used.is_some_and(|used| used.contains(name)) {
                result.insert(name.clone(), declaration.clone());
            }
        }
    }
    Value::Object(result)
}

/// Copies the declarations only the linked Score still knows.
fn inherited_instrument(export: &mut Export<'_>, id: &str, declaration: &mut Map<String, Value>) {
    let Some(original) = export.original.as_ref() else {
        return;
    };
    for key in ["role", "sound"] {
        if let Some(value) = original.field(&["instruments", id, key]) {
            declaration.insert(key.into(), value.clone());
        }
    }
}

/// Resolves one instrument's source declaration.
fn source(
    export: &mut Export<'_>,
    id: &str,
    instrument: &riffra_core::TrackInstrument,
    declaration: &mut Map<String, Value>,
) -> Result<(), ExportError> {
    match &instrument.source {
        TrackInstrumentSource::Internal {
            resource: riffra_core::InternalInstrumentResource::BuiltInPreset { preset_id },
            ..
        } => {
            export.catalog.resolve(preset_id).map_err(|error| {
                ExportError::new(format!("built-in preset {preset_id}: {error}"))
            })?;
            declaration.insert("preset".into(), json!(preset_id));
        }
        TrackInstrumentSource::Internal {
            resource: riffra_core::InternalInstrumentResource::UserSnapshot { snapshot_id, .. },
            ..
        } => {
            let root = export
                .data_root
                .join("project-instruments")
                .join(snapshot_id);
            let directory = format!("sounds/{id}");
            copy_package(export, &root, &directory)?;
            declaration.insert(
                "definition".into(),
                json!(format!("{directory}/definition.json")),
            );
        }
        TrackInstrumentSource::Vst3(plugin) => {
            let mut spec = json!({
                "vst3": vst3_spec(export, &format!("instruments/{id}"), id, plugin)?,
            });
            if let Some(declarations) = export
                .original
                .as_ref()
                .and_then(|original| original.articulations(id))
            {
                let declarations =
                    used_articulations(declarations, export.written_articulations.get(id));
                if declarations
                    .as_object()
                    .is_some_and(|object| !object.is_empty())
                {
                    spec["articulations"] = declarations;
                }
            }
            declaration.insert("source".into(), spec);
        }
    }
    Ok(())
}

/// Builds one `source.vst3` declaration and its state file.
fn vst3_spec(
    export: &mut Export<'_>,
    scope: &str,
    id: &str,
    plugin: &riffra_core::Vst3Plugin,
) -> Result<Value, ExportError> {
    let entry = plugin_entry(export, plugin, PluginRole::Instrument, id)?;
    let mut vst3 = json!({"plugin": entry.name});
    if let Some(identifier) = &entry.identifier {
        vst3["identifier"] = json!(identifier);
    }
    if let Some(state) = state_file(export, scope, "instrument", &entry, plugin)? {
        vst3["state"] = json!(state);
    }
    Ok(vst3)
}

/// Resolves the plugin catalog entry of one VST3 device.
fn plugin_entry(
    export: &Export<'_>,
    plugin: &riffra_core::Vst3Plugin,
    role: PluginRole,
    subject: &str,
) -> Result<crate::api::output::PluginEntry, ExportError> {
    if plugin.disabled_placeholder {
        return Err(ExportError::new(format!(
            "the device on \"{subject}\" is a disabled missing plugin"
        )));
    }
    let entry = crate::plugins::catalog::validated_entry_at_path(export.data_root, &plugin.path)
        .map_err(|error| ExportError::new(format!("{subject}: {error}")))?;
    if entry.role != Some(role) {
        return Err(ExportError::new(format!(
            "plugin \"{}\" is not validated as {role:?}",
            entry.name
        )));
    }
    Ok(entry)
}

/// Writes one plugin state file and returns its Score path.
fn state_file(
    export: &mut Export<'_>,
    scope: &str,
    device: &str,
    entry: &crate::api::output::PluginEntry,
    plugin: &riffra_core::Vst3Plugin,
) -> Result<Option<String>, ExportError> {
    let Some(identifier) = entry.identifier.clone() else {
        omit(
            &mut export.omissions,
            format!("plugin \"{}\"", entry.name),
            "the catalog entry has no stable identifier, so the plugin state cannot be stored",
        );
        return Ok(None);
    };
    let relative = format!("states/{scope}/{device}.json");
    let snapshot = PluginStateSnapshot {
        schema_version: 2,
        plugin_identifier: identifier,
        plugin_path: entry.path.clone(),
        parameter_values: plugin.parameter_values.clone(),
        state_data: plugin.state_data.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&snapshot)
        .map_err(|error| ExportError::new(format!("plugin state could not be encoded: {error}")))?;
    add_file(export, relative.clone(), bytes)?;
    Ok(Some(relative))
}

/// Assigns the Score ID of every effect in one chain, in order.
fn chain_ids(chain: &[EffectDevice]) -> Vec<(String, String)> {
    let mut used = BTreeSet::new();
    chain
        .iter()
        .map(|effect| {
            (
                effect.id.clone(),
                unique_id(&effect.name, "effect", &mut used),
            )
        })
        .collect()
}

/// Builds one effect chain declaration.
fn effects(
    export: &mut Export<'_>,
    ids: &MixIds,
    scope: &str,
    effects: &[EffectDevice],
) -> Result<Vec<Value>, ExportError> {
    let mut result = Vec::with_capacity(effects.len());
    for effect in effects {
        let (_, id) = &ids.effects[&effect.id];
        let entry = plugin_entry(export, &effect.plugin, PluginRole::Effect, &effect.name)?;
        let mut vst3 = json!({"plugin": entry.name});
        if let Some(identifier) = &entry.identifier {
            vst3["identifier"] = json!(identifier);
        }
        if let Some(state) = state_file(
            export,
            scope,
            &format!("effects/{id}"),
            &entry,
            &effect.plugin,
        )? {
            vst3["state"] = json!(state);
        }
        let mut declaration = json!({"id": id, "vst3": vst3});
        if effect.bypassed {
            declaration["bypassed"] = json!(true);
        }
        result.push(declaration);
    }
    Ok(result)
}

/// The Score IDs of the mixer and the owners of every effect device.
struct MixIds {
    buses: BTreeMap<String, String>,
    effects: BTreeMap<String, (ReferenceOwner, String)>,
}

impl MixIds {
    fn new(export: &Export<'_>) -> Self {
        let mut used = BTreeSet::new();
        let buses = export
            .session
            .arrangement
            .mixer
            .buses
            .iter()
            .map(|bus| (bus.id.clone(), unique_id(&bus.name, "bus", &mut used)))
            .collect::<BTreeMap<_, _>>();
        let mut effects = BTreeMap::new();
        for track in &export.tracks {
            let owner = match &track.role {
                TrackRole::Kit(kit) => ReferenceOwner::Kit(kit.clone()),
                _ => ReferenceOwner::Instrument(track.id.clone()),
            };
            for (device, id) in chain_ids(&track.track.effects) {
                effects.insert(device, (owner.clone(), id));
            }
        }
        for bus in &export.session.arrangement.mixer.buses {
            let owner = ReferenceOwner::Bus(buses[&bus.id].clone());
            for (device, id) in chain_ids(&bus.effects) {
                effects.insert(device, (owner.clone(), id));
            }
        }
        for (device, id) in chain_ids(&export.session.arrangement.mixer.master_effects) {
            effects.insert(device, (ReferenceOwner::Master, id));
        }
        Self { buses, effects }
    }
}

/// Builds the mixer, routing, and automation declarations.
fn mix(
    export: &mut Export<'_>,
    document: &mut Map<String, Value>,
    ids: &MixIds,
) -> Result<(), ExportError> {
    let mut mix = Map::new();
    let mut buses = Map::new();
    for bus in &export.session.arrangement.mixer.buses {
        let id = ids.buses[&bus.id].clone();
        let (gain, pan) = pan_law(bus.pan_law, bus.gain_db, bus.pan);
        buses.insert(
            id.clone(),
            json!({
                "kind": match bus.kind {
                    MixBusKind::Group => "group",
                    MixBusKind::Return => "return",
                },
                "gain_db": gain,
                "pan": pan,
                "effects": Value::Array(effects(
                    export,
                    ids,
                    &format!("buses/{id}"),
                    &bus.effects,
                )?),
            }),
        );
    }
    let mut tracks = Map::new();
    for track in export.tracks.clone() {
        let mut declaration = Map::new();
        if let Some(output) = &track.track.output_bus_id {
            let Some(bus) = ids.buses.get(output) else {
                return Err(ExportError::new(format!(
                    "track \"{}\" routes to an undefined bus",
                    track.track.name
                )));
            };
            declaration.insert("output".into(), json!(bus));
        }
        let mut sends = Map::new();
        for send in export
            .session
            .arrangement
            .mixer
            .sends
            .iter()
            .filter(|send| send.track_id == track.track.id)
        {
            let Some(bus) = ids.buses.get(&send.return_bus_id) else {
                return Err(ExportError::new(format!(
                    "track \"{}\" sends to an undefined bus",
                    track.track.name
                )));
            };
            let value = if send.position == SendPosition::PostFader && send.enabled {
                json!(send.level_db)
            } else {
                json!({
                    "level_db": send.level_db,
                    "position": if send.position == SendPosition::PreFader { "pre" } else { "post" },
                    "enabled": send.enabled,
                })
            };
            sends.insert(bus.clone(), value);
        }
        if !sends.is_empty() {
            declaration.insert("sends".into(), Value::Object(sends));
        }
        if let TrackRole::Kit(_) = track.role {
            let (gain, pan) = pan_law(track.track.pan_law, track.track.gain_db, track.track.pan);
            declaration.insert("gain_db".into(), json!(gain));
            declaration.insert("pan".into(), json!(pan));
        }
        if !declaration.is_empty() {
            tracks.insert(track.id.clone(), Value::Object(declaration));
        }
    }
    if !tracks.is_empty() {
        mix.insert("tracks".into(), Value::Object(tracks));
    }
    if !buses.is_empty() {
        mix.insert("buses".into(), Value::Object(buses));
    }
    let sidechain = sidechain(export, ids);
    if !sidechain.is_empty() {
        mix.insert("sidechain".into(), Value::Array(sidechain));
    }
    let mut master = Map::new();
    if export.session.settings.master_db != 0.0 {
        master.insert("gain_db".into(), json!(export.session.settings.master_db));
    }
    let master_effects = effects(
        export,
        ids,
        "master",
        &export.session.arrangement.mixer.master_effects,
    )?;
    if !master_effects.is_empty() {
        master.insert("effects".into(), Value::Array(master_effects));
    }
    if !master.is_empty() {
        mix.insert("master".into(), Value::Object(master));
    }
    let automation = automation(export, ids);
    if !automation.is_empty() {
        mix.insert("automation".into(), Value::Array(automation));
    }
    if !mix.is_empty() {
        document.insert("mix".into(), Value::Object(mix));
    }
    Ok(())
}

/// Builds the sidechain routes declared by the canonical mixer.
fn sidechain(export: &mut Export<'_>, ids: &MixIds) -> Vec<Value> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for send in &export.session.arrangement.mixer.sidechain_sends {
        let (Some(from), Some((owner, effect))) = (
            track_score_id(export, &send.source_track_id),
            ids.effects.get(&send.target_device_id),
        ) else {
            omit(
                &mut export.omissions,
                "sidechain",
                format!(
                    "the route from \"{}\" targets a device the Score does not export",
                    send.source_track_id
                ),
            );
            continue;
        };
        let key = (from.clone(), owner.clone(), effect.clone());
        if !seen.insert(key) {
            continue;
        }
        let mut route = json!({
            "from": from,
            "to": effect_target(owner, effect, "effect"),
        });
        if send.level_db != 0.0 {
            route["level_db"] = json!(send.level_db);
        }
        if send.position == SendPosition::PreFader {
            route["position"] = json!("pre");
        }
        if !send.enabled {
            route["enabled"] = json!(false);
        }
        result.push(route);
    }
    result.sort_by_key(|route| {
        (
            route["from"].as_str().unwrap_or_default().to_owned(),
            route["to"]["effect"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    });
    result
}

/// The Score owner of an effect device.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ReferenceOwner {
    Instrument(String),
    Kit(String),
    Bus(String),
    Master,
}

/// Renders the Score reference to one effect device.
fn effect_target(owner: &ReferenceOwner, effect: &str, id_key: &str) -> Value {
    let mut to = json!({ id_key: effect });
    match owner {
        ReferenceOwner::Instrument(id) => to["instrument"] = json!(id),
        ReferenceOwner::Kit(id) => to["kit"] = json!(id),
        ReferenceOwner::Bus(id) => to["bus"] = json!(id),
        ReferenceOwner::Master => to["master"] = json!(true),
    }
    to
}

/// Builds the score automation declarations from canonical lanes.
fn automation(export: &mut Export<'_>, ids: &MixIds) -> Vec<Value> {
    let lanes = export.session.arrangement.automation_lanes.clone();
    let mut result = Vec::new();
    for lane in &lanes {
        let Some(target) = automation_target(export, ids, lane) else {
            continue;
        };
        let interpolation = match lane.interpolation {
            AutomationInterpolation::Linear => "linear",
            AutomationInterpolation::Step => "step",
            AutomationInterpolation::LinearAmplitude { .. } => {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "the Score evaluates this volume automation linearly",
                );
                "linear"
            }
        };
        let (offset, pan) = automation_offset(export, &lane.target);
        if pan {
            omit(
                &mut export.omissions,
                format!("automation \"{}\"", lane.id),
                "pan automation on a non-linear pan law cannot be converted",
            );
            continue;
        }
        let mut groups = BTreeMap::<(usize, usize), Vec<(u64, Value)>>::new();
        for point in &lane.points {
            let Some((section, occurrence)) = locate(export, point.tick.0) else {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "a point lies outside every section",
                );
                continue;
            };
            let section_ref = &export.sections[section];
            groups.entry((section, occurrence)).or_default().push((
                point.tick.0,
                json!({
                    "at": section_ref.position(point.tick.0, export.ppq),
                    "value": point.value + offset,
                }),
            ));
        }
        for ((section, occurrence), mut points) in groups {
            points.sort_by_key(|(tick, _)| *tick);
            let points = points
                .into_iter()
                .map(|(_, point)| point)
                .collect::<Vec<_>>();
            let section = &export.sections[section];
            let mut declaration = json!({
                "target": target,
                "section": section.name,
                "points": points,
            });
            if occurrence > 1 {
                declaration["occurrence"] = json!(occurrence);
            }
            if interpolation != "linear" {
                declaration["interpolation"] = json!(interpolation);
            }
            result.push(declaration);
        }
    }
    result.sort_by_key(|declaration| {
        (
            declaration["section"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            declaration["target"].to_string(),
        )
    });
    result
}

/// Returns the section containing a tick and its form occurrence.
fn locate(export: &Export<'_>, tick: u64) -> Option<(usize, usize)> {
    let mut occurrence = BTreeMap::<String, usize>::new();
    for (index, section) in export.sections.iter().enumerate() {
        let entry = occurrence.entry(section.name.clone()).or_insert(0);
        *entry += 1;
        let occurrence = *entry;
        if tick >= section.start_tick && tick < section.end_tick {
            return Some((index, occurrence));
        }
        if index + 1 == export.sections.len() && tick == section.end_tick {
            return Some((index, occurrence));
        }
    }
    None
}

/// Returns the pan law gain offset applied to a lane and whether it controls pan.
fn automation_offset(export: &Export<'_>, target: &AutomationTarget) -> (f64, bool) {
    let converted = |pan: PanLaw, gain: f64, pan_value: f64| pan_law(pan, gain, pan_value);
    match target {
        AutomationTarget::TrackMix {
            track_id,
            parameter,
        } => {
            let Some(track) = export
                .session
                .arrangement
                .tracks
                .iter()
                .find(|track| track.id == *track_id)
            else {
                return (0.0, false);
            };
            match parameter {
                TrackMixParameter::Volume => {
                    let (gain, _) = converted(track.pan_law, track.gain_db, track.pan);
                    (gain - track.gain_db, false)
                }
                TrackMixParameter::Pan => (0.0, track.pan_law != PanLaw::LinearBalance),
            }
        }
        AutomationTarget::BusMix { bus_id, parameter } => {
            let Some(bus) = export
                .session
                .arrangement
                .mixer
                .buses
                .iter()
                .find(|bus| bus.id == *bus_id)
            else {
                return (0.0, false);
            };
            match parameter {
                BusMixParameter::Gain => {
                    let (gain, _) = converted(bus.pan_law, bus.gain_db, bus.pan);
                    (gain - bus.gain_db, false)
                }
                BusMixParameter::Pan => (0.0, bus.pan_law != PanLaw::LinearBalance),
            }
        }
        _ => (0.0, false),
    }
}

/// Maps one canonical lane target to a Score automation target.
fn automation_target(
    export: &mut Export<'_>,
    ids: &MixIds,
    lane: &AutomationLane,
) -> Option<Value> {
    let target = match &lane.target {
        AutomationTarget::TrackMix {
            track_id,
            parameter,
        } => {
            let Some(id) = track_score_id(export, track_id) else {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "the track is not exported",
                );
                return None;
            };
            let owner = if let Some(kit) = super::kit_of(export, track_id) {
                json!({"kit": kit})
            } else {
                json!({"instrument": id})
            };
            let mut target = owner;
            target["parameter"] = json!(match parameter {
                TrackMixParameter::Volume => "volume",
                TrackMixParameter::Pan => "pan",
            });
            target
        }
        AutomationTarget::BusMix { bus_id, parameter } => {
            let Some(id) = ids.buses.get(bus_id) else {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "the bus is not exported",
                );
                return None;
            };
            json!({
                "bus": id,
                "parameter": match parameter {
                    BusMixParameter::Gain => "gain",
                    BusMixParameter::Pan => "pan",
                },
            })
        }
        AutomationTarget::MasterGain => json!({"master": true, "parameter": "gain"}),
        AutomationTarget::SendLevel { send_id } => {
            let Some(send) = export
                .session
                .arrangement
                .mixer
                .sends
                .iter()
                .find(|send| send.id == *send_id)
            else {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "the send is not exported",
                );
                return None;
            };
            let (Some(instrument), Some(bus)) = (
                track_score_id(export, &send.track_id),
                ids.buses.get(&send.return_bus_id),
            ) else {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "the send source or bus is not exported",
                );
                return None;
            };
            json!({"send": {"instrument": instrument, "bus": bus}})
        }
        AutomationTarget::SidechainSendLevel { sidechain_send_id } => {
            let Some(send) = export
                .session
                .arrangement
                .mixer
                .sidechain_sends
                .iter()
                .find(|send| send.id == *sidechain_send_id)
            else {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "the sidechain route is not exported",
                );
                return None;
            };
            let (Some(from), Some((owner, effect))) = (
                track_score_id(export, &send.source_track_id),
                ids.effects.get(&send.target_device_id),
            ) else {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "the sidechain route targets a device the Score does not export",
                );
                return None;
            };
            json!({
                "sidechain": {
                    "from": from,
                    "to": effect_target(owner, effect, "effect"),
                }
            })
        }
        AutomationTarget::DeviceParameter {
            device_id,
            parameter_id,
        } => {
            let name = match parameter_name(export, device_id, parameter_id) {
                Ok(Some(name)) => name,
                Ok(None) => {
                    omit(
                        &mut export.omissions,
                        format!("automation \"{}\"", lane.id),
                        "the plugin parameter name is not available",
                    );
                    return None;
                }
                Err(reason) => {
                    omit(
                        &mut export.omissions,
                        format!("automation \"{}\"", lane.id),
                        reason,
                    );
                    return None;
                }
            };
            if let Some((owner, effect)) = ids.effects.get(device_id) {
                json!({
                    "effect": effect_target(owner, effect, "id"),
                    "parameter": name,
                })
            } else {
                let Some(track) = export.session.arrangement.tracks.iter().find(|track| {
                    track
                        .instrument
                        .as_ref()
                        .is_some_and(|instrument| instrument.id == *device_id)
                }) else {
                    omit(
                        &mut export.omissions,
                        format!("automation \"{}\"", lane.id),
                        "the target device is not exported",
                    );
                    return None;
                };
                if track
                    .instrument
                    .as_ref()
                    .is_some_and(|instrument| instrument.as_vst3().is_none())
                {
                    omit(
                        &mut export.omissions,
                        format!("automation \"{}\"", lane.id),
                        "built-in instrument parameters are written as performance events, not mix automation",
                    );
                    return None;
                }
                if super::kit_of(export, &track.id).is_some() {
                    omit(
                        &mut export.omissions,
                        format!("automation \"{}\"", lane.id),
                        "kit source parameters cannot be targeted by Score mix automation",
                    );
                    return None;
                }
                let Some(id) = track_score_id(export, &track.id) else {
                    omit(
                        &mut export.omissions,
                        format!("automation \"{}\"", lane.id),
                        "the instrument is not exported",
                    );
                    return None;
                };
                json!({"source": id, "parameter": name})
            }
        }
    };
    Some(target)
}

/// Resolves a stable plugin parameter ID to its Score selector.
fn parameter_name(
    export: &mut Export<'_>,
    device_id: &str,
    parameter_id: &str,
) -> Result<Option<String>, String> {
    let plugin = export
        .session
        .arrangement
        .tracks
        .iter()
        .flat_map(|track| {
            track
                .instrument
                .as_ref()
                .and_then(|instrument| {
                    instrument
                        .as_vst3()
                        .map(|plugin| (instrument.id.clone(), plugin))
                })
                .into_iter()
                .chain(
                    track
                        .effects
                        .iter()
                        .map(|effect| (effect.id.clone(), &effect.plugin)),
                )
        })
        .chain(
            export
                .session
                .arrangement
                .mixer
                .buses
                .iter()
                .flat_map(|bus| {
                    bus.effects
                        .iter()
                        .map(|effect| (effect.id.clone(), &effect.plugin))
                }),
        )
        .chain(
            export
                .session
                .arrangement
                .mixer
                .master_effects
                .iter()
                .map(|effect| (effect.id.clone(), &effect.plugin)),
        )
        .find(|(id, _)| id == device_id);
    let Some((_, plugin)) = plugin else {
        return Err("the automation target is not a VST3 plugin".into());
    };
    let parameters = if let Some(cached) = export.parameters.get(device_id) {
        cached
            .iter()
            .map(|parameter| {
                (
                    parameter.id.clone(),
                    parameter.name.clone(),
                    parameter.index,
                )
            })
            .collect::<Vec<_>>()
    } else {
        let described =
            crate::plugins::validation::describe_parameters(export.scanner, plugin, &[])
                .map_err(|error| format!("plugin parameters could not be read: {error}"))?;
        let parameters = described
            .parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.parameter_id.clone(),
                    parameter.name.clone(),
                    parameter.index,
                )
            })
            .collect::<Vec<_>>();
        export.parameters.insert(
            device_id.to_owned(),
            described
                .parameters
                .iter()
                .map(|parameter| super::PluginParameter {
                    id: parameter.parameter_id.clone(),
                    name: parameter.name.clone(),
                    index: parameter.index,
                })
                .collect(),
        );
        parameters
    };
    let Some((_, name, index)) = parameters
        .iter()
        .find(|(id, _, _)| id.as_deref() == Some(parameter_id))
    else {
        return Ok(None);
    };
    let duplicates = parameters
        .iter()
        .filter(|(_, other, _)| other == name)
        .count();
    Ok(Some(if duplicates > 1 {
        format!("{name}#{index}")
    } else {
        name.clone()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{
        AutomationInterpolation, AutomationPoint, CreativeSession, TimelineTick, Track,
        TrackInstrument,
    };

    fn export<'a>(
        session: &'a CreativeSession,
        sections: Vec<super::super::SongSection>,
        tracks: Vec<super::super::ExportTrack<'a>>,
    ) -> Export<'a> {
        Export {
            session,
            data_root: std::path::Path::new("."),
            catalog: crate::test_support::empty_built_in_catalog(),
            scanner: std::path::Path::new("unused"),
            ppq: 480,
            sections,
            tracks,
            kits: BTreeMap::new(),
            original: None,
            omissions: Vec::new(),
            files: BTreeMap::new(),
            audio_files: BTreeMap::new(),
            parameters: BTreeMap::new(),
            written_articulations: BTreeMap::new(),
        }
    }

    fn lane(target: AutomationTarget) -> AutomationLane {
        AutomationLane {
            id: "lane".into(),
            target,
            interpolation: AutomationInterpolation::Linear,
            points: vec![AutomationPoint {
                id: "point".into(),
                tick: TimelineTick(0),
                value: 0.5,
            }],
        }
    }

    #[test]
    fn duplicate_instrument_declarations_are_refused() {
        let mut instruments = Map::new();
        declare(&mut instruments, "lead", json!({"audio": true})).unwrap();
        let error = declare(&mut instruments, "lead", json!({"audio": false})).unwrap_err();
        assert!(error.message.contains("ID lead"), "{}", error.message);
        assert_eq!(instruments["lead"]["audio"], true);
    }

    #[test]
    fn plugin_automation_targets_the_source_or_the_effect() {
        let mut session = CreativeSession::new(1);
        let mut track = Track::instrument("track:1".into(), "Lead".into());
        track.instrument = Some(
            TrackInstrument::vst3(
                "device:instrument".into(),
                "Synth".into(),
                "C:/plugins/synth.vst3".into(),
            )
            .unwrap(),
        );
        track.effects.push(
            EffectDevice::new(
                "device:effect".into(),
                "Drive".into(),
                "C:/plugins/drive.vst3".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks.push(track);
        let section = super::super::SongSection {
            base: "verse".into(),
            name: "verse".into(),
            start_tick: 0,
            end_tick: 1920,
            bar_lengths: vec![1920],
            bar_starts: vec![0],
            meter: (4, 4),
            bpm: 120.0,
            tonic: "C".into(),
            scale: None,
            inherited: false,
        };
        let tracks = vec![super::super::ExportTrack {
            id: "lead".into(),
            track: &session.arrangement.tracks[0],
            role: TrackRole::Instrument,
        }];
        let mut export = export(&session, vec![section], tracks);
        export.parameters.insert(
            "device:instrument".into(),
            vec![super::super::PluginParameter {
                id: Some("cutoff".into()),
                name: "Cutoff".into(),
                index: 0,
            }],
        );
        export.parameters.insert(
            "device:effect".into(),
            vec![super::super::PluginParameter {
                id: Some("drive".into()),
                name: "Drive".into(),
                index: 0,
            }],
        );
        let ids = MixIds::new(&export);
        assert_eq!(
            automation_target(
                &mut export,
                &ids,
                &lane(AutomationTarget::DeviceParameter {
                    device_id: "device:instrument".into(),
                    parameter_id: "cutoff".into(),
                }),
            ),
            Some(json!({"source": "lead", "parameter": "Cutoff"}))
        );
        assert_eq!(
            automation_target(
                &mut export,
                &ids,
                &lane(AutomationTarget::DeviceParameter {
                    device_id: "device:effect".into(),
                    parameter_id: "drive".into(),
                }),
            ),
            Some(json!({
                "effect": {"instrument": "lead", "id": "drive"},
                "parameter": "Drive",
            }))
        );
    }
}
