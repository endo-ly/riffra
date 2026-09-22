use serde::Deserialize;
use std::collections::BTreeSet;

const MIN_PREVIEW_TEMPO_BPM: f64 = 30.0;
const MAX_PREVIEW_TEMPO_BPM: f64 = 300.0;
const MIN_PREVIEW_TICKS_PER_BEAT: u16 = 1;
const MAX_PREVIEW_TICKS_PER_BEAT: u16 = 32_767;
const MAX_PREVIEW_NUMERATOR: u8 = 32;
const MAX_PREVIEW_DENOMINATOR: u8 = 128;
const MIN_PREVIEW_NOTES: usize = 1;
const MAX_PREVIEW_NOTES: usize = 32;
const MAX_PREVIEW_DURATION_SECONDS: f64 = 10.0;
const MAX_MIDI_NOTE: u8 = 127;
const MAX_CATEGORY_CHARS: usize = 64;
const MAX_TAG_COUNT: usize = 12;
const MAX_TAG_CHARS: usize = 32;

/// The practical MIDI range recommended for one instrument.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentRecommendedRange {
    pub min_midi: u8,
    pub max_midi: u8,
}

/// The deterministic MIDI pattern used for an instrument preview.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentPreviewDefinition {
    pub tempo_bpm: f64,
    pub ticks_per_beat: u16,
    pub time_signature: InstrumentPreviewTimeSignature,
    pub length_ticks: u64,
    pub notes: Vec<InstrumentPreviewNote>,
}

/// The meter used by an instrument preview.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentPreviewTimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

/// One MIDI note in an instrument preview.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentPreviewNote {
    pub tick: u64,
    pub duration_ticks: u64,
    pub note: u8,
    pub velocity: u8,
}

/// Display metadata projected from a Sonalloy instrument definition.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InstrumentMetadata {
    pub(crate) name: String,
    pub(crate) author: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) recommended_range: Option<InstrumentRecommendedRange>,
    pub(crate) preview: Option<InstrumentPreviewDefinition>,
}

#[derive(Debug, Deserialize)]
struct InstrumentDefinitionProjection {
    metadata: SourceInstrumentMetadata,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct SourceInstrumentMetadata {
    name: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    recommended_range: Option<InstrumentRecommendedRangeSource>,
    #[serde(default)]
    preview: Option<InstrumentPreviewDefinitionSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct InstrumentRecommendedRangeSource {
    min_midi: u8,
    max_midi: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct InstrumentPreviewDefinitionSource {
    tempo_bpm: f64,
    ticks_per_beat: u16,
    time_signature: InstrumentPreviewTimeSignatureSource,
    length_ticks: u64,
    notes: Vec<InstrumentPreviewNoteSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct InstrumentPreviewTimeSignatureSource {
    numerator: u8,
    denominator: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct InstrumentPreviewNoteSource {
    tick: u64,
    duration_ticks: u64,
    note: u8,
    velocity: u8,
}

/// Reads and validates the display metadata from one Sonalloy definition.
pub(crate) fn read_definition_metadata(
    definition_json: &str,
    instrument_id: &str,
) -> Result<InstrumentMetadata, String> {
    let projection: InstrumentDefinitionProjection = serde_json::from_str(definition_json)
        .map_err(|error| {
            format!(
                "instrument '{instrument_id}' metadata could not be read from definition.json: {error}"
            )
        })?;
    let source = projection.metadata;
    let metadata = InstrumentMetadata {
        name: source.name.trim().to_owned(),
        author: normalize_optional_text(source.author),
        description: normalize_optional_text(source.description),
        category: source.category,
        tags: source.tags,
        recommended_range: source
            .recommended_range
            .map(|range| InstrumentRecommendedRange {
                min_midi: range.min_midi,
                max_midi: range.max_midi,
            }),
        preview: source.preview.map(InstrumentPreviewDefinition::from),
    };
    validate_instrument_metadata(instrument_id, &metadata)?;
    Ok(metadata)
}

impl From<InstrumentPreviewDefinitionSource> for InstrumentPreviewDefinition {
    fn from(source: InstrumentPreviewDefinitionSource) -> Self {
        Self {
            tempo_bpm: source.tempo_bpm,
            ticks_per_beat: source.ticks_per_beat,
            time_signature: InstrumentPreviewTimeSignature {
                numerator: source.time_signature.numerator,
                denominator: source.time_signature.denominator,
            },
            length_ticks: source.length_ticks,
            notes: source
                .notes
                .into_iter()
                .map(|note| InstrumentPreviewNote {
                    tick: note.tick,
                    duration_ticks: note.duration_ticks,
                    note: note.note,
                    velocity: note.velocity,
                })
                .collect(),
        }
    }
}

pub(crate) fn validate_instrument_metadata(
    instrument_id: &str,
    metadata: &InstrumentMetadata,
) -> Result<(), String> {
    if metadata.name.trim().is_empty() {
        return Err(metadata_error(
            instrument_id,
            "metadata.name",
            "must not be empty",
        ));
    }
    if metadata.name.chars().any(char::is_control) {
        return Err(metadata_error(
            instrument_id,
            "metadata.name",
            "must not contain control characters",
        ));
    }
    if let Some(category) = &metadata.category {
        validate_text(
            instrument_id,
            "metadata.category",
            category,
            MAX_CATEGORY_CHARS,
            "category",
        )?;
    }
    if metadata.tags.len() > MAX_TAG_COUNT {
        return Err(metadata_error(
            instrument_id,
            "metadata.tags",
            "must contain at most 12 items",
        ));
    }
    let mut seen = BTreeSet::new();
    for (index, tag) in metadata.tags.iter().enumerate() {
        let field = format!("metadata.tags[{index}]");
        validate_text(instrument_id, &field, tag, MAX_TAG_CHARS, "tag")?;
        if !seen.insert(tag.to_ascii_lowercase()) {
            return Err(metadata_error(
                instrument_id,
                &field,
                "must not duplicate another tag",
            ));
        }
    }
    if let Some(range) = &metadata.recommended_range
        && (range.min_midi > MAX_MIDI_NOTE
            || range.max_midi > MAX_MIDI_NOTE
            || range.min_midi > range.max_midi)
    {
        return Err(metadata_error(
            instrument_id,
            "metadata.recommended_range",
            "must contain MIDI notes from 0 to 127 with min_midi at most max_midi",
        ));
    }
    if let Some(preview) = &metadata.preview {
        validate_preview(instrument_id, preview, metadata.recommended_range.as_ref())?;
    }
    Ok(())
}

fn validate_text(
    instrument_id: &str,
    field: &str,
    value: &str,
    maximum_chars: usize,
    label: &str,
) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > maximum_chars
        || value.chars().any(char::is_control)
    {
        return Err(metadata_error(
            instrument_id,
            field,
            &format!("{label} has invalid text"),
        ));
    }
    Ok(())
}

fn validate_preview(
    instrument_id: &str,
    preview: &InstrumentPreviewDefinition,
    recommended_range: Option<&InstrumentRecommendedRange>,
) -> Result<(), String> {
    if !preview.tempo_bpm.is_finite()
        || preview.tempo_bpm < MIN_PREVIEW_TEMPO_BPM
        || preview.tempo_bpm > MAX_PREVIEW_TEMPO_BPM
    {
        return Err(metadata_error(
            instrument_id,
            "metadata.preview.tempo_bpm",
            "must be finite and between 30 and 300 BPM",
        ));
    }
    if preview.ticks_per_beat < MIN_PREVIEW_TICKS_PER_BEAT
        || preview.ticks_per_beat > MAX_PREVIEW_TICKS_PER_BEAT
    {
        return Err(metadata_error(
            instrument_id,
            "metadata.preview.ticks_per_beat",
            "must be between 1 and 32767",
        ));
    }
    if preview.time_signature.numerator == 0
        || preview.time_signature.numerator > MAX_PREVIEW_NUMERATOR
        || !is_valid_preview_denominator(preview.time_signature.denominator)
    {
        return Err(metadata_error(
            instrument_id,
            "metadata.preview.time_signature",
            "has invalid numerator or denominator",
        ));
    }
    if preview.length_ticks == 0 || preview_duration_seconds(preview) > MAX_PREVIEW_DURATION_SECONDS
    {
        return Err(metadata_error(
            instrument_id,
            "metadata.preview.length_ticks",
            "must describe at most 10 seconds of music",
        ));
    }
    if !(MIN_PREVIEW_NOTES..=MAX_PREVIEW_NOTES).contains(&preview.notes.len()) {
        return Err(metadata_error(
            instrument_id,
            "metadata.preview.notes",
            "must contain between 1 and 32 notes",
        ));
    }
    let mut previous_tick = None;
    for (index, note) in preview.notes.iter().enumerate() {
        let field = format!("metadata.preview.notes[{index}]");
        if previous_tick.is_some_and(|previous| note.tick < previous) {
            return Err(metadata_error(
                instrument_id,
                &format!("{field}.tick"),
                "must be ordered by non-decreasing tick",
            ));
        }
        previous_tick = Some(note.tick);
        if note.duration_ticks == 0
            || note.tick >= preview.length_ticks
            || note.duration_ticks > preview.length_ticks - note.tick
            || note.note > MAX_MIDI_NOTE
            || note.velocity == 0
            || note.velocity > MAX_MIDI_NOTE
            || recommended_range
                .is_some_and(|range| note.note < range.min_midi || note.note > range.max_midi)
        {
            return Err(metadata_error(
                instrument_id,
                &field,
                "contains an invalid note",
            ));
        }
    }
    Ok(())
}

fn is_valid_preview_denominator(value: u8) -> bool {
    value > 0 && value <= MAX_PREVIEW_DENOMINATOR && (value & (value - 1)) == 0
}

fn preview_duration_seconds(preview: &InstrumentPreviewDefinition) -> f64 {
    preview.length_ticks as f64 * 60.0 / (preview.ticks_per_beat as f64 * preview.tempo_bpm)
}

fn metadata_error(instrument_id: &str, field: &str, message: &str) -> String {
    format!("instrument '{instrument_id}' has invalid {field}: {message}")
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "user:01900000-0000-7000-8000-000000000001";

    fn definition(preview: Option<&str>) -> String {
        format!(
            r#"{{
                "metadata": {{
                    "name": "User Bass",
                    "author": "Composer",
                    "description": "A user instrument",
                    "category": "Bass",
                    "tags": ["Sub", "Warm"],
                    "recommended_range": {{"min_midi": 36, "max_midi": 60}}{}
                }}
            }}"#,
            preview.unwrap_or("")
        )
    }

    #[test]
    fn reads_snake_case_definition_metadata_into_the_control_projection() {
        let metadata = read_definition_metadata(
            &definition(Some(
                r#", "preview": {
                        "tempo_bpm": 120,
                        "ticks_per_beat": 480,
                        "time_signature": {"numerator": 4, "denominator": 4},
                        "length_ticks": 1920,
                        "notes": [{"tick": 0, "duration_ticks": 480, "note": 48, "velocity": 100}]
                    }"#,
            )),
            ID,
        )
        .unwrap();

        assert_eq!(metadata.name, "User Bass");
        assert_eq!(metadata.category.as_deref(), Some("Bass"));
        assert_eq!(metadata.tags, ["Sub", "Warm"]);
        assert_eq!(metadata.recommended_range.unwrap().max_midi, 60);
        assert_eq!(metadata.preview.unwrap().time_signature.numerator, 4);
    }

    #[test]
    fn accepts_missing_preview_but_rejects_invalid_preview_with_id_and_field() {
        let metadata = read_definition_metadata(&definition(None), ID).unwrap();
        assert!(metadata.preview.is_none());

        let invalid = definition(Some(
            r#", "preview": {
                "tempo_bpm": 301,
                "ticks_per_beat": 480,
                "time_signature": {"numerator": 4, "denominator": 4},
                "length_ticks": 1920,
                "notes": [{"tick": 0, "duration_ticks": 480, "note": 48, "velocity": 100}]
            }"#,
        ));
        let error = read_definition_metadata(&invalid, ID).unwrap_err();
        assert!(error.contains(ID));
        assert!(error.contains("metadata.preview.tempo_bpm"));
    }
}
