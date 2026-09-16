use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

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

/// Metadata presented to clients for one built-in instrument.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BuiltInInstrumentSummary {
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub category: String,
    pub tags: Vec<String>,
    pub recommended_range: InstrumentRecommendedRange,
    pub preview: InstrumentPreviewDefinition,
}

/// The MIDI range in which an instrument is intended to be used.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentRecommendedRange {
    pub min_midi: u8,
    pub max_midi: u8,
}

/// The deterministic MIDI pattern used for built-in instrument previews.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentPreviewDefinition {
    pub tempo_bpm: f64,
    pub ticks_per_beat: u16,
    pub time_signature: InstrumentPreviewTimeSignature,
    pub length_ticks: u64,
    pub notes: Vec<InstrumentPreviewNote>,
}

/// The meter used by an instrument preview.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentPreviewTimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

/// One MIDI note in an instrument preview.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentPreviewNote {
    pub tick: u64,
    pub duration_ticks: u64,
    pub note: u8,
    pub velocity: u8,
}

/// A resolved built-in instrument definition retained by the Host.
#[derive(Clone, Debug)]
pub struct BuiltInInstrumentDefinition {
    pub summary: BuiltInInstrumentSummary,
    pub definition_json: String,
    pub base_dir: PathBuf,
}

/// Immutable catalog loaded from the composition root's resource directory.
#[derive(Clone, Debug)]
pub struct BuiltInInstrumentCatalog {
    root: PathBuf,
    definitions: BTreeMap<String, BuiltInInstrumentDefinition>,
    errors: Vec<String>,
    invalid_preset_ids: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceManifest {
    source_release: String,
    presets: Vec<ResourceManifestPreset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceManifestPreset {
    id: String,
    name: String,
    author: Option<String>,
    #[serde(default)]
    description: Option<String>,
    category: String,
    tags: Vec<String>,
    recommended_range: InstrumentRecommendedRange,
    preview: InstrumentPreviewDefinition,
    definition_path: String,
    resource_base_path: String,
}

impl BuiltInInstrumentCatalog {
    /// Loads and validates the resource directory once for the Host lifetime.
    ///
    /// A missing or unreadable individual definition is reported through
    /// [`Self::errors`] and does not prevent the remaining catalog from loading.
    /// A malformed resource manifest is a packaging error and prevents catalog
    /// creation. Definition contents remain opaque to Riffra.
    pub fn load(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        if !root.is_dir() {
            return Err(format!(
                "built-in instrument resource root is not a directory: {}",
                root.display()
            ));
        }

        let manifest = read_manifest(&root)?;
        if manifest.source_release.trim().is_empty() {
            return Err("built-in instrument resource manifest has no sourceRelease".into());
        }

        let manifest_ids = manifest
            .presets
            .iter()
            .map(|preset| preset.id.trim().to_owned())
            .collect::<Vec<_>>();
        if manifest_ids.iter().any(String::is_empty) {
            return Err("built-in instrument resource manifest contains an empty preset id".into());
        }
        let mut sorted_manifest_ids = manifest_ids.clone();
        sorted_manifest_ids.sort();
        if manifest_ids != sorted_manifest_ids {
            return Err("built-in instrument resource manifest preset list is not sorted".into());
        }
        if manifest_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(
                "built-in instrument resource manifest contains duplicate preset ids".into(),
            );
        }

        let mut definitions = BTreeMap::new();
        let mut errors = Vec::new();
        let mut invalid_preset_ids = BTreeSet::new();
        for preset in manifest.presets {
            let id = preset.id.trim().to_owned();
            let name = preset.name.trim().to_owned();
            if name.is_empty() {
                return Err(format!(
                    "built-in instrument resource manifest preset '{id}' has no name"
                ));
            }
            let author = normalize_optional_text(preset.author);
            let description = normalize_optional_text(preset.description);
            validate_manifest_text(&preset.category, MAX_CATEGORY_CHARS, "category", &id)?;
            let category = preset.category;
            let tags = validate_tags(preset.tags, &id)?;
            validate_summary_metadata(
                &id,
                &category,
                &tags,
                &preset.recommended_range,
                &preset.preview,
            )?;
            let definition_path =
                resolve_bundle_path(&root, &preset.definition_path, "definitionPath")?;
            let base_dir =
                resolve_bundle_path(&root, &preset.resource_base_path, "resourceBasePath")?;
            if !base_dir.is_dir() {
                invalid_preset_ids.insert(id.clone());
                errors.push(format!(
                    "built-in instrument preset '{id}' resource base directory is missing: {}",
                    base_dir.display()
                ));
                continue;
            }
            let definition_json = match fs::read_to_string(&definition_path) {
                Ok(definition_json) => definition_json,
                Err(error) => {
                    invalid_preset_ids.insert(id.clone());
                    errors.push(format!(
                        "built-in instrument preset '{id}' definition could not be read: {error}"
                    ));
                    continue;
                }
            };
            definitions.insert(
                id.clone(),
                BuiltInInstrumentDefinition {
                    summary: BuiltInInstrumentSummary {
                        id,
                        name,
                        author,
                        description,
                        category,
                        tags,
                        recommended_range: preset.recommended_range,
                        preview: preset.preview,
                    },
                    definition_json,
                    base_dir,
                },
            );
        }

        Ok(Self {
            root,
            definitions,
            errors,
            invalid_preset_ids,
        })
    }

    /// Returns the resource root used to resolve built-in resource paths.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns stable, preset-id-sorted client metadata.
    pub fn summaries(&self) -> Vec<BuiltInInstrumentSummary> {
        self.definitions
            .values()
            .map(|definition| definition.summary.clone())
            .collect()
    }

    /// Returns catalog diagnostics for individual invalid preset entries.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Resolves one preset for canonical assignment or native projection.
    pub fn resolve(&self, preset_id: &str) -> Result<&BuiltInInstrumentDefinition, String> {
        self.definitions.get(preset_id).ok_or_else(|| {
            if self.invalid_preset_ids.contains(preset_id) {
                format!("built-in instrument preset is invalid: {preset_id}")
            } else {
                format!("built-in instrument preset is not available: {preset_id}")
            }
        })
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_owned()))
}

fn validate_manifest_text(
    value: &str,
    maximum_chars: usize,
    field: &str,
    id: &str,
) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > maximum_chars
        || value.chars().any(|character| character.is_control())
    {
        return Err(format!(
            "built-in instrument preset '{id}' has an invalid {field}"
        ));
    }
    Ok(())
}

fn validate_tags(tags: Vec<String>, id: &str) -> Result<Vec<String>, String> {
    if tags.is_empty() {
        return Err(format!("built-in instrument preset '{id}' has no tags"));
    }
    if tags.len() > MAX_TAG_COUNT {
        return Err(format!(
            "built-in instrument preset '{id}' has too many tags"
        ));
    }
    let mut seen = BTreeSet::new();
    for tag in &tags {
        validate_manifest_text(tag, MAX_TAG_CHARS, "tag", id)?;
        if !seen.insert(tag.to_ascii_lowercase()) {
            return Err(format!(
                "built-in instrument preset '{id}' has duplicate tags"
            ));
        }
    }
    Ok(tags)
}

fn validate_summary_metadata(
    id: &str,
    category: &str,
    tags: &[String],
    range: &InstrumentRecommendedRange,
    preview: &InstrumentPreviewDefinition,
) -> Result<(), String> {
    if category.is_empty() {
        return Err(format!("built-in instrument preset '{id}' has no category"));
    }
    if tags.is_empty() {
        return Err(format!("built-in instrument preset '{id}' has no tags"));
    }
    if range.min_midi > MAX_MIDI_NOTE
        || range.max_midi > MAX_MIDI_NOTE
        || range.min_midi > range.max_midi
    {
        return Err(format!(
            "built-in instrument preset '{id}' has an invalid recommended MIDI range"
        ));
    }
    if !preview.tempo_bpm.is_finite()
        || preview.tempo_bpm < MIN_PREVIEW_TEMPO_BPM
        || preview.tempo_bpm > MAX_PREVIEW_TEMPO_BPM
    {
        return Err(format!(
            "built-in instrument preset '{id}' has an invalid preview tempo"
        ));
    }
    if preview.ticks_per_beat < MIN_PREVIEW_TICKS_PER_BEAT
        || preview.ticks_per_beat > MAX_PREVIEW_TICKS_PER_BEAT
    {
        return Err(format!(
            "built-in instrument preset '{id}' has an invalid preview ticks-per-beat value"
        ));
    }
    if preview.time_signature.numerator == 0
        || preview.time_signature.numerator > MAX_PREVIEW_NUMERATOR
        || !is_valid_preview_denominator(preview.time_signature.denominator)
    {
        return Err(format!(
            "built-in instrument preset '{id}' has an invalid preview time signature"
        ));
    }
    if preview.length_ticks == 0 || preview_duration_seconds(preview) > MAX_PREVIEW_DURATION_SECONDS
    {
        return Err(format!(
            "built-in instrument preset '{id}' has an invalid preview length"
        ));
    }
    if !(MIN_PREVIEW_NOTES..=MAX_PREVIEW_NOTES).contains(&preview.notes.len()) {
        return Err(format!(
            "built-in instrument preset '{id}' has an invalid preview note count"
        ));
    }
    let mut previous_tick = None;
    for note in &preview.notes {
        if previous_tick.is_some_and(|previous| note.tick < previous) {
            return Err(format!(
                "built-in instrument preset '{id}' has unsorted preview notes"
            ));
        }
        previous_tick = Some(note.tick);
        if note.duration_ticks == 0
            || note.tick >= preview.length_ticks
            || note.duration_ticks > preview.length_ticks - note.tick
            || note.note < range.min_midi
            || note.note > range.max_midi
            || note.velocity == 0
            || note.note > MAX_MIDI_NOTE
            || note.velocity > MAX_MIDI_NOTE
        {
            return Err(format!(
                "built-in instrument preset '{id}' has an invalid preview note"
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

fn resolve_bundle_path(root: &Path, value: &str, field: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "built-in instrument resource manifest has an invalid {field}: {value}"
        ));
    }
    Ok(root.join(path))
}

fn read_manifest(root: &Path) -> Result<ResourceManifest, String> {
    let path = root.join("manifest.json");
    if !path.is_file() {
        return Err(format!(
            "built-in instrument resource manifest is missing: {}",
            path.display()
        ));
    }
    let contents = fs::read_to_string(&path).map_err(|error| {
        format!("built-in instrument resource manifest could not be read: {error}")
    })?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("built-in instrument resource manifest is invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("riffra-builtins-{suffix}"));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_definition(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn manifest_entry(
        id: &str,
        name: &str,
        description: Option<&str>,
        definition_path: &str,
        resource_base_path: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": name,
            "description": description,
            "author": "Riffra",
            "category": "Test",
            "tags": ["test"],
            "recommendedRange": {"minMidi": 36, "maxMidi": 84},
            "preview": {
                "tempoBpm": 120.0,
                "ticksPerBeat": 480,
                "timeSignature": {"numerator": 4, "denominator": 4},
                "lengthTicks": 1920,
                "notes": [{"tick": 0, "durationTicks": 480, "note": 48, "velocity": 100}]
            },
            "definitionPath": definition_path,
            "resourceBasePath": resource_base_path,
        })
    }

    fn write_manifest(root: &Path, presets: &[serde_json::Value]) {
        fs::write(
            root.join("manifest.json"),
            serde_json::json!({
                "sourceRelease": "vtest",
                "presets": presets,
            })
            .to_string(),
        )
        .unwrap();
    }

    fn load_single_preset_error(preset: serde_json::Value) -> String {
        let root = TempRoot::new();
        write_definition(&root.0, "sound.data", "opaque");
        fs::create_dir_all(root.0.join("resources")).unwrap();
        write_manifest(&root.0, &[preset]);
        BuiltInInstrumentCatalog::load(&root.0).unwrap_err()
    }

    fn assert_rejected(preset: serde_json::Value, message: &str) {
        let error = load_single_preset_error(preset);
        assert!(
            error.contains(message),
            "expected error containing '{message}', got '{error}'"
        );
    }

    #[test]
    fn loads_catalog_from_manifest_and_keeps_definition_opaque() {
        let root = TempRoot::new();
        write_definition(
            &root.0,
            "arbitrary/location/sound.data",
            r#"{"completelyOpaque":"value"}"#,
        );
        fs::create_dir_all(root.0.join("resources/first")).unwrap();
        fs::create_dir(root.0.join("unlisted-directory")).unwrap();
        write_manifest(
            &root.0,
            &[manifest_entry(
                "01-first",
                "First",
                Some("First description"),
                "arbitrary/location/sound.data",
                "resources/first",
            )],
        );

        let catalog = BuiltInInstrumentCatalog::load(&root.0).unwrap();

        let summaries = catalog.summaries();
        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.id.as_str())
                .collect::<Vec<_>>(),
            ["01-first"]
        );
        let definition = catalog.resolve("01-first").unwrap();
        assert_eq!(
            definition.definition_json,
            r#"{"completelyOpaque":"value"}"#
        );
        assert_eq!(definition.base_dir, root.0.join("resources/first"));
        assert!(catalog.errors().is_empty());
    }

    #[test]
    fn invalid_json_is_retained_as_an_opaque_definition() {
        let root = TempRoot::new();
        write_definition(&root.0, "sound.data", "not-json");
        fs::create_dir_all(root.0.join("resources")).unwrap();
        write_manifest(
            &root.0,
            &[manifest_entry(
                "01-opaque",
                "Opaque",
                None,
                "sound.data",
                "resources",
            )],
        );

        let catalog = BuiltInInstrumentCatalog::load(&root.0).unwrap();

        assert_eq!(
            catalog.resolve("01-opaque").unwrap().definition_json,
            "not-json"
        );
        assert!(catalog.errors().is_empty());
    }

    #[test]
    fn manifest_is_required() {
        let root = TempRoot::new();

        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();

        assert!(error.contains("manifest is missing"));
    }

    #[test]
    fn manifest_requires_sorted_unique_ids() {
        let root = TempRoot::new();
        write_definition(&root.0, "first.data", "first");
        write_definition(&root.0, "second.data", "second");
        fs::create_dir_all(root.0.join("resources/first")).unwrap();
        fs::create_dir_all(root.0.join("resources/second")).unwrap();
        write_manifest(
            &root.0,
            &[
                manifest_entry(
                    "02-second",
                    "Second",
                    None,
                    "second.data",
                    "resources/second",
                ),
                manifest_entry("01-first", "First", None, "first.data", "resources/first"),
            ],
        );

        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();

        assert!(error.contains("not sorted"));

        write_manifest(
            &root.0,
            &[
                manifest_entry("01-first", "First", None, "first.data", "resources/first"),
                manifest_entry(
                    "01-first",
                    "First again",
                    None,
                    "first.data",
                    "resources/first",
                ),
            ],
        );
        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();
        assert!(error.contains("duplicate preset ids"));
    }

    #[test]
    fn rejects_preview_notes_outside_recommended_range() {
        let root = TempRoot::new();
        write_definition(&root.0, "sound.data", "opaque");
        fs::create_dir_all(root.0.join("resources")).unwrap();
        let mut preset = manifest_entry(
            "01-invalid-preview",
            "Invalid Preview",
            None,
            "sound.data",
            "resources",
        );
        preset["preview"]["notes"][0]["note"] = serde_json::json!(85);
        write_manifest(&root.0, &[preset]);

        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();

        assert!(error.contains("invalid preview note"));
    }

    #[test]
    fn accepts_power_of_two_preview_denominators_through_128() {
        for denominator in [64, 128] {
            let root = TempRoot::new();
            write_definition(&root.0, "sound.data", "opaque");
            fs::create_dir_all(root.0.join("resources")).unwrap();
            let mut preset = manifest_entry(
                "01-valid-meter",
                "Valid Meter",
                None,
                "sound.data",
                "resources",
            );
            preset["preview"]["timeSignature"]["denominator"] = serde_json::json!(denominator);
            write_manifest(&root.0, &[preset]);

            let catalog = BuiltInInstrumentCatalog::load(&root.0).unwrap();

            assert_eq!(
                catalog.summaries()[0].preview.time_signature.denominator,
                denominator
            );
        }
    }

    #[test]
    fn accepts_maximum_preview_numerator_and_equal_note_ticks() {
        let root = TempRoot::new();
        write_definition(&root.0, "sound.data", "opaque");
        fs::create_dir_all(root.0.join("resources")).unwrap();
        let mut preset = manifest_entry(
            "01-valid-preview",
            "Valid Preview",
            None,
            "sound.data",
            "resources",
        );
        preset["preview"]["timeSignature"]["numerator"] = serde_json::json!(32);
        preset["preview"]["notes"] = serde_json::json!([
            {"tick": 0, "durationTicks": 480, "note": 48, "velocity": 100},
            {"tick": 0, "durationTicks": 240, "note": 52, "velocity": 100}
        ]);
        write_manifest(&root.0, &[preset]);

        let catalog = BuiltInInstrumentCatalog::load(&root.0).unwrap();

        assert_eq!(catalog.summaries()[0].preview.time_signature.numerator, 32);
        assert_eq!(catalog.summaries()[0].preview.notes.len(), 2);
    }

    #[test]
    fn rejects_manifest_text_and_preview_order_violations() {
        let base = || {
            manifest_entry(
                "01-invalid-text",
                "Invalid Text",
                None,
                "sound.data",
                "resources",
            )
        };

        let mut preset = base();
        preset["category"] = serde_json::json!(" Test");
        assert_rejected(preset, "invalid category");

        let mut preset = base();
        preset["category"] = serde_json::json!("x".repeat(65));
        assert_rejected(preset, "invalid category");

        let mut preset = base();
        preset["category"] = serde_json::json!("Test\n");
        assert_rejected(preset, "invalid category");

        let mut preset = base();
        preset["tags"] = serde_json::Value::Array(vec![serde_json::json!("tag"); 13]);
        assert_rejected(preset, "too many tags");

        let mut preset = base();
        preset["tags"] = serde_json::json!([" tag"]);
        assert_rejected(preset, "invalid tag");

        let mut preset = base();
        preset["tags"] = serde_json::json!(["x".repeat(33)]);
        assert_rejected(preset, "invalid tag");

        let mut preset = base();
        preset["tags"] = serde_json::json!(["tag\n"]);
        assert_rejected(preset, "invalid tag");

        let mut preset = base();
        preset["tags"] = serde_json::json!(["Test", "test"]);
        assert_rejected(preset, "duplicate tags");

        let mut preset = base();
        preset["preview"]["timeSignature"]["numerator"] = serde_json::json!(33);
        assert_rejected(preset, "invalid preview time signature");

        let mut preset = base();
        preset["preview"]["notes"] = serde_json::json!([
            {"tick": 480, "durationTicks": 240, "note": 48, "velocity": 100},
            {"tick": 0, "durationTicks": 240, "note": 52, "velocity": 100}
        ]);
        assert_rejected(preset, "unsorted preview notes");
    }

    #[test]
    fn rejects_manifest_values_outside_public_preview_contract() {
        let base = || {
            manifest_entry(
                "01-invalid-contract",
                "Invalid Contract",
                None,
                "sound.data",
                "resources",
            )
        };

        let mut preset = base();
        preset["recommendedRange"]["maxMidi"] = serde_json::json!(128);
        assert_rejected(preset, "invalid recommended MIDI range");

        let mut preset = base();
        preset["preview"]["tempoBpm"] = serde_json::json!(29.9);
        assert_rejected(preset, "invalid preview tempo");

        let mut preset = base();
        preset["preview"]["tempoBpm"] = serde_json::json!(300.1);
        assert_rejected(preset, "invalid preview tempo");

        let mut preset = base();
        preset["preview"]["ticksPerBeat"] = serde_json::json!(32_768);
        assert_rejected(preset, "invalid preview ticks-per-beat");

        let mut preset = base();
        preset["preview"]["timeSignature"]["denominator"] = serde_json::json!(3);
        assert_rejected(preset, "invalid preview time signature");

        let mut preset = base();
        preset["preview"]["lengthTicks"] = serde_json::json!(9_601);
        assert_rejected(preset, "invalid preview length");

        let mut preset = base();
        preset["preview"]["notes"] = serde_json::json!([]);
        assert_rejected(preset, "invalid preview note count");

        let mut preset = base();
        let note = preset["preview"]["notes"][0].clone();
        preset["preview"]["notes"] = serde_json::Value::Array(vec![note; 33]);
        assert_rejected(preset, "invalid preview note count");

        let mut preset = base();
        preset["preview"]["notes"][0]["velocity"] = serde_json::json!(128);
        assert_rejected(preset, "invalid preview note");
    }
}
