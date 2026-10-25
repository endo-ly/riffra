pub(crate) mod events;
mod lower;
mod parse;
mod resolve;
pub(super) mod syntax;
mod time;

use std::collections::BTreeMap;
use std::path::Path;

use num_traits::ToPrimitive;
use serde::Serialize;
use serde_json::Value;

use events::{MeterChange, MusicalEvent, PerformanceEvents, TempoChange};
use time::q;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Default)]
struct ErrorContext {
    token_location: Option<(usize, String)>,
    score_position: Option<time::Quarter>,
    section: Option<String>,
    occurrence: Option<usize>,
    bar: Option<usize>,
    step: Option<String>,
    source_bar: Option<usize>,
}

/// A score parsing or expansion error with its resolved source details.
#[derive(Debug, thiserror::Error)]
#[error("{path}: {message}")]
pub(crate) struct Error {
    pub(crate) path: String,
    pub(crate) message: String,
    pub(crate) detail: Box<Value>,
    line: usize,
    column: usize,
    actual: Option<Box<Value>>,
    context: Option<Box<ErrorContext>>,
}

impl Error {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            detail: Box::new(Value::Null),
            path: path.into(),
            message: message.into(),
            line: 0,
            column: 0,
            actual: None,
            context: None,
        }
    }
    fn at(mut self, path: &str) -> Self {
        if self.path.is_empty() {
            self.path = path.into();
        }
        self
    }
    fn with_token_location(mut self, path: &str, offset: usize, token: &str) -> Self {
        self = self.at(path);
        self.context.get_or_insert_with(Box::default).token_location =
            Some((offset, token.to_owned()));
        self
    }
    fn with_score_position(mut self, position: time::Quarter, source_bar: Option<usize>) -> Self {
        let context = self.context.get_or_insert_with(Box::default);
        context.score_position = Some(position);
        context.source_bar = source_bar;
        self
    }
    fn with_score_context(
        mut self,
        section: &str,
        occurrence: usize,
        bar: usize,
        step: String,
    ) -> Self {
        let context = self.context.get_or_insert_with(Box::default);
        context.section = Some(section.to_owned());
        context.occurrence = Some(occurrence);
        context.bar = Some(bar);
        context.step = Some(step);
        context.score_position = None;
        self
    }
    fn take_score_position(&mut self) -> Option<time::Quarter> {
        self.context
            .as_mut()
            .and_then(|context| context.score_position.take())
    }
    fn locate(mut self, source: &Source) -> Self {
        if let Some((offset, token)) = self
            .context
            .as_deref()
            .and_then(|context| context.token_location.as_ref())
            && let Some((line, column)) = source.token_location(&self.path, *offset, token)
        {
            self.line = line;
            self.column = column;
        }
        if self.line == 0 {
            let mut path = self.path.as_str();
            loop {
                if let Some(&(line, column)) = source.positions.get(path) {
                    self.line = line;
                    self.column = column;
                    break;
                }
                let Some(index) = path.rfind(['.', '[']) else {
                    break;
                };
                path = &path[..index];
            }
        }
        let token = self
            .context
            .as_deref()
            .and_then(|context| context.token_location.as_ref())
            .map(|(_, token)| Box::new(Value::String(token.clone())));
        self.actual = self
            .actual
            .or(token)
            .or_else(|| source.values.get(&self.path).cloned().map(Box::new));
        let mut path_context = self.path.as_str();
        let mut bar = self
            .context
            .as_deref()
            .and_then(|context| context.bar)
            .map(Value::from);
        let mut step = self
            .context
            .as_deref()
            .and_then(|context| context.step.clone())
            .map(Value::from);
        loop {
            if bar.is_some() {
                break;
            }
            if let Some(value) = source.values.get(&format!("{path_context}.at.bar")) {
                bar = Some(value.clone());
                step = source
                    .values
                    .get(&format!("{path_context}.at.step"))
                    .cloned();
                break;
            }
            let Some(index) = path_context.rfind(['.', '[']) else {
                break;
            };
            path_context = &path_context[..index];
        }
        let section = self
            .context
            .as_deref()
            .and_then(|context| context.section.clone())
            .or_else(|| {
                self.path
                    .strip_prefix("sections.")
                    .and_then(|s| s.split('.').next())
                    .map(str::to_owned)
            });
        let source_bar = self
            .context
            .as_deref()
            .and_then(|context| context.source_bar);
        let occurrence = self
            .context
            .as_deref()
            .and_then(|context| context.occurrence);
        self.detail = Box::new(
            serde_json::json!({"source_type":"score","source_bar":source_bar,"bar":bar,"step":step,"line":self.line,"column":self.column,"actual":self.actual,"expected":self.message,"section":section,"occurrence":occurrence}),
        );
        self
    }
}

#[derive(Debug, Clone, Default)]
struct Source {
    text: String,
    positions: BTreeMap<String, (usize, usize)>,
    indices: BTreeMap<String, usize>,
    values: BTreeMap<String, Value>,
}

impl Source {
    fn token_location(&self, path: &str, offset: usize, token: &str) -> Option<(usize, usize)> {
        let value = self.values.get(path)?.as_str()?;
        let start = *self.indices.get(path)?;
        let end = self
            .indices
            .values()
            .copied()
            .filter(|index| *index > start)
            .min()
            .unwrap_or(self.text.len());
        let prefix = value.get(..offset)?;
        let occurrence = prefix.match_indices(token).count();
        let raw = self.text.get(start..end)?;
        let raw_offset = raw.match_indices(token).nth(occurrence)?.0;
        let source_prefix = self.text.get(..start + raw_offset)?;
        let line = source_prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = source_prefix
            .rsplit('\n')
            .next()?
            .trim_end_matches('\r')
            .chars()
            .count()
            + 1;
        Some((line, column))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Occurrence {
    pub(crate) section: String,
    pub(crate) bars: usize,
    pub(crate) tonic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) scale: Option<String>,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) bpm: f64,
    pub(crate) meter: String,
    pub(crate) chords: Vec<ChordChange>,
    pub(crate) bar_start_ticks: Vec<u64>,
    pub(crate) bar_end_ticks: Vec<u64>,
    pub(crate) part_gains: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChordChange {
    pub(crate) tick: u64,
    pub(crate) chord: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ScoreLane {
    pub(crate) id: String,
    pub(crate) gain_db: f64,
    pub(crate) pattern: PerformanceEvents,
    pub(crate) sources: Vec<EventSource>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EventSource {
    pub(crate) path: String,
    pub(crate) spelling: Option<String>,
    #[serde(skip_serializing)]
    pub(crate) token_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_bar: Option<usize>,
    pub(crate) tick: u64,
    pub(crate) at_quarters: String,
    pub(crate) duration_quarters: String,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) section: String,
    pub(crate) bar: usize,
    pub(crate) step: String,
    pub(crate) occurrence: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ScorePlan {
    source: Source,
    pub(crate) title: String,
    pub(crate) instruments: serde_json::Map<String, Value>,
    pub(crate) lanes: Vec<ScoreLane>,
    pub(crate) sections: Vec<Occurrence>,
    pub(crate) length_ticks: u64,
    pub(crate) ticks_per_beat: u16,
    pub(crate) master: Option<Value>,
    pub(crate) fade_out_seconds: f64,
    pub(crate) timeline: PerformanceEvents,
    pub(crate) part_windows: Vec<PartWindow>,
}

#[derive(Debug, Clone)]
pub(crate) struct PartWindow {
    pub(crate) instrument: String,
    pub(crate) section_start_tick: u64,
    pub(crate) section_end_tick: u64,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
}

impl ScorePlan {
    pub(crate) fn source_yaml(&self) -> &str {
        &self.source.text
    }

    /// Resolve source metadata for a diagnostic at a score field path.
    pub(crate) fn source_detail(&self, path: &str, message: &str) -> Value {
        *Error::new(path, message).locate(&self.source).detail
    }
}

/// Distinguishes file access failures from errors in the score itself.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ReadError {
    #[error("could not read score: {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Parse(#[from] Error),
}

/// Read and expand a Score YAML file.
///
/// # Errors
/// Returns a file access error or a source-located score error.
pub(crate) fn read(path: &Path) -> std::result::Result<ScorePlan, ReadError> {
    let text = std::fs::read_to_string(path).map_err(|source| ReadError::Io {
        path: path.to_owned(),
        source,
    })?;
    Ok(compile(&text)?)
}

/// Parse, validate, and expand Score YAML into a musical plan.
///
/// # Errors
/// Returns a source-located error for invalid syntax, structure, or musical content.
pub(crate) fn compile(text: &str) -> Result<ScorePlan> {
    let mut source = Source::default();
    let result = parse::parse(text, &mut source).and_then(|document| {
        resolve::validate_structure(&document)?;
        lower::lower(&document)
    });
    result
        .map_err(|error| error.locate(&source))
        .map(|mut plan| {
            for lane in &mut plan.lanes {
                for origin in &mut lane.sources {
                    if let Some((section_index, section)) = plan
                        .sections
                        .iter()
                        .enumerate()
                        .find(|(_, section)| {
                            origin.tick >= section.start_tick && origin.tick < section.end_tick
                        })
                        .or_else(|| {
                            plan.sections
                                .iter()
                                .enumerate()
                                .next_back()
                                .filter(|(_, section)| origin.tick == section.end_tick)
                        })
                    {
                        let (numerator, denominator) =
                            syntax::meter(&section.meter).expect("resolved meter");
                        let elapsed = q(
                            i64::try_from(origin.tick - section.start_tick)
                                .expect("resolved musical time"),
                            i64::from(plan.ticks_per_beat),
                        );
                        let bar_length = q(i64::from(numerator) * 4, i64::from(denominator));
                        let index = (&elapsed / &bar_length)
                            .to_integer()
                            .to_usize()
                            .expect("resolved bar")
                            .min(section.bars - 1);
                        origin.section = section.section.clone();
                        origin.occurrence = section_index + 1;
                        origin.bar = index + 1;
                        origin.step = ((elapsed
                            - bar_length * q(i64::try_from(index).expect("resolved bar"), 1))
                            * q(4, 1)
                            + q(1, 1))
                        .to_string();
                    }
                    let token_location = origin
                        .token_offset
                        .zip(origin.spelling.as_deref())
                        .and_then(|(offset, token)| {
                            source.token_location(&origin.path, offset, token)
                        });
                    if let Some((line, column)) = token_location {
                        origin.line = line;
                        origin.column = column;
                        continue;
                    }
                    let mut path = origin.path.as_str();
                    loop {
                        if let Some(&(line, column)) = source.positions.get(path) {
                            origin.line = line;
                            origin.column = column;
                            break;
                        }
                        let Some(i) = path.rfind(['.', '[']) else {
                            break;
                        };
                        path = &path[..i];
                    }
                }
            }
            plan.source = source;
            plan
        })
}

/// Convert finite decibels to a finite linear gain, returning `None` on overflow.
pub(crate) fn gain_linear(gain_db: f64) -> Option<f64> {
    if !gain_db.is_finite() {
        return None;
    }
    let gain = 10.0_f64.powf(gain_db / 20.0);
    gain.is_finite().then_some(gain)
}

fn object<'a>(v: &'a Value, path: &str) -> Result<&'a serde_json::Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| Error::new(path, "expected a mapping"))
}
fn array<'a>(v: &'a Value, path: &str) -> Result<&'a Vec<Value>> {
    v.as_array()
        .ok_or_else(|| Error::new(path, "expected a sequence"))
}
fn string<'a>(v: &'a Value, path: &str) -> Result<&'a str> {
    v.as_str()
        .ok_or_else(|| Error::new(path, "expected a string"))
}
fn integer(v: &Value, path: &str) -> Result<i64> {
    v.as_i64()
        .ok_or_else(|| Error::new(path, "expected an integer"))
}
fn number(v: &Value, path: &str) -> Result<f64> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| Error::new(path, "expected a finite number"))
}
fn required<'a>(v: &'a Value, key: &str, path: &str) -> Result<&'a Value> {
    v.get(key).ok_or_else(|| {
        Error::new(
            format!("{path}.{key}").trim_start_matches('.'),
            "required field is missing",
        )
    })
}
fn text_field<'a>(v: &'a Value, key: &str, path: &str) -> Result<&'a str> {
    string(required(v, key, path)?, &format!("{path}.{key}"))
}
fn optional_number(v: &Value, key: &str, default: f64, path: &str) -> Result<f64> {
    v.get(key)
        .map_or(Ok(default), |v| number(v, &format!("{path}.{key}")))
}
fn optional_int(v: &Value, key: &str, default: i64, path: &str) -> Result<i64> {
    v.get(key)
        .map_or(Ok(default), |v| integer(v, &format!("{path}.{key}")))
}
fn fields(v: &Value, allowed: &[&str], path: &str) -> Result<()> {
    for key in object(v, path)?.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(Error::new(
                format!("{path}.{key}").trim_start_matches('.'),
                format!("unknown field; expected {}", allowed.join(", ")),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
fn to_yaml(document: &Value) -> std::result::Result<String, yaml_rust2::emitter::EmitError> {
    fn node(value: &Value) -> yaml_rust2::Yaml {
        use yaml_rust2::Yaml;
        match value {
            Value::Null => Yaml::Null,
            Value::Bool(value) => Yaml::Boolean(*value),
            Value::String(value) => Yaml::String(value.clone()),
            Value::Number(value) => value
                .as_i64()
                .map_or_else(|| Yaml::Real(value.to_string()), Yaml::Integer),
            Value::Array(values) => Yaml::Array(values.iter().map(node).collect()),
            Value::Object(values) => Yaml::Hash(
                values
                    .iter()
                    .map(|(key, value)| (Yaml::String(key.clone()), node(value)))
                    .collect(),
            ),
        }
    }
    let mut output = String::new();
    yaml_rust2::YamlEmitter::new(&mut output).dump(&node(document))?;
    output.push('\n');
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::time::{Quarter, q};
    use super::*;
    use serde_json::json;

    fn document() -> serde_json::Value {
        serde_json::json!({"meta":{"title":"Timing","bpm":120,"meter":"4/4","tuning":"12edo","reference_octave":4,"gate":1},"form":[{"section":"phrase","bars":1,"tonic":"C","scale":"ionian"}],"sections":{"phrase":{"lines":{"main":{"instrument":"part","melody":"phrase"}}}},"melodies":{"phrase":["1 - - - | 3 - - - | 5 - - - | 1' - - -"]},"instruments":{"part":{"external":true}}})
    }

    #[test]
    fn exact_placements_preserve_enharmonic_spelling_and_ignore_gate() {
        let mut doc = document();
        doc["meta"]["gate"] = serde_json::json!(0.5);
        doc["form"][0]["tonic"] = serde_json::json!("A");
        doc["form"][0]["scale"] = serde_json::json!("aeolian");
        doc["sections"]["phrase"] = serde_json::json!({"placements":[{"instrument":"part","degree":"#1","at":{"bar":1,"offset_quarters":"1/7"},"duration_quarters":"1/3"},{"instrument":"part","degree":"b2","at":{"bar":1,"offset_quarters":"1/3"},"duration_quarters":"1/3"}]});
        let plan = super::compile(&doc.to_string()).expect("exact placements");
        assert_eq!(plan.ticks_per_beat, 3360);
        let events = &plan.lanes[0].pattern.events;
        assert!(matches!(
            events[0],
            super::MusicalEvent::Note {
                tick: 480,
                duration_ticks: 1120,
                note: 70,
                ..
            }
        ));
        assert!(matches!(
            events[1],
            super::MusicalEvent::Note {
                tick: 1120,
                duration_ticks: 1120,
                note: 70,
                ..
            }
        ));
        assert_eq!(
            plan.lanes[0]
                .sources
                .iter()
                .map(|s| s.spelling.as_deref())
                .collect::<Vec<_>>(),
            [Some("#1"), Some("b2")]
        );
        doc["sections"]["phrase"]["placements"][0]["at"]["offset_quarters"] =
            serde_json::json!("1/32767");
        assert!(
            super::compile(&doc.to_string()).is_err(),
            "unrepresentable PPQ must not quantize"
        );
    }

    #[test]
    fn meter_tuplets_ties_and_partial_bars_share_one_exact_timeline() {
        let mut doc = document();
        doc["meta"]["meter"] = serde_json::json!("6/8");
        doc["meta"]["gate"] = serde_json::json!(0.5);
        doc["melodies"]["phrase"] = serde_json::json!(["[1 2 b3] | - - | . . | 4 - | 5 - | 1' -"]);
        doc["time_changes"] = serde_json::json!([{"at_quarters":"1/3","bpm":90}]);
        let plan = super::compile(&doc.to_string()).expect("6/8 tuplets");
        let events = &plan.lanes[0].pattern.events;
        assert_eq!(plan.length_ticks, 1440);
        assert_eq!(events.len(), 6);
        assert!(matches!(
            events[2],
            super::MusicalEvent::Note {
                tick: 160,
                duration_ticks: 160,
                note: 63,
                ..
            }
        ));
        assert_eq!(plan.timeline.tempo_changes[1].tick, 160);
        doc["meta"]["meter"] = serde_json::json!("3/4");
        doc["melodies"]["phrase"] = serde_json::json!(["1 - - - | - - - - | . . . ."]);
        let plan = super::compile(&doc.to_string()).expect("3/4 tie");
        assert!(matches!(
            plan.lanes[0].pattern.events[0],
            super::MusicalEvent::Note {
                duration_ticks: 480,
                ..
            }
        ));
        doc["form"][0]["last_bar_quarters"] = serde_json::json!("2");
        doc["melodies"]["phrase"] = serde_json::json!(["1 - - - | - - - -"]);
        assert_eq!(
            super::compile(&doc.to_string())
                .expect("partial bar")
                .length_ticks,
            960
        );
    }

    #[test]
    fn invalid_scores_identify_the_source_instead_of_guessing() {
        let good = document();
        let mut unknown = good.clone();
        unknown["meta"]["key"] = serde_json::json!("C");
        let mut cycle = good.clone();
        cycle["sections"]["unused"] = serde_json::json!({"from":"unused"});
        let mut conflict = good.clone();
        conflict["sections"]["phrase"]["events"] = serde_json::json!([{"kind":"mod_wheel","instrument":"part","value":0,"at":{"bar":1,"step":1}},{"kind":"mod_wheel","instrument":"part","value":1,"at":{"bar":1,"step":1}}]);
        let mut malformed = good.clone();
        malformed["sections"]["phrase"]["events"] = serde_json::json!([{"kind":"hit","instrument":"part","down":true,"at":{"bar":1,"step":1}}]);
        let cases = [
            (unknown.to_string(), "meta.key"),
            (cycle.to_string(), "sections.unused"),
            (conflict.to_string(), "sections.phrase.events[1]"),
            (malformed.to_string(), "sections.phrase.events[0].down"),
            ("meta: {title: A, title: B}".into(), "meta.title"),
            ("meta: &a {title: A}\nform: *a".into(), "meta"),
        ];
        for (text, path) in cases {
            let error = super::compile(&text).expect_err("invalid score");
            assert_eq!(error.path, path);
            let detail = &error.detail;
            assert!(detail["line"].as_u64().expect("line") > 0);
            assert!(detail["column"].as_u64().expect("column") > 0);
        }
    }

    #[test]
    fn melody_diagnostics_follow_from_replace_and_use_source_tokens() {
        let diagnostic = |doc: &serde_json::Value, melody: &str, source_path: &str| {
            let mut doc = doc.clone();
            doc["meta"]["title"] = serde_json::json!("青い夜");
            doc["sections"]["phrase"]["lines"]["main"] = serde_json::json!({
                "instrument": "part",
                "melody": melody
            });
            let yaml = super::to_yaml(&doc).expect("Score YAML");
            let text = format!("# 日本語のコメント\n{yaml}");
            let expected_token = text.find(". - - -").expect("quoted tie token") + 2;
            let error = super::compile(&text).expect_err("unmatched tie fails");
            assert_eq!(error.path, source_path);
            let detail = &error.detail;
            let before = &text[..expected_token];
            assert_eq!(
                detail["line"],
                before.bytes().filter(|byte| *byte == b'\n').count() + 1
            );
            assert_eq!(
                detail["column"],
                before
                    .rsplit('\n')
                    .next()
                    .expect("line prefix")
                    .chars()
                    .count()
                    + 1
            );
            assert_eq!(detail["actual"], "-");
            assert_eq!(detail["section"], "phrase");
            assert_eq!(detail["occurrence"], 1);
            assert_eq!(detail["bar"], 1);
            assert_eq!(detail["step"], "2");
            assert_eq!(detail["source_bar"], 1);
        };

        let mut inherited = document();
        inherited["melodies"]["source"] =
            serde_json::json!([". - - - | - - - - | - - - - | - - - -"]);
        inherited["melodies"]["from_copy"] = serde_json::json!({"from":"source"});
        diagnostic(&inherited, "from_copy", "melodies.source[0]");

        let mut replaced = document();
        replaced["melodies"]["base"] =
            serde_json::json!(["1 - - - | 3 - - - | 5 - - - | 1' - - -"]);
        replaced["melodies"]["replace_copy"] = serde_json::json!({
            "from":"base",
            "replace":{"1":". - - - | - - - - | - - - - | - - - -"}
        });
        diagnostic(&replaced, "replace_copy", "melodies.replace_copy.replace.1");

        let mut used = document();
        used["melodies"]["source"] = serde_json::json!([". - - - | - - - - | - - - - | - - - -"]);
        used["melodies"]["use_copy"] = serde_json::json!([{"use":"source","items":[1,1]}]);
        diagnostic(&used, "use_copy", "melodies.source[0]");
    }

    #[test]
    fn triggers_require_an_explicit_note_and_rolls_replace_only_pattern_drums() {
        let mut missing_note = document();
        missing_note["sections"]["phrase"] = serde_json::json!({"events":[{"kind":"hit","instrument":"part","at":{"bar":1,"step":1}}]});
        let error = super::compile(&missing_note.to_string()).expect_err("trigger note required");
        let message = &error.message;
        assert!(message.contains("instruments.part.trigger_note_midi"));
        assert!(message.contains("score quarter 0"));

        let mut doc = document();
        doc["instruments"]["part"]["trigger_note_midi"] = serde_json::json!(36);
        doc["drums"] = serde_json::json!({"beat":{"parts":{"part":"x...|....|....|...."}}});
        doc["sections"]["phrase"] = serde_json::json!({
            "drums":"beat",
            "placements":[{"instrument":"part","degree":"1","at":{"bar":1,"step":1},"duration_quarters":"1/4"}],
            "events":[{"kind":"step_roll","instrument":"part","bar":1,"from_step":1,"through_step":1,"velocity_from":120,"velocity_to":120}]
        });
        let plan = super::compile(&doc.to_string()).expect("explicit trigger note");
        let notes = plan.lanes[0]
            .pattern
            .events
            .iter()
            .filter_map(|event| match event {
                super::MusicalEvent::Note { note, velocity, .. } => Some((*note, *velocity)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 2);
        assert!(
            notes.contains(&(36, 120)),
            "roll replaces the ordinary drum hit"
        );
        assert!(
            notes.contains(&(60, 100)),
            "explicit placement remains layered"
        );

        doc["sections"]["phrase"]["events"] = serde_json::json!([
            {"kind":"step_roll","instrument":"part","bar":1,"from_step":1,"through_step":1,"velocity_from":120,"velocity_to":120},
            {"kind":"step_roll","instrument":"part","bar":1,"from_step":1,"through_step":1,"velocity_from":100,"velocity_to":100}
        ]);
        let error = super::compile(&doc.to_string()).expect_err("overlapping rolls rejected");
        assert!(error.message.contains("overlapping drum rolls"));
    }

    #[test]
    fn instrument_pan_is_validated_and_defaults_to_center() {
        let mut doc = document();
        doc["instruments"]["part"]["pan"] = serde_json::json!(2);
        let error = super::compile(&doc.to_string()).expect_err("pan out of range");
        assert_eq!(error.path, "instruments.part.pan");
        doc["instruments"]["part"]["pan"] = serde_json::json!("left");
        assert!(super::compile(&doc.to_string()).is_err());
        doc["instruments"]["part"]["pan"] = serde_json::json!(-1);
        assert!(super::compile(&doc.to_string()).is_ok());
    }

    #[test]
    fn add_placements_append_across_inheritance_levels_and_placements_replaces() {
        let mut doc = document();
        doc["sections"] = serde_json::json!({
            "phrase": {"placements":[{"instrument":"part","degree":"1","at":{"bar":1,"step":1}}]},
            "bridge": {"from":"phrase","add_placements":[{"instrument":"part","degree":"3","at":{"bar":1,"step":2}}]},
            "final": {"from":"bridge","add_placements":[{"instrument":"part","degree":"5","at":{"bar":1,"step":3}}]},
            "solo": {"add_placements":[{"instrument":"part","degree":"3","at":{"bar":1,"step":4}}]},
            "replaced": {"from":"phrase","placements":[{"instrument":"part","degree":"5","at":{"bar":1,"step":4}}]}
        });
        let notes = |name: &str| {
            let mut doc = doc.clone();
            doc["form"][0]["section"] = serde_json::json!(name);
            let plan = super::compile(&doc.to_string()).expect(name);
            plan.lanes[0]
                .sources
                .iter()
                .map(|source| {
                    (
                        source.spelling.clone().expect("degree spelling"),
                        source.path.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            notes("final"),
            [
                ("1".to_owned(), "sections.phrase.placements[0]".to_owned()),
                (
                    "3".to_owned(),
                    "sections.bridge.add_placements[0]".to_owned()
                ),
                (
                    "5".to_owned(),
                    "sections.final.add_placements[0]".to_owned()
                ),
            ]
        );
        assert_eq!(
            notes("solo"),
            [("3".to_owned(), "sections.solo.add_placements[0]".to_owned())]
        );
        assert_eq!(
            notes("replaced"),
            [("5".to_owned(), "sections.replaced.placements[0]".to_owned())]
        );
        let mut invalid = doc.clone();
        invalid["sections"]["solo"]["add_placements"] =
            serde_json::json!([{"instrument":"part","degree":"9","at":{"bar":1,"step":1}}]);
        let error = super::compile(&invalid.to_string()).expect_err("invalid placement");
        assert_eq!(error.path, "sections.solo.add_placements[0]");
        let mut invalid = doc.clone();
        invalid["sections"]["phrase"]["placements"] =
            serde_json::json!([{"instrument":"part","degree":"9","at":{"bar":1,"step":1}}]);
        let error = super::compile(&invalid.to_string()).expect_err("inherited placement");
        assert_eq!(error.path, "sections.phrase.placements[0]");
    }

    #[test]
    fn form_scale_is_optional_and_does_not_change_notes() {
        let mut doc = document();
        doc["form"][0]
            .as_object_mut()
            .expect("form entry")
            .remove("scale");
        let plan = super::compile(&doc.to_string()).expect("scale-less score compiles");
        assert!(plan.sections[0].scale.is_none());
        let occurrence = serde_json::to_value(&plan.sections[0]).expect("occurrence serializes");
        assert!(occurrence.get("scale").is_none());
        let events = serde_json::to_value(&plan.lanes[0].pattern.events).expect("events");
        doc["form"][0]["scale"] = serde_json::json!("aeolian");
        let plan = super::compile(&doc.to_string()).expect("scaled score compiles");
        assert_eq!(plan.sections[0].scale.as_deref(), Some("aeolian"));
        assert_eq!(
            serde_json::to_value(&plan.lanes[0].pattern.events).expect("events"),
            events
        );
        doc["form"][0]["scale"] = serde_json::json!("mystery");
        let error = super::compile(&doc.to_string()).expect_err("unknown scale");
        assert_eq!(error.path, "form[0].scale");
    }

    #[test]
    fn arps_honor_rests_ties_and_repeat_merging() {
        let mut doc = document();
        doc["meta"]["meter"] = serde_json::json!("2/4");
        doc["sections"]["phrase"] = serde_json::json!({
            "chords":["1:m"],
            "arp":{"instrument":"part","pattern":"up"}
        });
        doc["arps"] = serde_json::json!({"up":{"order":"0 . 2 - | - - . 1"}});
        let notes = |text: &str| {
            let plan = super::compile(text).expect("arp score compiles");
            plan.lanes[0]
                .pattern
                .events
                .iter()
                .filter_map(|event| match event {
                    super::MusicalEvent::Note {
                        tick,
                        duration_ticks,
                        ..
                    } => Some((*tick, *duration_ticks)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(notes(&doc.to_string()), [(0, 120), (240, 480), (840, 120)]);
        doc["arps"]["up"]["order"] = serde_json::json!("1 1 - 2 | . . . .");
        assert_eq!(notes(&doc.to_string()), [(0, 120), (120, 240), (360, 120)]);
        doc["arps"]["up"]["retrigger_each_step"] = serde_json::json!(false);
        assert_eq!(notes(&doc.to_string()), [(0, 360), (360, 120)]);
        doc["arps"]["up"]["retrigger_each_step"] = serde_json::json!(true);
        doc["arps"]["up"]["order"] = serde_json::json!("- 1 . . | . . . .");
        let error = super::compile(&doc.to_string()).expect_err("tie without a note");
        assert_eq!(error.path, "sections.phrase.arp");
    }

    fn expression_document() -> Value {
        json!({
            "meta":{"title":"Expression", "bpm":120, "meter":"4/4", "tuning":"12edo", "gate":"1/2"},
            "form":[{"section":"a", "bars":1, "tonic":"C"}],
            "sections":{"a":{"lines":{"main":{"instrument":"lead", "melody":"line"}}}},
            "melodies":{"line":["1@80~ 3@115~ 5 - | 6 - . . | . . . . | . . . ."]},
            "instruments":{"lead":{"external":true, "gate":"3/4"}}
        })
    }

    fn notes(plan: &ScorePlan, id: &str) -> Vec<(Quarter, Quarter, u8, u8)> {
        plan.lanes
            .iter()
            .filter(|lane| lane.id == id)
            .flat_map(|lane| &lane.pattern.events)
            .filter_map(|event| match event {
                MusicalEvent::Note {
                    tick,
                    duration_ticks,
                    note,
                    velocity,
                } => Some((
                    q(
                        i64::try_from(*tick).unwrap(),
                        i64::from(plan.ticks_per_beat),
                    ),
                    q(
                        i64::try_from(*duration_ticks).unwrap(),
                        i64::from(plan.ticks_per_beat),
                    ),
                    *note,
                    *velocity,
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn note_attributes_gate_and_legato_resolve_on_the_exact_timeline() {
        let doc = expression_document();
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(
            notes(&plan, "lead"),
            [
                (q(0, 1), q(17, 64), 60, 80),
                (q(1, 4), q(17, 64), 64, 115),
                (q(1, 2), q(3, 8), 67, 100),
                (q(1, 1), q(3, 8), 69, 100),
            ]
        );
        for line in [
            "1~ . . . | . . . . | . . . . | . . . .",
            ". . . . | . . . . | . . . . | . . . 1~",
        ] {
            let mut invalid = doc.clone();
            invalid["melodies"]["line"] = json!([line]);
            assert!(compile(&invalid.to_string()).is_err());
        }
        let mut short_gate = doc;
        short_gate["instruments"]["lead"]["gate"] = json!("1/100");
        short_gate["melodies"]["line"] = json!(["1~ 3 . . | . . . . | . . . . | . . . ."]);
        let connected = notes(&compile(&short_gate.to_string()).unwrap(), "lead");
        assert!(connected[0].0.clone() + &connected[0].1 > connected[1].0);
        assert!(connected[0].0.clone() + &connected[0].1 < &connected[1].0 + &connected[1].1);
    }

    #[test]
    fn bass_chord_tone_tokens_follow_the_active_chord() {
        let mut doc = expression_document();
        doc["sections"]["a"] = json!({
            "chords": ["1:maj7"],
            "lines": {"main":{"instrument":"lead","melody":"line","register":"bass"}}
        });
        doc["melodies"]["line"] = json!(["c1 c3 c5 c7 | . . . . | . . . . | . . . ."]);
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|note| note.2)
                .collect::<Vec<_>>(),
            [36, 40, 43, 47]
        );

        doc["sections"]["a"]["chords"] = json!(["1:m7"]);
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|note| note.2)
                .collect::<Vec<_>>(),
            [36, 39, 43, 46]
        );

        doc["sections"]["a"]["chords"] = json!(["5:maj7/3"]);
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|note| note.2)
                .collect::<Vec<_>>(),
            [31, 35, 38, 42]
        );

        doc["sections"]["a"]["chords"] = json!(["1:maj"]);
        assert!(compile(&doc.to_string()).is_err());
    }

    #[test]
    fn placements_accept_chord_tone_pitch_tokens() {
        let mut doc = expression_document();
        doc["sections"]["a"] = json!({
            "chords": ["1:maj"],
            "placements": [{
                "instrument":"lead",
                "at":{"bar":1,"step":1},
                "degree":"c3",
                "duration_steps":4
            }]
        });

        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|note| note.2)
                .collect::<Vec<_>>(),
            [64]
        );
    }

    #[test]
    fn automatic_voicing_limits_upper_tones_and_explicit_voicing_keeps_large_chords() {
        let mut doc = expression_document();
        doc["instruments"] = json!({
            "keys": {
                "external": true,
                "voicing_floor_midi": 0,
                "voicing_ceiling_midi": 127
            }
        });
        doc["sections"]["a"] = json!({
            "chords": ["1:(1,2,b3,3,4,b5,5,b6,6,b7,7)"],
            "comp": {"keys": "hold"}
        });

        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(
            error.message,
            "automatic voicing supports at most 6 upper tones; specify an explicit voicing"
        );
        assert_eq!(error.path, "sections.a.comp.keys");

        doc["sections"]["a"]["chords"] = json!(["1:(1,2,3,4,5,6,7,9,11,13)"]);
        doc["instruments"]["keys"]["voicing"] =
            json!(["1", "2", "3", "4", "5", "6", "7", "9", "11", "13"]);
        assert_eq!(notes(&compile(&doc.to_string()).unwrap(), "keys").len(), 10);
    }

    #[test]
    fn bass_chord_tone_root_must_fit_the_instrument_range() {
        let mut doc = expression_document();
        doc["sections"]["a"] = json!({
            "chords": ["1:maj"],
            "lines": {"main":{"instrument":"lead","melody":"line","register":"bass"}}
        });
        doc["melodies"]["line"] = json!(["c1 . . . | . . . . | . . . . | . . . ."]);
        doc["instruments"]["lead"]["bass_floor_midi"] = json!(28);
        doc["instruments"]["lead"]["bass_ceiling_midi"] = json!(34);

        let error = compile(&doc.to_string()).expect_err("C2 exceeds the bass ceiling");

        assert_eq!(error.message, "bass range contains no matching pitch");
        assert_eq!(error.path, "melodies.line[0]");
    }

    #[test]
    fn automatic_accompaniment_preserves_compound_chord_intervals() {
        for (quality, expected) in [
            ("9", vec![48, 52, 55, 58, 62]),
            ("11", vec![48, 52, 55, 58, 62, 65]),
            ("13", vec![48, 52, 55, 58, 62, 65, 69]),
            ("7(b9,#11,omit5)", vec![48, 52, 58, 61, 66]),
            ("(1,b3,5,b7,#11)", vec![48, 51, 55, 58, 66]),
        ] {
            let mut doc = expression_document();
            doc["sections"]["a"] = json!({"chords":[format!("1:{quality}")],
            "comp":{"lead":"pulse"},"pad":{"instrument":"pad","pattern":"pulse"},
            "arp":{"instrument":"arp","pattern":"up"}});
            for id in ["pad", "arp"] {
                doc["instruments"][id] = json!({"external":true});
            }
            doc["comp"] = json!({"pulse":"x - - - | . . . . | . . . . | . . . ."});
            let cells: Vec<_> = (0..16)
                .map(|index| {
                    if index < expected.len() {
                        index.to_string()
                    } else {
                        ".".into()
                    }
                })
                .collect();
            let order = cells
                .chunks(4)
                .map(|beat| beat.join(" "))
                .collect::<Vec<_>>()
                .join(" | ");
            doc["arps"] = json!({"up":{"order":order}});

            let plan = compile(&doc.to_string()).unwrap();

            for id in ["lead", "pad", "arp"] {
                assert_eq!(
                    notes(&plan, id)
                        .iter()
                        .map(|note| note.2)
                        .collect::<Vec<_>>(),
                    expected,
                    "{quality}: {id}"
                );
            }
        }
    }

    #[test]
    fn voicing_precedence_is_shared_by_comp_pad_arp_and_placements() {
        let mut doc = expression_document();
        doc["instruments"]["lead"]["voicing"] = json!(["3", "7"]);
        doc["sections"]["a"] = json!({"chords":["1:m9"],
        "comp":{"lead":{"pattern":"pulse", "voicing":["9'"]}},
        "pad":{"instrument":"pad", "pattern":"pulse"},
        "arp":{"instrument":"arp", "pattern":"up", "voicing":["3", "9"]},
        "placements":[{"instrument":"lead", "at":{"bar":1,"step":9}, "chord":"1:m9", "voicing":["5,"], "duration_quarters":1}]});
        doc["instruments"]["pad"] = json!({"external":true, "voicing":["3", "7"]});
        doc["instruments"]["arp"] = json!({"external":true});
        doc["comp"] = json!({"pulse":"x - - - | . . . . | . . . . | . . . ."});
        doc["arps"] = json!({"up":{"order":"0 1 . . | . . . . | . . . . | . . . ."}});
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(
            notes(&plan, "lead").iter().map(|n| n.2).collect::<Vec<_>>(),
            [74, 43]
        );
        assert_eq!(
            notes(&plan, "pad").iter().map(|n| n.2).collect::<Vec<_>>(),
            [51, 58]
        );
        assert_eq!(
            notes(&plan, "arp").iter().map(|n| n.2).collect::<Vec<_>>(),
            [51, 62]
        );
        doc["sections"]["a"]["comp"]["lead"] = json!("pulse");
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|n| n.2)
                .collect::<Vec<_>>(),
            [51, 58, 43]
        );
        let placement = &mut doc["sections"]["a"]["placements"][0];
        placement.as_object_mut().unwrap().remove("chord");
        placement["degree"] = json!("1");
        assert!(
            compile(&doc.to_string())
                .unwrap_err()
                .message
                .contains("voicing fields require chord")
        );
        doc["sections"]["a"] = json!({"chords":["1:maj/3"],
        "comp":{"lead":{"pattern":"pulse","voicing":["1,"]}}});
        let invalid = compile(&doc.to_string()).unwrap_err();
        assert!(invalid.message.contains("below the slash bass"));
        assert_eq!(invalid.path, "sections.a.comp.lead");
        doc["sections"]["a"]["comp"]["lead"]["voicing"] = json!(["1"]);
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|note| note.2)
                .collect::<Vec<_>>(),
            [52, 60]
        );
    }

    #[test]
    fn counter_rests_and_ties_respect_lead_silence_and_instrument_gate() {
        let mut doc = expression_document();
        doc["melodies"]["line"] = json!(["1 - - - | . . . . | 1 - - - | . . . ."]);
        doc["sections"]["a"]["counter"] =
            json!({"instrument":"counter", "pattern":"answer", "follows":["main"]});
        doc["instruments"]["counter"] = json!({"external":true});
        doc["counters"] = json!({"answer":{"when":"silent_full_beat", "cell":". 3 - 5"}});
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(
            notes(&plan, "counter"),
            [
                (q(5, 4), q(1, 4), 64, 100),
                (q(7, 4), q(1, 8), 67, 100),
                (q(13, 4), q(1, 4), 64, 100),
                (q(15, 4), q(1, 8), 67, 100)
            ]
        );
        doc["counters"]["answer"]["cell"] = json!("- 3");
        assert!(compile(&doc.to_string()).is_err());

        doc["counters"]["answer"]["cell"] = json!("3");
        doc["melodies"]["line"] = json!(["1 - - - | - - - - | . . . . | . . . ."]);
        doc["instruments"]["lead"]["gate"] = json!("1/4");
        doc["instruments"]["layer"] = json!({"external":true,"gate":1});
        doc["sections"]["a"]["lines"]["double"] = json!({"instrument":"layer","melody":"line"});
        doc["sections"]["a"]["counter"]["follows"] = json!(["main", "double"]);
        let layered = compile(&doc.to_string()).unwrap();
        assert_eq!(notes(&layered, "lead")[0].1, q(1, 2));
        assert_eq!(notes(&layered, "layer")[0].1, q(2, 1));
        assert_eq!(
            notes(&layered, "counter")
                .iter()
                .map(|n| n.0.clone())
                .collect::<Vec<_>>(),
            [q(2, 1), q(3, 1)]
        );
    }

    #[test]
    fn swing_warps_all_grid_generators_but_keeps_explicit_events_and_tuplets() {
        let mut doc = expression_document();
        doc["meta"]["swing"] = json!({"subdivision":8,"ratio":"2/3"});
        doc["meta"]["gate"] = json!(1);
        doc["instruments"]["lead"]["gate"] = json!(1);
        doc["instruments"]["drum"] = json!({"external":true,"trigger_note_midi":36,"gate":"1/2"});
        doc["sections"]["a"]["drums"] = json!("beat");
        doc["drums"] = json!({"beat":{"parts":{"drum":"xxxx | .... | .... | ...."}}});
        doc["melodies"]["line"] = json!(["1 - 3 - | [1 2 3] | . . . . | . . . ."]);
        doc["sections"]["a"]["placements"] = json!([{"instrument":"lead", "degree":"5", "at":{"bar":1,"offset_quarters":"1/2"},"duration_quarters":"1/2"}]);
        let plan = compile(&doc.to_string()).unwrap();
        let lead = notes(&plan, "lead");
        assert!(lead.contains(&(q(0, 1), q(2, 3), 60, 100)));
        assert!(lead.contains(&(q(2, 3), q(1, 3), 64, 100)));
        assert!(lead.contains(&(q(1, 2), q(1, 2), 67, 100)));
        let tuplets: Vec<_> = lead
            .iter()
            .filter(|n| n.0 >= q(1, 1) && n.0 < q(2, 1))
            .map(|n| (n.0.clone(), n.1.clone()))
            .collect();
        assert_eq!(
            tuplets,
            [(q(1, 1), q(1, 3)), (q(4, 3), q(1, 3)), (q(5, 3), q(1, 3))]
        );
        assert_eq!(
            notes(&plan, "drum")
                .iter()
                .map(|n| (n.0.clone(), n.1.clone()))
                .collect::<Vec<_>>(),
            [
                (q(0, 1), q(1, 6)),
                (q(1, 3), q(1, 6)),
                (q(2, 3), q(1, 12)),
                (q(5, 6), q(1, 12))
            ]
        );
        assert_eq!(plan.length_ticks, 4 * u64::from(plan.ticks_per_beat));
    }
    #[test]
    fn swing_preserves_meters_and_form_overrides_without_affecting_equal_timing() {
        for (meter, beats, slots) in [("3/4", 3, 4), ("6/8", 6, 2), ("5/8", 5, 2), ("7/16", 7, 1)] {
            let mut doc = expression_document();
            doc["meta"]["meter"] = json!(meter);
            let pattern = (0..beats)
                .map(|_| vec!["1"; slots].join(" "))
                .collect::<Vec<_>>()
                .join(" | ");
            doc["melodies"]["line"] = json!([pattern]);
            let straight = compile(&doc.to_string()).unwrap();
            for subdivision in [8, 16] {
                doc["meta"]["swing"] = json!({"subdivision":subdivision,"ratio":"1/2"});
                assert_eq!(
                    notes(&compile(&doc.to_string()).unwrap(), "lead"),
                    notes(&straight, "lead")
                );
                doc["meta"]["swing"]["ratio"] = json!("2/3");
                let swung = compile(&doc.to_string()).unwrap();
                assert_eq!(
                    q(
                        i64::try_from(swung.length_ticks).unwrap(),
                        i64::from(swung.ticks_per_beat)
                    ),
                    q(
                        i64::try_from(straight.length_ticks).unwrap(),
                        i64::from(straight.ticks_per_beat)
                    )
                );
                let end = q(
                    i64::try_from(swung.length_ticks).unwrap(),
                    i64::from(swung.ticks_per_beat),
                );
                assert!(
                    notes(&swung, "lead")
                        .iter()
                        .all(|(at, duration, ..)| at + duration <= end)
                );
            }
        }
        let mut doc = expression_document();
        doc["melodies"]["line"] = json!(["1 - 3 - | . . . . | . . . . | . . . ."]);
        doc["meta"]["swing"] = json!({"subdivision":8,"ratio":"2/3"});
        doc["form"] = json!([
            {"section":"a","bars":1,"tonic":"C"},
            {"section":"a","bars":1,"tonic":"C","swing":{"subdivision":8,"ratio":"1/2"}},
            {"section":"a","bars":1,"tonic":"C"}
        ]);
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|n| n.0.clone())
                .collect::<Vec<_>>(),
            [q(0, 1), q(2, 3), q(4, 1), q(9, 2), q(8, 1), q(26, 3)]
        );
        for invalid in [
            json!({"subdivision":4,"ratio":"2/3"}),
            json!({"subdivision":8,"ratio":0}),
            json!({"subdivision":8,"ratio":1}),
        ] {
            doc["meta"]["swing"] = invalid;
            assert!(compile(&doc.to_string()).is_err());
        }
    }

    #[test]
    fn swing_gate_and_legato_are_shared_by_the_accompaniment_generators() {
        let mut doc = expression_document();
        doc["meta"]["swing"] = json!({"subdivision":16,"ratio":"2/3"});
        doc["sections"]["a"] = json!({"chords":["1:7"],
        "comp":{"comp":"pulse"},"pad":{"instrument":"pad","pattern":"pulse"},
        "arp":{"instrument":"arp","pattern":"pulse"},"counter":{"instrument":"counter","pattern":"pulse","follows":["silent"]},"lines":{"main":{"instrument":"lead","melody":"pulse","register":"bass"},"silent":{"instrument":"lead","melody":"silent"}}});
        doc["melodies"]["silent"] = json!([".:16"]);
        for id in ["comp", "pad", "arp", "counter"] {
            doc["instruments"][id] = json!({"external":true});
        }
        doc["melodies"]["pulse"] = json!(["1~ 3 - . | . . . . | . . . . | . . . ."]);
        doc["comp"] = json!({"pulse":"x x . . | . . . . | . . . . | . . . ."});
        doc["arps"] = json!({"pulse":{"order":"0 1 . . | . . . . | . . . . | . . . ."}});
        doc["counters"] = json!({"pulse":{"when":"silent_full_beat","cell":"1 3 - ."}});
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(notes(&plan, "lead")[0].1, q(1, 3) + q(1, 64));
        assert_eq!(notes(&plan, "lead")[1].0, q(1, 3));
        for id in ["comp", "pad", "arp"] {
            let events = notes(&plan, id);
            assert!(
                events.iter().any(|n| n.0 == q(0, 1) && n.1 == q(1, 6)),
                "{id}"
            );
            assert!(
                events.iter().any(|n| n.0 == q(1, 3) && n.1 == q(1, 12)),
                "{id}"
            );
        }
        assert_eq!(notes(&plan, "counter")[1].0, q(1, 3));
        assert_eq!(notes(&plan, "counter")[1].1, q(1, 4));
    }

    #[test]
    fn bar_conditions_select_drum_overrides() {
        let mut doc = expression_document();
        doc["form"] = json!([{"section":"a","bars":4,"tonic":"C"}]);
        doc["sections"]["a"] = json!({
        "chords":["1:maj","1:maj","1:maj","1:maj"],
        "drums":"beat"});
        doc["instruments"]["drum"] = json!({"external":true,"trigger_note_midi":36});
        doc["drums"] = json!({"beat":{"parts":{"drum":"x...|....|....|...."},
        "overrides":[{"when":{"even":true},"parts":{"drum":"X...|....|....|...."}}]}});
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(
            notes(&plan, "drum")
                .iter()
                .map(|n| (n.0.clone(), n.2, n.3))
                .collect::<Vec<_>>(),
            [
                (q(0, 1), 36, 96),
                (q(4, 1), 36, 120),
                (q(8, 1), 36, 96),
                (q(12, 1), 36, 120)
            ]
        );
    }

    #[test]
    fn pad_hold_is_builtin_and_refollows_chord_changes() {
        let mut doc = expression_document();
        doc["sections"]["a"] =
            json!({"chords":["1:maj 4:maj"], "pad":{"instrument":"pad","pattern":"hold"}});
        doc["instruments"]["pad"] = json!({"external":true});
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(
            notes(&plan, "pad"),
            [
                (q(0, 1), q(1, 1), 48, 100),
                (q(0, 1), q(1, 1), 52, 100),
                (q(0, 1), q(1, 1), 55, 100),
                (q(2, 1), q(1, 1), 53, 100),
                (q(2, 1), q(1, 1), 57, 100),
                (q(2, 1), q(1, 1), 60, 100)
            ]
        );
        doc["form"] =
            json!([{"section":"a","bars":4,"tonic":"C"},{"section":"a","bars":4,"tonic":"C"}]);
        doc["sections"]["a"]["chords"] = json!(["1:maj", "5:maj", "NC", "1:maj"]);
        let reset = compile(&doc.to_string()).unwrap();
        for at in [q(0, 1), q(12, 1), q(16, 1), q(28, 1)] {
            assert_eq!(
                notes(&reset, "pad")
                    .into_iter()
                    .filter(|n| n.0 == at)
                    .map(|n| n.2)
                    .collect::<Vec<_>>(),
                [48, 52, 55]
            );
        }
        doc["comp"] = json!({"hold":"x - - - | . . . . | . . . . | . . . ."});
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "comp.hold");
    }

    #[test]
    fn pattern_lengths_assertions_and_swing_share_the_section_timeline() {
        let mut doc = expression_document();
        let expected = notes(&compile(&doc.to_string()).unwrap(), "lead");
        doc["melodies"]["line"] = json!(["1@80~ 3@115~ 5:2 | 6:2 .:10"]);
        assert_eq!(notes(&compile(&doc.to_string()).unwrap(), "lead"), expected);

        doc["melodies"]["line"] = json!(["1 | .:15"]);
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "melodies.line[0]");
        assert_eq!(
            error.message,
            "boundary assertion is not at the requested boundary"
        );
        let detail = &error.detail;
        assert_eq!(detail["actual"], "|");
        assert!(detail["column"].as_u64().unwrap() > 0);

        doc["form"][0]["bars"] = json!(2);
        doc["instruments"]["lead"]["gate"] = json!(1);
        doc["meta"]["swing"] = json!({"subdivision":8,"ratio":"2/3"});
        doc["melodies"]["line"] = json!(["1:18 3:14"]);
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(
            notes(&plan, "lead"),
            [(q(0, 1), q(14, 3), 60, 100), (q(14, 3), q(10, 3), 64, 100)]
        );

        doc["meta"]["meter"] = json!("3/4");
        doc["form"][0]["bars"] = json!(1);
        doc["sections"]["a"]["lines"]["main"]["loop"] = json!(true);
        doc["melodies"]["line"] = json!(["[1 3]:6"]);
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(
            notes(&plan, "lead")
                .iter()
                .map(|n| n.0.clone())
                .collect::<Vec<_>>(),
            [q(0, 1), q(5, 6), q(5, 3), q(7, 3)]
        );
        doc["melodies"]["line"] = json!(["[1 3] .:2"]);
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "melodies.line[0]");
        assert_eq!(error.message, "tuplet must start at an interval boundary");
        doc["melodies"]["line"] = json!(["1:5"]);
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "sections.a.lines.main");
        assert_eq!(
            error.message,
            "melody length must match section length or divide it with loop"
        );
    }

    #[test]
    fn melody_items_preserve_references_replacement_lengths_and_pickups() {
        let mut doc = expression_document();
        doc["instruments"]["lead"]["gate"] = json!(1);
        doc["melodies"] = json!({"base":["1:8","3:8"],"changed":{"from":"base","replace":{"2":"5:8"}},"line":[{"use":"changed","items":[2,2]},{"use":"changed","items":[2,2]}]});
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead"),
            [(q(0, 1), q(2, 1), 67, 100), (q(2, 1), q(2, 1), 67, 100)]
        );
        doc["melodies"]["changed"]["replace"]["2"] = json!("5:7");
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "melodies.changed.replace.2");
        assert_eq!(
            error.message,
            "replacement item length must equal source length"
        );

        doc["melodies"]["changed"]["replace"]["2"] = json!("5:8");
        doc["melodies"]["line"] = json!([{"use":"changed","items":[2,2],"shift_steps":-2},".:8"]);
        doc["sections"]["a"]["placements"] =
            json!([{ "instrument":"lead","melody":"line","at":{"bar":1,"step":5},"gain_db":-6 }]);
        doc["sections"]["a"]
            .as_object_mut()
            .unwrap()
            .remove("lines");
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead"),
            [(q(1, 2), q(2, 1), 67, 100)]
        );
        doc["sections"]["a"]
            .as_object_mut()
            .unwrap()
            .remove("placements");
        doc["sections"]["a"]["lines"] = json!({"main":{"instrument":"lead","melody":"line"}});

        doc["form"] =
            json!([{"section":"intro","bars":1,"tonic":"C"},{"section":"a","bars":2,"tonic":"C"}]);
        doc["sections"]["intro"] = json!({});
        doc["sections"]["a"]["lines"]["main"]["loop"] = json!(true);
        doc["melodies"] = json!({"line":{"pickup_steps":3,"items":["5 6 7 || 1:16"]}});
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead"),
            [
                (q(13, 4), q(1, 4), 67, 100),
                (q(7, 2), q(1, 4), 69, 100),
                (q(15, 4), q(1, 4), 71, 100),
                (q(4, 1), q(4, 1), 60, 100),
                (q(8, 1), q(4, 1), 60, 100)
            ]
        );
        doc["form"] = json!([{"section":"a","bars":1,"tonic":"C"}]);
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "melodies.line.items[0]");
        assert_eq!(error.message, "melody note begins before score");

        doc["melodies"]["line"]["items"] = json!(["5 6 7 || 1:16", "3:16"]);
        doc["sections"]["a"]["lines"]["main"]["slice_items"] = json!([2, 2]);
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead"),
            [(q(0, 1), q(4, 1), 64, 100)]
        );
    }

    #[test]
    fn melody_registers_tonic_overrides_and_chord_tones_resolve_pitch() {
        let mut doc = expression_document();
        doc["sections"]["a"]["chords"] = json!(["1:m7/5"]);
        doc["sections"]["a"]["lines"]["main"]["register"] = json!("bass");
        doc["melodies"]["line"] =
            json!({"relative_to":"chord_bass","items":["1:4 b3:4 c3:4 c7:4"]});
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|n| n.2)
                .collect::<Vec<_>>(),
            [31, 34, 39, 46]
        );
        doc["sections"]["a"]["lines"]["main"]["tonic"] = json!("D");
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "sections.a.lines.main");
        assert_eq!(error.message, "tonic cannot override chord_bass material");
        doc["melodies"]["line"]["relative_to"] = json!("tonic");
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead")
                .iter()
                .map(|n| n.2)
                .collect::<Vec<_>>(),
            [38, 41, 39, 46]
        );
    }

    #[test]
    fn articulation_and_dynamics_combine_before_velocity_validation() {
        let mut doc = expression_document();
        doc["melodies"]["line"] = json!(["1^!@110:4 3_:4 5:8"]);
        doc["instruments"]["lead"]["articulations"] =
            json!({"staccato":{"gate":"1/3"},"tenuto":{"velocity":-4}});
        doc["sections"]["a"]["events"] = json!([{"kind":"dynamics","instrument":"lead","at":{"bar":1,"step":1},"duration_quarters":4,"from":-20,"to":-10}]);
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead"),
            [
                (q(0, 1), q(1, 3), 60, 106),
                (q(1, 1), q(1, 1), 64, 78),
                (q(2, 1), q(3, 2), 67, 85)
            ]
        );
        doc["sections"]["a"]["events"][0]["from"] = json!(10);
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "melodies.line[0]");
        assert_eq!(error.message, "final velocity must be 1..127");
        doc["sections"]["a"]["events"][0]["from"] = json!(-20);
        let duplicate = doc["sections"]["a"]["events"][0].clone();
        doc["sections"]["a"]["events"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "sections.a.events[0]");
        assert_eq!(error.message, "overlapping dynamics for instrument");
    }

    #[test]
    fn control_automation_samples_exact_positions_and_detects_conflicts() {
        let mut doc = expression_document();
        doc["sections"]["a"]["events"] = json!([{"kind":"automation","instrument":"lead","at":{"bar":1,"step":1},"duration_quarters":"1/32","control":"pitch_bend","from_value":-1,"to_value":1}]);
        let plan = compile(&doc.to_string()).unwrap();
        let controls = plan.lanes[0]
            .pattern
            .events
            .iter()
            .filter_map(|e| {
                if let MusicalEvent::PitchBend { tick, value } = e {
                    Some((
                        q(
                            i64::try_from(*tick).unwrap(),
                            i64::from(plan.ticks_per_beat),
                        ),
                        *value,
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            controls,
            [(q(0, 1), -1.0), (q(1, 64), 0.0), (q(1, 32), 1.0)]
        );
        doc["sections"]["a"]["events"].as_array_mut().unwrap().push(json!({"kind":"pitch_bend","instrument":"lead","at":{"bar":1,"offset_quarters":"1/128"},"value":0}));
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "sections.a.events[1]");
        assert_eq!(error.message, "control conflicts with automation interval");
    }

    #[test]
    fn instrument_swing_and_offsets_apply_after_notated_expressions() {
        let mut doc = expression_document();
        doc["meta"]["swing"] = json!({"subdivision":8,"ratio":"2/3"});
        doc["form"][0]["swing"] = json!({"subdivision":8,"ratio":"3/4"});
        doc["instruments"]["lead"]["swing"] = json!({"subdivision":8,"ratio":"1/2"});
        doc["instruments"]["lead"]["gate"] = json!(1);
        doc["instruments"]["lead"]["timing_offset_steps"] = json!("1/2");
        doc["melodies"]["line"] = json!(["1:2 3:14"]);
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "lead"),
            [(q(1, 8), q(1, 2), 60, 100), (q(5, 8), q(27, 8), 64, 100)]
        );
        doc["instruments"]["lead"]["timing_offset_steps"] = json!("-1/2");
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "melodies.line[0]");
        assert_eq!(error.message, "timing offset places event outside score");
    }

    #[test]
    fn drum_phrases_repeat_and_bar_lists_select_overrides() {
        let mut doc = expression_document();
        doc["form"][0]["bars"] = json!(4);
        doc["sections"]["a"] = json!({"drums":"beat"});
        doc["instruments"]["drum"] = json!({"external":true,"trigger_note_midi":36});
        doc["drums"] = json!({"beat":{"parts":{"drum":["x... .... .... ....",".... x... .... ...."]},"overrides":[{"when":{"bars":[3,"last"]},"parts":{"drum":".... .... X... ...."}}]}});
        assert_eq!(
            notes(&compile(&doc.to_string()).unwrap(), "drum")
                .iter()
                .map(|n| (n.0.clone(), n.3))
                .collect::<Vec<_>>(),
            [
                (q(0, 1), 96),
                (q(5, 1), 96),
                (q(10, 1), 120),
                (q(14, 1), 120)
            ]
        );
        doc["form"][0]["bars"] = json!(3);
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "drums.beat.parts.drum");
        assert_eq!(
            error.message,
            "section bars must be a multiple of drum phrase length"
        );
    }

    #[test]
    fn anticipated_harmony_holds_across_bar_heads_and_guides_bass() {
        let mut doc = expression_document();
        doc["form"][0]["bars"] = json!(2);
        doc["sections"]["a"] = json!({"chords":["1:maj","<4:maj"],"pad":{"instrument":"pad","pattern":"hold"},"lines":{"main":{"instrument":"lead","melody":"line","register":"bass"}}});
        doc["instruments"]["pad"] = json!({"external":true,"gate":1});
        doc["instruments"]["lead"]["gate"] = json!(1);
        doc["melodies"]["line"] = json!({"relative_to":"chord_bass","items":[".:15 1:17"]});
        let plan = compile(&doc.to_string()).unwrap();
        assert_eq!(notes(&plan, "lead"), [(q(15, 4), q(17, 4), 29, 100)]);
        assert_eq!(
            notes(&plan, "pad"),
            [
                (q(0, 1), q(15, 4), 48, 100),
                (q(0, 1), q(15, 4), 52, 100),
                (q(0, 1), q(15, 4), 55, 100),
                (q(15, 4), q(17, 4), 53, 100),
                (q(15, 4), q(17, 4), 57, 100),
                (q(15, 4), q(17, 4), 60, 100)
            ]
        );
        assert_eq!(
            plan.sections[0].chords[1].tick,
            15 * u64::from(plan.ticks_per_beat) / 4
        );
        assert_eq!(
            plan.lanes.iter().find(|l| l.id == "pad").unwrap().sources[3]
                .spelling
                .as_deref(),
            Some("4:maj")
        );
        let mut crossing = doc.clone();
        crossing["form"] = json!([{"section":"before","bars":1,"tonic":"C"},{"section":"after","bars":1,"tonic":"C"}]);
        crossing["sections"] = json!({"before":{"chords":["1:maj"],"pad":{"instrument":"pad","pattern":"hold"}},"after":{"chords":["<4:maj"],"pad":{"instrument":"pad","pattern":"hold"}}});
        let crossing = compile(&crossing.to_string()).unwrap();
        assert_eq!(
            crossing.sections[1].chords[0].tick,
            15 * u64::from(crossing.ticks_per_beat) / 4
        );
        assert_eq!(
            notes(&crossing, "pad")
                .iter()
                .filter(|n| n.0 == q(4, 1))
                .map(|n| n.2)
                .collect::<Vec<_>>(),
            [53, 57, 60]
        );
        doc["sections"]["a"]["chords"] = json!(["1:maj", "<<<<<<<<<<<<<<<<4:maj"]);
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "sections");
        assert_eq!(
            error.message,
            "chord anticipation must follow previous chord change and remain within score"
        );
    }

    #[test]
    fn tempo_ramps_expand_to_steps_and_reject_intervening_changes() {
        let mut doc = expression_document();
        doc["time_changes"] = json!([{"at_quarters":1,"bpm":60,"over_quarters":1},{"at_quarters":2,"bpm":120,"over_quarters":"1/2"}]);
        let plan = compile(&doc.to_string()).unwrap();
        let tempos = plan
            .timeline
            .tempo_changes
            .iter()
            .map(|t| {
                (
                    q(
                        i64::try_from(t.tick).unwrap(),
                        i64::from(plan.ticks_per_beat),
                    ),
                    t.bpm,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            tempos,
            [
                (q(0, 1), 120.0),
                (q(1, 1), 105.0),
                (q(5, 4), 90.0),
                (q(3, 2), 75.0),
                (q(7, 4), 60.0),
                (q(2, 1), 90.0),
                (q(9, 4), 120.0)
            ]
        );
        doc["time_changes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"at_quarters":"3/2","bpm":100}));
        let error = compile(&doc.to_string()).unwrap_err();
        assert_eq!(error.path, "time_changes[0]");
        assert_eq!(error.message, "tempo change overlaps tempo ramp");
    }
}
