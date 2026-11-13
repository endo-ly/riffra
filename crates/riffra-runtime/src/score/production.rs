use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{Error, Result, array, fields, integer, number, object, string};
use crate::api::params::PluginStatePatchOperation;

pub(crate) mod mix;

#[derive(Debug, Clone)]
pub(crate) struct SourceSpec {
    pub(crate) library: Option<String>,
    pub(crate) vst3: Option<Vst3Spec>,
    pub(crate) velocity: Option<Vec<(u8, u8)>>,
}

#[derive(Debug, Clone)]
pub(crate) struct Vst3Spec {
    pub(crate) plugin: Option<String>,
    pub(crate) identifier: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) patch: Vec<PluginStatePatchOperation>,
    pub(crate) params: BTreeMap<String, ParameterValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParameterValue {
    Normalized(f64),
    Display(String),
}

#[derive(Debug, Clone)]
pub(crate) struct EffectSpec {
    pub(crate) id: String,
    pub(crate) library: Option<String>,
    pub(crate) vst3: Option<Vst3Spec>,
    pub(crate) bypassed: Option<bool>,
}

#[derive(Debug, Clone)]
pub(crate) struct KitSpec {
    pub(crate) source: SourceSpec,
    pub(crate) effects: Vec<EffectSpec>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Bar {
    Number(usize),
    Last,
}

#[derive(Debug, Clone)]
pub(crate) struct Position {
    pub(crate) bar: Bar,
    pub(crate) offset: num_rational::BigRational,
}

#[derive(Debug, Clone)]
pub(crate) struct AudioClipSpec {
    pub(crate) id: String,
    pub(crate) instrument: String,
    pub(crate) file: String,
    pub(crate) at: Position,
    pub(crate) offset_seconds: f64,
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) gain_db: f64,
    pub(crate) pan: f64,
    pub(crate) fade_in_seconds: f64,
    pub(crate) fade_out_seconds: f64,
}

fn position(value: &Value, path: &str) -> Result<Position> {
    fields(value, &["bar", "step", "offset_quarters"], path)?;
    if value.get("step").is_some() && value.get("offset_quarters").is_some() {
        return Err(Error::new(
            path,
            "step and offset_quarters are mutually exclusive",
        ));
    }
    let bar = super::required(value, "bar", path)?;
    let bar = if bar == "last" {
        Bar::Last
    } else {
        let n = usize::try_from(integer(bar, &format!("{path}.bar"))?)
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| Error::new(format!("{path}.bar"), "bar must be positive"))?;
        Bar::Number(n)
    };
    let offset = if let Some(offset) = value.get("offset_quarters") {
        super::time::rational(offset, &format!("{path}.offset_quarters"))?
    } else {
        let step = super::optional_int(value, "step", 1, path)?;
        if step < 1 {
            return Err(Error::new(format!("{path}.step"), "step must be positive"));
        }
        super::q(step - 1, 4)
    };
    if offset < super::q(0, 1) {
        return Err(Error::new(path, "position must be nonnegative"));
    }
    Ok(Position { bar, offset })
}

pub(super) fn parse_audio_clips(value: &Value, path: &str) -> Result<Vec<AudioClipSpec>> {
    let mut ids = BTreeSet::new();
    let mut clips = Vec::new();
    for (index, value) in array(value, path)?.iter().enumerate() {
        let p = format!("{path}[{index}]");
        fields(
            value,
            &[
                "id",
                "instrument",
                "file",
                "at",
                "offset_seconds",
                "duration_seconds",
                "gain_db",
                "pan",
                "fade_in_seconds",
                "fade_out_seconds",
            ],
            &p,
        )?;
        let required_string = |key| {
            optional_string(value, key, &p)?
                .ok_or_else(|| Error::new(format!("{p}.{key}"), "required field is missing"))
        };
        let id = required_string("id")?;
        if !ids.insert(id.clone()) {
            return Err(Error::new(
                format!("{p}.id"),
                "audio clip id must be unique in its section",
            ));
        }
        let instrument = required_string("instrument")?;
        let file = required_string("file")?;
        let at = position(super::required(value, "at", &p)?, &format!("{p}.at"))?;
        let offset_seconds = super::optional_number(value, "offset_seconds", 0.0, &p)?;
        let duration_seconds = value
            .get("duration_seconds")
            .map(|v| number(v, &format!("{p}.duration_seconds")))
            .transpose()?;
        let gain_db = super::optional_number(value, "gain_db", 0.0, &p)?;
        let pan = super::optional_number(value, "pan", 0.0, &p)?;
        let fade_in_seconds = super::optional_number(value, "fade_in_seconds", 0.0, &p)?;
        let fade_out_seconds = super::optional_number(value, "fade_out_seconds", 0.0, &p)?;
        for (key, value) in [
            ("offset_seconds", offset_seconds),
            ("fade_in_seconds", fade_in_seconds),
            ("fade_out_seconds", fade_out_seconds),
        ] {
            if value < 0.0 {
                return Err(Error::new(
                    format!("{p}.{key}"),
                    "expected a nonnegative duration",
                ));
            }
        }
        if duration_seconds.is_some_and(|v| v <= 0.0) {
            return Err(Error::new(
                format!("{p}.duration_seconds"),
                "duration must be positive",
            ));
        }
        if duration_seconds.is_some_and(|v| fade_in_seconds > v || fade_out_seconds > v) {
            return Err(Error::new(&p, "fade must not exceed duration"));
        }
        if !(-90.0..=24.0).contains(&gain_db) {
            return Err(Error::new(
                format!("{p}.gain_db"),
                "expected gain -90..24 dB",
            ));
        }
        if !(-1.0..=1.0).contains(&pan) {
            return Err(Error::new(format!("{p}.pan"), "expected pan -1..1"));
        }
        clips.push(AudioClipSpec {
            id,
            instrument,
            file,
            at,
            offset_seconds,
            duration_seconds,
            gain_db,
            pan,
            fade_in_seconds,
            fade_out_seconds,
        });
    }
    Ok(clips)
}

pub(super) fn parse_kits(value: &Value) -> Result<BTreeMap<String, KitSpec>> {
    let mut kits = BTreeMap::new();
    for (id, value) in object(value, "kits")? {
        let path = format!("kits.{id}");
        if id.is_empty() {
            return Err(Error::new(path, "kit id must not be empty"));
        }
        fields(value, &["source", "effects"], &path)?;
        let source = parse_source(
            super::required(value, "source", &path)?,
            &format!("{path}.source"),
        )?;
        let effects = value
            .get("effects")
            .map(|v| parse_effects(v, &format!("{path}.effects")))
            .transpose()?
            .unwrap_or_default();
        kits.insert(id.clone(), KitSpec { source, effects });
    }
    Ok(kits)
}

pub(super) fn parse_effects(value: &Value, path: &str) -> Result<Vec<EffectSpec>> {
    let mut ids = BTreeSet::new();
    let mut effects = Vec::new();
    for (index, value) in array(value, path)?.iter().enumerate() {
        let item_path = format!("{path}[{index}]");
        fields(value, &["id", "use", "vst3", "bypassed"], &item_path)?;
        let id = super::text_field(value, "id", &item_path)?;
        if id.is_empty() || !ids.insert(id.to_owned()) {
            return Err(Error::new(
                format!("{item_path}.id"),
                "effect id must be nonempty and unique in its chain",
            ));
        }
        let library = library_name(value, &item_path)?;
        let vst3 = value
            .get("vst3")
            .map(|v| parse_vst3(v, &format!("{item_path}.vst3")))
            .transpose()?;
        if library.is_none() && vst3.is_none() {
            return Err(Error::new(&item_path, "effect requires use or vst3"));
        }
        validate_vst3(&library, &vst3, &item_path)?;
        let bypassed = value
            .get("bypassed")
            .map(|v| {
                v.as_bool().ok_or_else(|| {
                    Error::new(format!("{item_path}.bypassed"), "expected a boolean")
                })
            })
            .transpose()?;
        effects.push(EffectSpec {
            id: id.to_owned(),
            library,
            vst3,
            bypassed,
        });
    }
    Ok(effects)
}

fn optional_string(value: &Value, key: &str, path: &str) -> Result<Option<String>> {
    value
        .get(key)
        .map(|value| {
            let path = format!("{path}.{key}");
            let text = string(value, &path)?;
            if text.is_empty() {
                return Err(Error::new(path, "expected a nonempty string"));
            }
            Ok(text.to_owned())
        })
        .transpose()
}

fn library_name(value: &Value, path: &str) -> Result<Option<String>> {
    let name = optional_string(value, "use", path)?;
    if let Some(name) = &name {
        let mut bytes = name.bytes();
        if !bytes
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            || !bytes.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err(Error::new(
                format!("{path}.use"),
                "library name must use lowercase letters, digits, and hyphens",
            ));
        }
    }
    Ok(name)
}

pub(super) fn parse_source(value: &Value, path: &str) -> Result<SourceSpec> {
    for key in ["voisona", "articulations"] {
        if value.get(key).is_some() {
            let mut error = Error::new(
                format!("{path}.{key}"),
                format!("source {key} is not implemented by Riffra"),
            );
            error.kind = Some("notImplemented");
            return Err(error);
        }
    }
    fields(value, &["use", "vst3", "velocity"], path)?;
    let library = library_name(value, path)?;
    let vst3 = value
        .get("vst3")
        .map(|v| parse_vst3(v, &format!("{path}.vst3")))
        .transpose()?;
    if library.is_none() && vst3.is_none() {
        return Err(Error::new(path, "source requires use or vst3"));
    }
    let velocity = value
        .get("velocity")
        .map(|v| parse_velocity(v, &format!("{path}.velocity")))
        .transpose()?;
    validate_vst3(&library, &vst3, path)?;
    Ok(SourceSpec {
        library,
        vst3,
        velocity,
    })
}

fn validate_vst3(library: &Option<String>, vst3: &Option<Vst3Spec>, path: &str) -> Result<()> {
    if let Some(vst3) = vst3 {
        if library.is_none() && vst3.plugin.is_none() {
            return Err(Error::new(
                format!("{path}.vst3.plugin").trim_start_matches('.'),
                "plugin is required without a library reference",
            ));
        }
        if library.is_none() && vst3.state.is_none() && !vst3.patch.is_empty() {
            return Err(Error::new(
                format!("{path}.vst3.patch").trim_start_matches('.'),
                "patch requires state",
            ));
        }
    }
    Ok(())
}

fn parse_vst3(value: &Value, path: &str) -> Result<Vst3Spec> {
    fields(
        value,
        &["plugin", "identifier", "state", "patch", "params"],
        path,
    )?;
    let plugin = optional_string(value, "plugin", path)?;
    let identifier = optional_string(value, "identifier", path)?;
    let state = optional_string(value, "state", path)?;
    let mut patch = Vec::new();
    if let Some(operations) = value.get("patch") {
        for (index, operation) in array(operations, &format!("{path}.patch"))?
            .iter()
            .enumerate()
        {
            patch.push(serde_json::from_value(operation.clone()).map_err(|error| {
                Error::new(format!("{path}.patch[{index}]"), error.to_string())
            })?);
        }
    }
    let mut params = BTreeMap::new();
    if let Some(values) = value.get("params") {
        for (key, value) in object(values, &format!("{path}.params"))? {
            let param_path = format!("{path}.params.{key}");
            if key.is_empty() {
                return Err(Error::new(param_path, "parameter name must not be empty"));
            }
            let parameter = if value.is_object() {
                fields(value, &["display"], &param_path)?;
                ParameterValue::Display(
                    super::text_field(value, "display", &param_path)?.to_owned(),
                )
            } else {
                let normalized = number(value, &param_path)?;
                if !(0.0..=1.0).contains(&normalized) {
                    return Err(Error::new(param_path, "expected normalized value 0..1"));
                }
                ParameterValue::Normalized(normalized)
            };
            params.insert(key.clone(), parameter);
        }
    }
    Ok(Vst3Spec {
        plugin,
        identifier,
        state,
        patch,
        params,
    })
}

fn parse_velocity(value: &Value, path: &str) -> Result<Vec<(u8, u8)>> {
    let mut points = Vec::new();
    for (index, point) in array(value, path)?.iter().enumerate() {
        let point_path = format!("{path}[{index}]");
        let pair = array(point, &point_path)?;
        if pair.len() != 2 {
            return Err(Error::new(
                point_path,
                "expected an input and output velocity",
            ));
        }
        let mut values = [0_u8; 2];
        for (i, item) in pair.iter().enumerate() {
            let item_path = format!("{point_path}[{i}]");
            values[i] = u8::try_from(integer(item, &item_path)?)
                .ok()
                .filter(|v| (1..=127).contains(v))
                .ok_or_else(|| Error::new(item_path, "expected velocity 1..127"))?;
        }
        if points.last().is_some_and(|(input, _)| *input >= values[0]) {
            return Err(Error::new(point_path, "input velocities must increase"));
        }
        points.push((values[0], values[1]));
    }
    if points.len() < 2
        || points.first().map(|p| p.0) != Some(1)
        || points.last().map(|p| p.0) != Some(127)
    {
        return Err(Error::new(
            path,
            "velocity map must start at 1 and end at 127",
        ));
    }
    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_values_are_typed_and_invalid_fields_are_located() {
        let value = json!({
            "use": "bass",
            "vst3": {"params": {"Mix": 0.5, "Mode": {"display": "Wide"}}},
            "velocity": [[1, 18], [64, 68], [127, 120]]
        });
        let source = parse_source(&value, "instruments.bass.source").unwrap();
        assert_eq!(source.library.as_deref(), Some("bass"));
        assert_eq!(source.velocity.unwrap()[1], (64, 68));
        let params = source.vst3.unwrap().params;
        assert!(matches!(params["Mix"], ParameterValue::Normalized(0.5)));
        assert!(matches!(&params["Mode"], ParameterValue::Display(value) if value == "Wide"));

        for (replacement, path) in [
            (
                json!({"vst3":{"plugin":"synth", "params":{"Mix":1.2}}}),
                "instruments.bass.source.vst3.params.Mix",
            ),
            (
                json!({"use":"bass", "velocity":[[1, 1], [1, 2], [127, 127]]}),
                "instruments.bass.source.velocity[1]",
            ),
            (
                json!({"use":"bass", "unexpected":true}),
                "instruments.bass.source.unexpected",
            ),
        ] {
            assert_eq!(
                parse_source(&replacement, "instruments.bass.source")
                    .unwrap_err()
                    .path,
                path
            );
        }
    }

    #[test]
    fn effects_require_unique_ids_and_resolvable_sources() {
        let effects = json!([
            {"id":"eq", "use":"eq-template"},
            {"id":"compressor", "vst3":{"plugin":"Compressor"}, "bypassed":true}
        ]);
        let parsed = parse_effects(&effects, "instruments.bass.effects").unwrap();
        assert_eq!(parsed[0].id, "eq");
        assert_eq!(parsed[0].library.as_deref(), Some("eq-template"));
        assert_eq!(
            parsed[1].vst3.as_ref().unwrap().plugin.as_deref(),
            Some("Compressor")
        );
        assert_eq!(parsed[1].bypassed, Some(true));

        let duplicate = json!([{"id":"eq", "use":"a"}, {"id":"eq", "use":"b"}]);
        assert_eq!(
            parse_effects(&duplicate, "effects").unwrap_err().path,
            "effects[1].id"
        );
        let missing = json!([{"id":"eq"}]);
        assert_eq!(
            parse_effects(&missing, "effects").unwrap_err().path,
            "effects[0]"
        );
    }
}
