//! Score production content: instruments, effects, mix, audio clips.

use super::{
    Export, ExportError, PanLaw, TrackRole, add_file, copy_package, file_identifier, instrument_of,
    kit_of, omit, pan_law, rational, track_score_id, unique_id,
};
use crate::api::output::{PluginRole, PluginStateSnapshot};
use riffra_core::{
    AudioClip, AutomationInterpolation, AutomationLane, AutomationTarget, BusMixParameter,
    MixBusKind, SendPosition, Track, TrackInstrumentSource, TrackMixParameter,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Writes every production declaration of the Score document.
///
/// # Errors
/// Returns an error when a plugin or audio file cannot be expressed.
pub(super) fn apply(
    export: &mut Export<'_>,
    document: &mut Map<String, Value>,
) -> Result<(), ExportError> {
    meta(export, document);
    form(export, document);
    time_changes(export, document);
    let (instruments, kits) = instruments(export)?;
    if instruments.is_empty() {
        return Err(ExportError::new(
            "the Project has no instrument the Score can express",
        ));
    }
    document.insert("instruments".into(), Value::Object(instruments));
    if !kits.is_empty() {
        document.insert("kits".into(), Value::Object(kits));
    }
    mix(export, document)?;
    audio(export, document)?;
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
    let first = export.sections.first().cloned();
    if let Some(section) = &first {
        meta.insert("bpm".into(), json!(section.bpm));
        meta.insert(
            "meter".into(),
            Value::String(format!("{}/{}", section.meter.0, section.meter.1)),
        );
    }
    meta.insert("tuning".into(), Value::String("12edo".into()));
    if export.session.settings.mixdown.fade_out_seconds > 0.0 {
        meta.insert(
            "fade_out_seconds".into(),
            json!(export.session.settings.mixdown.fade_out_seconds),
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
    for section in &export.sections {
        let _ = section;
    }
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

/// Builds the instrument and kit declarations.
fn instruments(export: &mut Export<'_>) -> Result<Declarations, ExportError> {
    let mut instruments = Map::new();
    let mut kits = Map::new();
    let tracks = export.tracks.clone();
    for track in &tracks {
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
                        &track.id,
                        &track.track.effects,
                        "instruments",
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
                instruments.insert(track.id.clone(), Value::Object(declaration));
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
                let vst3 = vst3_spec(export, kit, plugin)?;
                let mut source = json!({"vst3": vst3});
                if let Some(role) = export
                    .original
                    .as_ref()
                    .and_then(|original| original.field(&["kits", kit, "source", "role"]))
                {
                    source["role"] = role.clone();
                }
                if let Some(articulations) = export
                    .original
                    .as_ref()
                    .and_then(|original| original.articulations(kit))
                {
                    source["articulations"] = articulations.clone();
                }
                kits.insert(
                    kit.clone(),
                    json!({
                        "source": source,
                        "effects": Value::Array(
                            effects(export, kit, &track.track.effects, "kits")?
                        ),
                    }),
                );
                for member in export.kits.get(kit).cloned().unwrap_or_default() {
                    let mut member_spec = json!({
                        "kit": kit,
                        "trigger_note_midi": member.trigger,
                    });
                    if let Some(role) = export
                        .original
                        .as_ref()
                        .and_then(|original| original.field(&["instruments", &member.id, "role"]))
                    {
                        member_spec["role"] = role.clone();
                    }
                    if let Some(sound) = export
                        .original
                        .as_ref()
                        .and_then(|original| original.field(&["instruments", &member.id, "sound"]))
                    {
                        member_spec["sound"] = sound.clone();
                    }
                    instruments.insert(member.id.clone(), member_spec);
                }
            }
            TrackRole::Audio => {
                let (gain, pan) =
                    pan_law(track.track.pan_law, track.track.gain_db, track.track.pan);
                instruments.insert(
                    track.id.clone(),
                    json!({
                        "audio": true,
                        "gain_db": gain,
                        "pan": pan,
                        "effects": Value::Array(
                            effects(export, &track.id, &track.track.effects, "instruments")?
                        ),
                    }),
                );
            }
        }
    }
    Ok((instruments, kits))
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
            let mut spec = json!({"vst3": vst3_spec(export, id, plugin)?});
            if let Some(articulations) = export
                .original
                .as_ref()
                .and_then(|original| original.articulations(id))
            {
                spec["articulations"] = articulations.clone();
            }
            declaration.insert("source".into(), spec);
        }
    }
    Ok(())
}

/// Builds one `source.vst3` declaration and its state file.
fn vst3_spec(
    export: &mut Export<'_>,
    id: &str,
    plugin: &riffra_core::Vst3Plugin,
) -> Result<Value, ExportError> {
    let entry = plugin_entry(export, plugin, PluginRole::Instrument, id)?;
    let mut vst3 = json!({"plugin": entry.name});
    if let Some(identifier) = &entry.identifier {
        vst3["identifier"] = json!(identifier);
    }
    if let Some(state) = state_file(export, id, &entry, plugin)? {
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
    owner: &str,
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
    let relative = format!("states/{owner}.json");
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

/// Builds one effect chain declaration.
fn effects(
    export: &mut Export<'_>,
    owner: &str,
    effects: &[riffra_core::EffectDevice],
    field: &str,
) -> Result<Vec<Value>, ExportError> {
    let mut used = BTreeSet::new();
    let mut result = Vec::with_capacity(effects.len());
    for effect in effects {
        let id = unique_id(&effect.name, "effect", &mut used);
        let entry = plugin_entry(export, &effect.plugin, PluginRole::Effect, &effect.name)?;
        let mut vst3 = json!({"plugin": entry.name});
        if let Some(identifier) = &entry.identifier {
            vst3["identifier"] = json!(identifier);
        }
        if let Some(state) = state_file(export, &format!("{owner}-{id}"), &entry, &effect.plugin)? {
            vst3["state"] = json!(state);
        }
        let mut declaration = json!({"id": id, "vst3": vst3});
        if effect.bypassed {
            declaration["bypassed"] = json!(true);
        }
        result.push(declaration);
        let _ = field;
    }
    Ok(result)
}

/// Builds the mixer, routing, and automation declarations.
fn mix(export: &mut Export<'_>, document: &mut Map<String, Value>) -> Result<(), ExportError> {
    let mut mix = Map::new();
    let mut tracks = Map::new();
    let mut buses = Map::new();
    let mut bus_ids = BTreeMap::<String, String>::new();
    let mut used_buses = BTreeSet::new();
    for bus in &export.session.arrangement.mixer.buses {
        let id = unique_id(&bus.name, "bus", &mut used_buses);
        let (gain, pan) = pan_law(bus.pan_law, bus.gain_db, bus.pan);
        let mut declaration = json!({
            "kind": match bus.kind {
                MixBusKind::Group => "group",
                MixBusKind::Return => "return",
            },
            "gain_db": gain,
            "pan": pan,
            "effects": effects(export, &id, &bus.effects, "buses")?,
        });
        if bus.muted || bus.solo {
            omit(
                &mut export.omissions,
                format!("bus \"{}\"", bus.name),
                "mute and solo are work state",
            );
        }
        declaration["effects"] = json!(effects(export, &id, &bus.effects, "buses")?);
        buses.insert(id.clone(), declaration);
        bus_ids.insert(bus.id.clone(), id);
    }
    let mut used_track_ids = BTreeSet::new();
    for track in export.tracks.clone() {
        if matches!(track.role, TrackRole::Audio) {
            continue;
        }
        let mut declaration = Map::new();
        if track.track.muted || track.track.solo {
            omit(
                &mut export.omissions,
                format!("track \"{}\"", track.track.name),
                "mute and solo are work state",
            );
        }
        if let Some(output) = &track.track.output_bus_id {
            let Some(bus) = bus_ids.get(output) else {
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
            let Some(bus) = bus_ids.get(&send.return_bus_id) else {
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
            let id = unique_id(&track.id, "track", &mut used_track_ids);
            tracks.insert(id, Value::Object(declaration));
        }
    }
    if !tracks.is_empty() {
        mix.insert("tracks".into(), Value::Object(tracks));
    }
    if !buses.is_empty() {
        mix.insert("buses".into(), Value::Object(buses));
    }
    let sidechain = sidechain(export, &bus_ids)?;
    if !sidechain.is_empty() {
        mix.insert("sidechain".into(), Value::Array(sidechain));
    }
    let mut master = Map::new();
    if export.session.settings.master_db != 0.0 {
        master.insert("gain_db".into(), json!(export.session.settings.master_db));
    }
    let master_effects = effects(
        export,
        "master",
        &export.session.arrangement.mixer.master_effects,
        "master",
    )?;
    if !master_effects.is_empty() {
        master.insert("effects".into(), Value::Array(master_effects));
    }
    if !master.is_empty() {
        mix.insert("master".into(), Value::Object(master));
    }
    let automation = automation(export)?;
    if !automation.is_empty() {
        mix.insert("automation".into(), Value::Array(automation));
    }
    if !mix.is_empty() {
        document.insert("mix".into(), Value::Object(mix));
    }
    Ok(())
}

/// Builds the sidechain routes declared by the canonical mixer.
fn sidechain(
    export: &mut Export<'_>,
    _buses: &BTreeMap<String, String>,
) -> Result<Vec<Value>, ExportError> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for send in &export.session.arrangement.mixer.sidechain_sends {
        let Some(from) = track_score_id(export, &send.source_track_id) else {
            return Err(ExportError::new(
                "a sidechain route starts from a track the Score does not export",
            ));
        };
        let Some((owner, effect)) = effect_reference(export, &send.target_device_id) else {
            return Err(ExportError::new(
                "a sidechain route targets a device the Score does not export",
            ));
        };
        let key = (from.clone(), owner.clone(), effect.clone());
        if !seen.insert(key) {
            continue;
        }
        let mut to = json!({ "effect": effect });
        match &owner {
            ReferenceOwner::Instrument(id) => to["instrument"] = json!(id),
            ReferenceOwner::Kit(id) => to["kit"] = json!(id),
            ReferenceOwner::Bus(id) => to["bus"] = json!(id),
            ReferenceOwner::Master => to["master"] = json!(true),
        }
        let mut route = json!({ "from": from, "to": to });
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
    Ok(result)
}

/// The Score owner of an effect device.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ReferenceOwner {
    Instrument(String),
    Kit(String),
    Bus(String),
    Master,
}

fn effect_reference(export: &Export<'_>, device_id: &str) -> Option<(ReferenceOwner, String)> {
    for track in &export.tracks {
        if let Some(effect) = track
            .track
            .effects
            .iter()
            .find(|effect| effect.id == device_id)
        {
            let owner = match &track.role {
                TrackRole::Instrument => ReferenceOwner::Instrument(track.id.clone()),
                TrackRole::Kit(kit) => ReferenceOwner::Kit(kit.clone()),
                TrackRole::Audio => return None,
            };
            let id = effect_id(&effect.name, &track.track.effects, effect);
            return Some((owner, id));
        }
    }
    for bus in &export.session.arrangement.mixer.buses {
        if let Some(effect) = bus.effects.iter().find(|effect| effect.id == device_id) {
            let id = effect_id(&effect.name, &bus.effects, effect);
            return Some((ReferenceOwner::Bus(bus.name.clone()), id));
        }
    }
    export
        .session
        .arrangement
        .mixer
        .master_effects
        .iter()
        .find(|effect| effect.id == device_id)
        .map(|effect| {
            (
                ReferenceOwner::Master,
                effect_id(
                    &effect.name,
                    &export.session.arrangement.mixer.master_effects,
                    effect,
                ),
            )
        })
}

/// Returns the Score ID assigned to one effect in its chain.
fn effect_id(
    name: &str,
    chain: &[riffra_core::EffectDevice],
    effect: &riffra_core::EffectDevice,
) -> String {
    let mut used = BTreeSet::new();
    for candidate in chain {
        let id = unique_id(&candidate.name, "effect", &mut used);
        if candidate.id == effect.id {
            return id;
        }
    }
    let _ = name;
    "effect".into()
}

/// Builds the score automation declarations from canonical lanes.
fn automation(export: &mut Export<'_>) -> Result<Vec<Value>, ExportError> {
    let lanes = export.session.arrangement.automation_lanes.clone();
    let mut result = Vec::new();
    for lane in &lanes {
        let Some(target) = automation_target(export, lane)? else {
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
        let mut groups = BTreeMap::<(usize, usize), Vec<Value>>::new();
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
            groups
                .entry((section, occurrence))
                .or_default()
                .push(json!({
                    "at": section_ref.position(point.tick.0, export.ppq),
                    "value": point.value + offset,
                }));
        }
        for ((section, occurrence), mut points) in groups {
            points.sort_by_key(|point| {
                (
                    point["at"]["bar"].as_u64().unwrap_or(0),
                    point["at"]["step"].as_u64().unwrap_or(0),
                    point["at"]["offset_quarters"].as_str().map(str::to_owned),
                )
            });
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
    Ok(result)
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
    lane: &AutomationLane,
) -> Result<Option<Value>, ExportError> {
    let target = match &lane.target {
        AutomationTarget::TrackMix {
            track_id,
            parameter,
        } => {
            let Some(id) = track_score_id(export, track_id) else {
                return Ok(None);
            };
            let owner = if let Some(kit) = kit_of(export, track_id) {
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
            let Some(bus) = export
                .session
                .arrangement
                .mixer
                .buses
                .iter()
                .find(|bus| bus.id == *bus_id)
            else {
                return Ok(None);
            };
            let mut used = BTreeSet::new();
            let mut id = String::new();
            for candidate in &export.session.arrangement.mixer.buses {
                let current = unique_id(&candidate.name, "bus", &mut used);
                if candidate.id == bus.id {
                    id = current;
                    break;
                }
            }
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
                return Ok(None);
            };
            let Some(instrument) = track_score_id(export, &send.track_id) else {
                return Ok(None);
            };
            let Some(bus) = export
                .session
                .arrangement
                .mixer
                .buses
                .iter()
                .find(|bus| bus.id == send.return_bus_id)
            else {
                return Ok(None);
            };
            let mut used = BTreeSet::new();
            let mut id = String::new();
            for candidate in &export.session.arrangement.mixer.buses {
                let current = unique_id(&candidate.name, "bus", &mut used);
                if candidate.id == bus.id {
                    id = current;
                    break;
                }
            }
            json!({"send": {"instrument": instrument, "bus": id}})
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
                return Ok(None);
            };
            let Some(from) = track_score_id(export, &send.source_track_id) else {
                return Ok(None);
            };
            let Some((owner, effect)) = effect_reference(export, &send.target_device_id) else {
                return Ok(None);
            };
            let mut to = json!({"effect": effect});
            match owner {
                ReferenceOwner::Instrument(id) => to["instrument"] = json!(id),
                ReferenceOwner::Kit(id) => to["kit"] = json!(id),
                ReferenceOwner::Bus(id) => to["bus"] = json!(id),
                ReferenceOwner::Master => to["master"] = json!(true),
            }
            json!({"sidechain": {"from": from, "to": to}})
        }
        AutomationTarget::DeviceParameter {
            device_id,
            parameter_id,
        } => {
            let Some(name) = parameter_name(export, device_id, parameter_id)? else {
                omit(
                    &mut export.omissions,
                    format!("automation \"{}\"", lane.id),
                    "the plugin parameter name is not available",
                );
                return Ok(None);
            };
            let Some((owner, effect)) = effect_reference(export, device_id) else {
                return Ok(None);
            };
            let mut effect_ref = json!({"id": effect});
            match owner {
                ReferenceOwner::Instrument(id) => effect_ref["instrument"] = json!(id),
                ReferenceOwner::Kit(id) => effect_ref["kit"] = json!(id),
                ReferenceOwner::Bus(id) => effect_ref["bus"] = json!(id),
                ReferenceOwner::Master => effect_ref["master"] = json!(true),
            }
            json!({"effect": effect_ref, "parameter": name})
        }
    };
    Ok(Some(target))
}

/// Resolves a stable plugin parameter ID to its Score selector.
fn parameter_name(
    export: &mut Export<'_>,
    device_id: &str,
    parameter_id: &str,
) -> Result<Option<String>, ExportError> {
    let plugin = export
        .session
        .arrangement
        .tracks
        .iter()
        .flat_map(|track| {
            track
                .instrument
                .as_ref()
                .and_then(|instrument| instrument.as_vst3())
                .map(|plugin| (instrument_key(track), plugin))
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
    let Some((key, plugin)) = plugin else {
        return Ok(None);
    };
    let parameters = if let Some(cached) = export.parameters.get(&key) {
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
            crate::plugins::validation::describe_parameters(export.scanner, plugin, &[]).map_err(
                |error| ExportError::new(format!("plugin parameters could not be read: {error}")),
            )?;
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
            key.clone(),
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

fn instrument_key(track: &Track) -> String {
    track
        .instrument
        .as_ref()
        .map_or_else(|| track.id.clone(), |instrument| instrument.id.clone())
}

/// Writes audio clips and their files.
fn audio(export: &mut Export<'_>, document: &mut Map<String, Value>) -> Result<(), ExportError> {
    let tracks = export.tracks.clone();
    let mut files = BTreeMap::<riffra_core::AssetId, String>::new();
    let mut by_section = BTreeMap::<usize, Vec<Value>>::new();
    for track in &tracks {
        let TrackRole::Audio = track.role else {
            continue;
        };
        for clip in export
            .session
            .arrangement
            .audio_clips
            .iter()
            .filter(|clip| clip.track_id == track.track.id)
        {
            if !exportable_clip(export, clip) {
                continue;
            }
            let Some(section) = export.sections.iter().position(|section| {
                clip.start_tick.0 >= section.start_tick && clip.start_tick.0 < section.end_tick
            }) else {
                omit(
                    &mut export.omissions,
                    format!("clip \"{}\"", clip.name),
                    "the clip starts outside every section",
                );
                continue;
            };
            let file = match files.get(&clip.asset_id) {
                Some(file) => file.clone(),
                None => {
                    let Some(asset) = crate::asset::load(export.data_root, &clip.asset_id)
                        .map_err(|error| {
                            ExportError::new(format!("asset could not be read: {error}"))
                        })?
                    else {
                        return Err(ExportError::new(format!(
                            "clip \"{}\" references a missing asset",
                            clip.name
                        )));
                    };
                    let Some(location) =
                        crate::asset::resolve_content_location(export.data_root, &clip.asset_id)
                            .map_err(|error| {
                                ExportError::new(format!("asset could not be read: {error}"))
                            })?
                    else {
                        return Err(ExportError::new(format!(
                            "clip \"{}\" references a missing file",
                            clip.name
                        )));
                    };
                    let name = file_identifier(&asset.name);
                    let mut relative = format!("audio/{name}.wav");
                    let mut number = 2;
                    while export.files.contains_key(&relative) {
                        relative = format!("audio/{name}_{number}.wav");
                        number += 1;
                    }
                    let bytes = std::fs::read(&location).map_err(|error| {
                        ExportError::new(format!("could not read {}: {error}", location))
                    })?;
                    add_file(export, relative.clone(), bytes)?;
                    files.insert(clip.asset_id.clone(), relative.clone());
                    relative
                }
            };
            let section_ref = &export.sections[section];
            let rate = f64::from(clip.source_sample_rate);
            let offset_seconds = clip.source_range.start as f64 / rate;
            let duration_seconds = (clip
                .source_range
                .end
                .saturating_sub(clip.source_range.start)) as f64
                / rate;
            let mut declaration = json!({
                "id": format!("clip_{}", identifier_or(&clip.name, "clip")),
                "instrument": track.id,
                "file": file,
                "at": section_ref.position(clip.start_tick.0, export.ppq),
                "offset_seconds": offset_seconds,
                "duration_seconds": duration_seconds,
                "gain_db": clip.gain_db,
                "pan": clip.pan,
                "fade_in_seconds": clip.fade_in.frames as f64 / f64::from(clip.fade_in.sample_rate),
                "fade_out_seconds": clip.fade_out.frames as f64 / f64::from(clip.fade_out.sample_rate),
            });
            if declaration["fade_in_seconds"].as_f64() == Some(0.0) {
                declaration
                    .as_object_mut()
                    .expect("object")
                    .remove("fade_in_seconds");
            }
            if declaration["fade_out_seconds"].as_f64() == Some(0.0) {
                declaration
                    .as_object_mut()
                    .expect("object")
                    .remove("fade_out_seconds");
            }
            by_section.entry(section).or_default().push(declaration);
        }
    }
    for (section, clips) in by_section {
        let name = export.sections[section].name.clone();
        let mut body = document["sections"][&name]
            .as_object()
            .cloned()
            .unwrap_or_default();
        body.insert("audio_clips".into(), Value::Array(clips));
        document["sections"][&name] = Value::Object(body);
    }
    Ok(())
}

/// Reports whether one audio clip can be written and reports the rest.
fn exportable_clip(export: &mut Export<'_>, clip: &AudioClip) -> bool {
    if clip.muted {
        omit(
            &mut export.omissions,
            format!("clip \"{}\"", clip.name),
            "the clip is muted",
        );
        return false;
    }
    if clip.loop_enabled {
        omit(
            &mut export.omissions,
            format!("clip \"{}\"", clip.name),
            "the clip loops",
        );
        return false;
    }
    if clip.take_variant != riffra_core::AudioTakeVariant::Raw {
        omit(
            &mut export.omissions,
            format!("clip \"{}\"", clip.name),
            "the clip uses a processed take",
        );
        return false;
    }
    true
}

fn identifier_or(text: &str, fallback: &str) -> String {
    super::identifier(text).unwrap_or_else(|| fallback.into())
}
