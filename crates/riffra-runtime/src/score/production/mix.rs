use std::collections::BTreeMap;

use serde_json::Value;

use super::{EffectSpec, ParameterValue, Position, optional_string, parse_effects, position};
use crate::score::{
    Error, Result, array, fields, integer, number, object, optional_number, required, string,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BusKind {
    Group,
    Return,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SendPosition {
    Pre,
    Post,
}

#[derive(Debug, Clone)]
pub(crate) struct SendSpec {
    pub(crate) level_db: f64,
    pub(crate) position: SendPosition,
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TrackMix {
    pub(crate) gain_db: Option<f64>,
    pub(crate) pan: Option<f64>,
    pub(crate) output: Option<String>,
    pub(crate) sends: BTreeMap<String, SendSpec>,
}

#[derive(Debug, Clone)]
pub(crate) struct Bus {
    pub(crate) kind: BusKind,
    pub(crate) gain_db: f64,
    pub(crate) pan: f64,
    pub(crate) effects: Vec<EffectSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Owner {
    Instrument(String),
    Kit(String),
    Bus(String),
    Master,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct EffectRef {
    pub(crate) owner: Owner,
    pub(crate) id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Sidechain {
    pub(crate) from: String,
    pub(crate) to: EffectRef,
    pub(crate) send: SendSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AutomationTarget {
    Track {
        owner: Owner,
        parameter: String,
    },
    Bus {
        id: String,
        parameter: String,
    },
    Master,
    Send {
        instrument: String,
        bus: String,
    },
    Sidechain {
        from: String,
        to: EffectRef,
    },
    Effect {
        effect: EffectRef,
        parameter: String,
    },
    Source {
        instrument: String,
        parameter: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interpolation {
    Linear,
    Step,
}

#[derive(Debug, Clone)]
pub(crate) struct Point {
    pub(crate) at: Position,
    pub(crate) value: ParameterValue,
}

#[derive(Debug, Clone)]
pub(crate) struct Automation {
    pub(crate) target: AutomationTarget,
    pub(crate) section: String,
    pub(crate) occurrence: Option<usize>,
    pub(crate) interpolation: Interpolation,
    pub(crate) points: Vec<Point>,
}

#[derive(Debug, Clone)]
pub(crate) struct Mix {
    pub(crate) tracks: BTreeMap<String, TrackMix>,
    pub(crate) buses: BTreeMap<String, Bus>,
    pub(crate) sidechain: Vec<Sidechain>,
    pub(crate) automation: Vec<Automation>,
    pub(crate) master_gain_db: f64,
    pub(crate) master_effects: Vec<EffectSpec>,
}

fn text(value: &Value, key: &str, path: &str) -> Result<String> {
    optional_string(value, key, path)?
        .ok_or_else(|| Error::new(format!("{path}.{key}"), "required field is missing"))
}

fn gain(value: f64, path: &str) -> Result<f64> {
    if super::super::gain_linear(value).is_none_or(|gain| gain > f64::from(f32::MAX)) {
        return Err(Error::new(path, "gain exceeds the audio engine range"));
    }
    Ok(value)
}

fn pan(value: f64, path: &str) -> Result<f64> {
    if !(-1.0..=1.0).contains(&value) {
        return Err(Error::new(path, "expected pan -1..1"));
    }
    Ok(value)
}

fn send(value: &Value, path: &str, scalar: bool) -> Result<SendSpec> {
    if scalar && value.is_number() {
        return Ok(SendSpec {
            level_db: gain(number(value, path)?, path)?,
            position: SendPosition::Post,
            enabled: true,
        });
    }
    if scalar {
        fields(value, &["level_db", "position", "enabled"], path)?;
    }
    let level_db = gain(
        optional_number(value, "level_db", 0.0, path)?,
        &format!("{path}.level_db"),
    )?;
    let position = match value
        .get("position")
        .map(|v| string(v, &format!("{path}.position")))
        .transpose()?
    {
        None | Some("post") => SendPosition::Post,
        Some("pre") => SendPosition::Pre,
        Some(_) => {
            return Err(Error::new(
                format!("{path}.position"),
                "expected pre or post",
            ));
        }
    };
    let enabled = value
        .get("enabled")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| Error::new(format!("{path}.enabled"), "expected a boolean"))
        })
        .transpose()?
        .unwrap_or(true);
    Ok(SendSpec {
        level_db,
        position,
        enabled,
    })
}

fn effects(value: &Value, path: &str) -> Result<Vec<EffectSpec>> {
    value
        .get("effects")
        .map(|v| parse_effects(v, &format!("{path}.effects")))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn owner(value: &Value, path: &str, id_key: &str) -> Result<EffectRef> {
    fields(value, &["instrument", "kit", "bus", "master", id_key], path)?;
    let mut selected = Vec::new();
    for key in ["instrument", "kit", "bus"] {
        if let Some(id) = optional_string(value, key, path)? {
            selected.push(match key {
                "instrument" => Owner::Instrument(id),
                "kit" => Owner::Kit(id),
                _ => Owner::Bus(id),
            });
        }
    }
    if let Some(master) = value.get("master") {
        if master != &Value::Bool(true) {
            return Err(Error::new(format!("{path}.master"), "master must be true"));
        }
        selected.push(Owner::Master);
    }
    if selected.len() != 1 {
        return Err(Error::new(path, "expected exactly one effect owner"));
    }
    Ok(EffectRef {
        owner: selected.remove(0),
        id: text(value, id_key, path)?,
    })
}

pub(in crate::score) fn parse(value: &Value) -> Result<Mix> {
    fields(
        value,
        &["tracks", "buses", "sidechain", "automation", "master"],
        "mix",
    )?;
    let mut buses = BTreeMap::new();
    if let Some(values) = value.get("buses") {
        for (id, value) in object(values, "mix.buses")? {
            let path = format!("mix.buses.{id}");
            if id.is_empty() || id == "master" {
                return Err(Error::new(
                    &path,
                    "bus id must be nonempty and cannot be master",
                ));
            }
            fields(value, &["kind", "gain_db", "pan", "effects"], &path)?;
            let kind = match text(value, "kind", &path)?.as_str() {
                "group" => BusKind::Group,
                "return" => BusKind::Return,
                _ => {
                    return Err(Error::new(
                        format!("{path}.kind"),
                        "expected group or return",
                    ));
                }
            };
            buses.insert(
                id.clone(),
                Bus {
                    kind,
                    gain_db: gain(
                        optional_number(value, "gain_db", 0.0, &path)?,
                        &format!("{path}.gain_db"),
                    )?,
                    pan: pan(
                        optional_number(value, "pan", 0.0, &path)?,
                        &format!("{path}.pan"),
                    )?,
                    effects: effects(value, &path)?,
                },
            );
        }
    }
    let mut tracks = BTreeMap::new();
    if let Some(values) = value.get("tracks") {
        for (id, value) in object(values, "mix.tracks")? {
            let path = format!("mix.tracks.{id}");
            fields(value, &["gain_db", "pan", "output", "sends"], &path)?;
            let gain_db = value
                .get("gain_db")
                .map(|v| {
                    gain(
                        number(v, &format!("{path}.gain_db"))?,
                        &format!("{path}.gain_db"),
                    )
                })
                .transpose()?;
            let pan = value
                .get("pan")
                .map(|v| pan(number(v, &format!("{path}.pan"))?, &format!("{path}.pan")))
                .transpose()?;
            let output = optional_string(value, "output", &path)?;
            let mut sends = BTreeMap::new();
            if let Some(values) = value.get("sends") {
                for (bus, value) in object(values, &format!("{path}.sends"))? {
                    sends.insert(
                        bus.clone(),
                        send(value, &format!("{path}.sends.{bus}"), true)?,
                    );
                }
            }
            tracks.insert(
                id.clone(),
                TrackMix {
                    gain_db,
                    pan,
                    output,
                    sends,
                },
            );
        }
    }
    let mut sidechain = Vec::new();
    if let Some(values) = value.get("sidechain") {
        for (index, value) in array(values, "mix.sidechain")?.iter().enumerate() {
            let path = format!("mix.sidechain[{index}]");
            fields(
                value,
                &["from", "to", "level_db", "position", "enabled"],
                &path,
            )?;
            sidechain.push(Sidechain {
                from: text(value, "from", &path)?,
                to: owner(
                    required(value, "to", &path)?,
                    &format!("{path}.to"),
                    "effect",
                )?,
                send: send(value, &path, false)?,
            });
        }
    }
    let (master_gain_db, master_effects) = if let Some(master) = value.get("master") {
        fields(master, &["gain_db", "effects"], "mix.master")?;
        (
            gain(
                optional_number(master, "gain_db", 0.0, "mix.master")?,
                "mix.master.gain_db",
            )?,
            effects(master, "mix.master")?,
        )
    } else {
        (0.0, Vec::new())
    };
    let automation = value
        .get("automation")
        .map(parse_automation)
        .transpose()?
        .unwrap_or_default();
    Ok(Mix {
        tracks,
        buses,
        sidechain,
        automation,
        master_gain_db,
        master_effects,
    })
}

fn target(value: &Value, path: &str) -> Result<AutomationTarget> {
    fields(
        value,
        &[
            "instrument",
            "kit",
            "bus",
            "master",
            "send",
            "sidechain",
            "effect",
            "source",
            "parameter",
        ],
        path,
    )?;
    let selectors = [
        "instrument",
        "kit",
        "bus",
        "master",
        "send",
        "sidechain",
        "effect",
        "source",
    ];
    let selected: Vec<_> = selectors
        .into_iter()
        .filter(|key| value.get(key).is_some())
        .collect();
    if selected.len() != 1 {
        return Err(Error::new(path, "expected exactly one automation target"));
    }
    let key = selected[0];
    let parameter = || text(value, "parameter", path);
    let result = match key {
        "instrument" | "kit" => {
            let id = text(value, key, path)?;
            let parameter = parameter()?;
            if !["volume", "pan"].contains(&parameter.as_str()) {
                return Err(Error::new(
                    format!("{path}.parameter"),
                    "expected volume or pan",
                ));
            }
            AutomationTarget::Track {
                owner: if key == "kit" {
                    Owner::Kit(id)
                } else {
                    Owner::Instrument(id)
                },
                parameter,
            }
        }
        "bus" => {
            let id = text(value, "bus", path)?;
            let parameter = parameter()?;
            if !["gain", "pan"].contains(&parameter.as_str()) {
                return Err(Error::new(
                    format!("{path}.parameter"),
                    "expected gain or pan",
                ));
            }
            AutomationTarget::Bus { id, parameter }
        }
        "master" => {
            if value["master"] != true {
                return Err(Error::new(format!("{path}.master"), "master must be true"));
            }
            if parameter()? != "gain" {
                return Err(Error::new(format!("{path}.parameter"), "expected gain"));
            }
            AutomationTarget::Master
        }
        "send" => {
            if value.get("parameter").is_some() {
                return Err(Error::new(
                    format!("{path}.parameter"),
                    "send target has no parameter field",
                ));
            }
            let send_path = format!("{path}.send");
            let send = &value["send"];
            fields(send, &["instrument", "bus"], &send_path)?;
            AutomationTarget::Send {
                instrument: text(send, "instrument", &send_path)?,
                bus: text(send, "bus", &send_path)?,
            }
        }
        "sidechain" => {
            if value.get("parameter").is_some() {
                return Err(Error::new(
                    format!("{path}.parameter"),
                    "sidechain target has no parameter field",
                ));
            }
            let sidechain_path = format!("{path}.sidechain");
            let sidechain = &value["sidechain"];
            fields(sidechain, &["from", "to"], &sidechain_path)?;
            AutomationTarget::Sidechain {
                from: text(sidechain, "from", &sidechain_path)?,
                to: owner(
                    required(sidechain, "to", &sidechain_path)?,
                    &format!("{sidechain_path}.to"),
                    "effect",
                )?,
            }
        }
        "effect" => AutomationTarget::Effect {
            effect: owner(&value["effect"], &format!("{path}.effect"), "id")?,
            parameter: parameter()?,
        },
        "source" => AutomationTarget::Source {
            instrument: text(value, "source", path)?,
            parameter: parameter()?,
        },
        _ => unreachable!(),
    };
    Ok(result)
}

fn parse_automation(value: &Value) -> Result<Vec<Automation>> {
    let mut automations = Vec::new();
    for (index, value) in array(value, "mix.automation")?.iter().enumerate() {
        let path = format!("mix.automation[{index}]");
        fields(
            value,
            &["target", "section", "occurrence", "interpolation", "points"],
            &path,
        )?;
        let target = target(required(value, "target", &path)?, &format!("{path}.target"))?;
        let section = text(value, "section", &path)?;
        let occurrence = value
            .get("occurrence")
            .map(|value| {
                usize::try_from(integer(value, &format!("{path}.occurrence"))?)
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        Error::new(format!("{path}.occurrence"), "occurrence must be positive")
                    })
            })
            .transpose()?;
        let interpolation = match value
            .get("interpolation")
            .map(|v| string(v, &format!("{path}.interpolation")))
            .transpose()?
        {
            None | Some("linear") => Interpolation::Linear,
            Some("step") => Interpolation::Step,
            Some(_) => {
                return Err(Error::new(
                    format!("{path}.interpolation"),
                    "expected linear or step",
                ));
            }
        };
        let items = array(required(value, "points", &path)?, &format!("{path}.points"))?;
        if items.is_empty() {
            return Err(Error::new(
                format!("{path}.points"),
                "automation requires at least one point",
            ));
        }
        let mut points = Vec::new();
        for (index, point) in items.iter().enumerate() {
            let point_path = format!("{path}.points[{index}]");
            fields(point, &["at", "value"], &point_path)?;
            let at = position(
                required(point, "at", &point_path)?,
                &format!("{point_path}.at"),
            )?;
            let raw = required(point, "value", &point_path)?;
            let device = matches!(
                target,
                AutomationTarget::Effect { .. } | AutomationTarget::Source { .. }
            );
            let value = if let Some(display) = raw.as_str() {
                if !device {
                    return Err(Error::new(
                        format!("{point_path}.value"),
                        "display values require a plugin parameter target",
                    ));
                }
                ParameterValue::Display(display.to_owned())
            } else {
                let numeric = number(raw, &format!("{point_path}.value"))?;
                if device && !(0.0..=1.0).contains(&numeric) {
                    return Err(Error::new(
                        format!("{point_path}.value"),
                        "expected normalized value 0..1",
                    ));
                }
                if matches!(&target, AutomationTarget::Track { parameter, .. } | AutomationTarget::Bus { parameter, .. } if parameter == "pan")
                {
                    pan(numeric, &format!("{point_path}.value"))?;
                } else if !device {
                    gain(numeric, &format!("{point_path}.value"))?;
                }
                ParameterValue::Normalized(numeric)
            };
            points.push(Point { at, value });
        }
        automations.push(Automation {
            target,
            section,
            occurrence,
            interpolation,
            points,
        });
    }
    Ok(automations)
}

pub(in crate::score) fn validate(plan: &crate::score::ScorePlan) -> Result<()> {
    let Some(mix) = &plan.mix else {
        return Ok(());
    };
    if !(-90.0..=0.0).contains(&mix.master_gain_db) {
        return Err(Error::new(
            "mix.master.gain_db",
            "master gain must be between -90 and 0 dB",
        ));
    }
    let is_track = |id: &str| {
        plan.kits.contains_key(id)
            || (plan.instruments.contains_key(id) && !plan.kit_members.contains_key(id))
    };
    let validate_owner = |owner: &Owner, path: &str| -> Result<()> {
        let exists = match owner {
            Owner::Instrument(id) => {
                plan.instruments.contains_key(id) && !plan.kit_members.contains_key(id)
            }
            Owner::Kit(id) => plan.kits.contains_key(id),
            Owner::Bus(id) => mix.buses.contains_key(id),
            Owner::Master => true,
        };
        if !exists {
            return Err(Error::new(path, "undefined effect owner"));
        }
        Ok(())
    };
    let validate_effect = |effect: &EffectRef, path: &str| -> Result<()> {
        validate_owner(&effect.owner, path)?;
        if let Owner::Instrument(id) = &effect.owner
            && plan.lanes.iter().filter(|lane| lane.id == *id).count() > 1
        {
            return Err(Error::new(path, "effect owner has multiple gain lanes"));
        }
        let chain = match &effect.owner {
            Owner::Instrument(id) => plan
                .production_effects
                .get(id)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            Owner::Kit(id) => &plan.kits[id].effects,
            Owner::Bus(id) => &mix.buses[id].effects,
            Owner::Master => &mix.master_effects,
        };
        if !chain.iter().any(|item| item.id == effect.id) {
            return Err(Error::new(path, "undefined effect id for owner"));
        }
        Ok(())
    };
    for (id, track) in &mix.tracks {
        let path = format!("mix.tracks.{id}");
        if !is_track(id) {
            return Err(Error::new(&path, "undefined mix track"));
        }
        if !plan.kits.contains_key(id) && (track.gain_db.is_some() || track.pan.is_some()) {
            return Err(Error::new(
                &path,
                "gain and pan for an instrument must be specified on the instrument",
            ));
        }
        if let Some(output) = &track.output
            && output != "master"
            && mix
                .buses
                .get(output)
                .is_none_or(|bus| bus.kind != BusKind::Group)
        {
            return Err(Error::new(
                format!("{path}.output"),
                "output must name a group bus or master",
            ));
        }
        for bus in track.sends.keys() {
            if mix
                .buses
                .get(bus)
                .is_none_or(|bus| bus.kind != BusKind::Return)
            {
                return Err(Error::new(
                    format!("{path}.sends.{bus}"),
                    "send must name a return bus",
                ));
            }
        }
    }
    let mut sidechains = std::collections::BTreeSet::new();
    for (index, item) in mix.sidechain.iter().enumerate() {
        let path = format!("mix.sidechain[{index}]");
        if !is_track(&item.from) {
            return Err(Error::new(
                format!("{path}.from"),
                "undefined sidechain source track",
            ));
        }
        validate_effect(&item.to, &format!("{path}.to"))?;
        if matches!(&item.to.owner, Owner::Instrument(id) | Owner::Kit(id) if id == &item.from) {
            return Err(Error::new(
                format!("{path}.to"),
                "sidechain source and target cannot be the same track",
            ));
        }
        if !sidechains.insert((item.from.clone(), item.to.clone())) {
            return Err(Error::new(path, "duplicate sidechain route"));
        }
    }
    let mut interpolation = BTreeMap::new();
    let mut values = BTreeMap::new();
    for (index, automation) in mix.automation.iter().enumerate() {
        let path = format!("mix.automation[{index}]");
        match &automation.target {
            AutomationTarget::Track { owner, parameter } => {
                validate_owner(owner, &format!("{path}.target"))?;
                if let Owner::Instrument(id) = owner
                    && parameter == "volume"
                    && plan
                        .sections
                        .iter()
                        .any(|section| section.part_gains.contains_key(id))
                {
                    return Err(Error::new(
                        format!("{path}.target"),
                        "track volume automation conflicts with part_gains",
                    ));
                }
            }
            AutomationTarget::Bus { id, .. } => {
                if !mix.buses.contains_key(id) {
                    return Err(Error::new(format!("{path}.target.bus"), "undefined bus"));
                }
            }
            AutomationTarget::Master => {}
            AutomationTarget::Send { instrument, bus } => {
                if mix
                    .tracks
                    .get(instrument)
                    .is_none_or(|track| !track.sends.contains_key(bus))
                {
                    return Err(Error::new(format!("{path}.target.send"), "undefined send"));
                }
            }
            AutomationTarget::Sidechain { from, to } => {
                if !sidechains.contains(&(from.clone(), to.clone())) {
                    return Err(Error::new(
                        format!("{path}.target.sidechain"),
                        "undefined sidechain route",
                    ));
                }
            }
            AutomationTarget::Effect { effect, .. } => {
                validate_effect(effect, &format!("{path}.target.effect"))?
            }
            AutomationTarget::Source { instrument, .. } => {
                if !plan.production_sources.contains_key(instrument) {
                    return Err(Error::new(
                        format!("{path}.target.source"),
                        "source target requires a production VST3 instrument",
                    ));
                }
            }
        }
        if let Some(previous) =
            interpolation.insert(automation.target.clone(), automation.interpolation)
            && previous != automation.interpolation
        {
            return Err(Error::new(
                format!("{path}.interpolation"),
                "automation for the same target must use one interpolation",
            ));
        }
        let occurrences: Vec<_> = plan
            .sections
            .iter()
            .filter(|section| section.section == automation.section)
            .collect();
        if occurrences.is_empty() {
            return Err(Error::new(
                format!("{path}.section"),
                "section does not occur in form",
            ));
        }
        if automation.occurrence.is_some_and(|n| n > occurrences.len()) {
            return Err(Error::new(
                format!("{path}.occurrence"),
                "section occurrence is outside form",
            ));
        }
        for (occurrence_index, occurrence) in occurrences.iter().enumerate() {
            if automation
                .occurrence
                .is_some_and(|n| n != occurrence_index + 1)
            {
                continue;
            }
            for (point_index, point) in automation.points.iter().enumerate() {
                if let ParameterValue::Normalized(value) = point.value {
                    let valid = match &automation.target {
                        AutomationTarget::Master => (-90.0..=0.0).contains(&value),
                        AutomationTarget::Send { .. } | AutomationTarget::Sidechain { .. } => {
                            (-90.0..=24.0).contains(&value)
                        }
                        AutomationTarget::Bus { parameter, .. } if parameter == "gain" => {
                            (-90.0..=24.0).contains(&value)
                        }
                        _ => true,
                    };
                    if !valid {
                        return Err(Error::new(
                            format!("{path}.points[{point_index}].value"),
                            "value is outside the parameter range",
                        ));
                    }
                }
                let point_path = format!("{path}.points[{point_index}]");
                let bar = match point.at.bar {
                    super::Bar::Number(n) => n,
                    super::Bar::Last => occurrence.bars,
                };
                if bar > occurrence.bars {
                    return Err(Error::new(
                        format!("{point_path}.at"),
                        "bar is outside the section",
                    ));
                }
                let offset = crate::score::time::tick(&point.at.offset, plan.ticks_per_beat)?;
                let start = occurrence.bar_start_ticks[bar - 1];
                if offset >= occurrence.bar_end_ticks[bar - 1] - start {
                    return Err(Error::new(
                        format!("{point_path}.at"),
                        "position is outside the selected bar",
                    ));
                }
                let key = (automation.target.clone(), start + offset);
                if let ParameterValue::Normalized(value) = point.value
                    && let Some(previous) = values.insert(key, value)
                    && previous != value
                {
                    return Err(Error::new(
                        format!("{point_path}.value"),
                        "conflicting automation values at the same tick",
                    ));
                }
            }
        }
    }
    Ok(())
}
