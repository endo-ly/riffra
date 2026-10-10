//! Sonalloy Bundle Format v1 file boundary.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
const MAX_DEPTH: usize = 32;

/// A diagnostic retaining the file or field that failed validation.
#[derive(Debug, thiserror::Error)]
#[error("sonalloy bundle {location}: {reason}")]
pub struct BundleError {
    /// Bundle-relative file or JSON field location, including the Part ID.
    pub location: String,
    /// Structured failure category.
    pub reason: BundleErrorKind,
}

/// Failure categories for the Bundle file boundary.
#[derive(Debug, thiserror::Error)]
pub enum BundleErrorKind {
    /// File access failed.
    #[error("file access failed: {0}")]
    Io(#[source] io::Error),
    /// JSON does not conform to the supported schema.
    #[error("invalid schema: {0}")]
    Schema(String),
    /// A manifest, path, or hash invariant failed.
    #[error("invalid manifest: {0}")]
    Manifest(String),
    /// Musical data or references cannot be executed.
    #[error("invalid demo: {0}")]
    Demo(String),
}

fn error(location: impl Into<String>, reason: BundleErrorKind) -> BundleError {
    BundleError {
        location: location.into(),
        reason,
    }
}

fn manifest_error(location: impl Into<String>, reason: impl Into<String>) -> BundleError {
    error(location, BundleErrorKind::Manifest(reason.into()))
}

fn demo_error(location: impl Into<String>, reason: impl Into<String>) -> BundleError {
    error(location, BundleErrorKind::Demo(reason.into()))
}

/// Verified files and typed performance data for the runtime import adapter.
#[derive(Debug)]
pub struct Bundle {
    /// Instrument Definition text in Demo Part order.
    pub instrument_definitions: Vec<String>,
    /// Canonical Bundle directory; only used during import.
    pub root: PathBuf,
    /// Versioned manifest and reference render conditions.
    pub manifest: BundleManifest,
    /// Demo parts in source definition order.
    pub demo: DemoDefinition,
    /// Patterns in the same order as Demo parts.
    pub patterns: Vec<PatternDefinition>,
}

/// Bundle Format v1 manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub format_version: u32,
    pub demo: String,
    pub render_settings: RenderSettings,
    // Option normally accepts a missing field. v1 requires an explicit null.
    #[serde(deserialize_with = "deserialize_render")]
    pub render: Option<RenderReferences>,
    pub files: Vec<ManifestFile>,
}

fn deserialize_render<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<RenderReferences>, D::Error> {
    Option::deserialize(deserializer)
}

/// Reference conditions for offline reproduction.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSettings {
    pub sample_rate: u32,
    pub block_size: u32,
    pub tail_seconds: f64,
}

/// Optional comparison WAV references, never used as musical source data.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderReferences {
    pub mix: String,
    pub stems: BTreeMap<String, String>,
}

/// One registered regular file and its content digest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
}

/// Demo Schema v1 data used by the adapter.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoDefinition {
    pub schema_version: u32,
    pub name: Option<String>,
    pub parts: Vec<DemoPart>,
    #[serde(default)]
    pub mix: DemoMix,
}

/// One Instrument Track's source references and mix settings.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoPart {
    pub id: String,
    pub instrument: String,
    pub pattern: String,
    #[serde(default)]
    pub gain_db: f64,
    pub midi_channel: Option<u8>,
    pub audio_input: Option<DemoAudioInput>,
}

/// A single source Part for an Instrument's external audio input.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoAudioInput {
    pub part: String,
}

/// Master mix settings independent of the source Bundle.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoMix {
    #[serde(default)]
    pub fade_out_seconds: f64,
    pub master: Option<MasteringSettings>,
}

/// FFmpeg loudnorm targets used by Sonalloy Demo rendering.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MasteringSettings {
    pub integrated_lufs: f64,
    pub true_peak_db: f64,
    pub loudness_range_lu: f64,
}

/// Pattern Schema v1 with lossless, source-ordered events.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternDefinition {
    pub schema_version: u32,
    pub name: Option<String>,
    pub ticks_per_beat: u16,
    pub length_ticks: u64,
    pub tempo_changes: Vec<TempoChange>,
    pub time_signature_changes: Vec<TimeSignatureChange>,
    pub events: Vec<PatternEvent>,
}

/// A source musical tick and tempo.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TempoChange {
    pub tick: u64,
    pub bpm: f64,
}

/// A source musical tick and time signature.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeSignatureChange {
    pub tick: u64,
    pub numerator: u8,
    pub denominator: u8,
}

/// The complete set of supported source performance events.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternEvent {
    Note {
        tick: u64,
        duration_ticks: u64,
        note: u8,
        velocity: u8,
    },
    SustainPedal {
        tick: u64,
        down: bool,
    },
    PitchBend {
        tick: u64,
        value: f32,
    },
    ModWheel {
        tick: u64,
        value: f32,
    },
    Aftertouch {
        tick: u64,
        value: f32,
    },
    ParameterChange {
        tick: u64,
        parameter: String,
        native_value: f32,
    },
}

/// Reads and validates a complete Bundle without modifying any Project.
///
/// # Errors
/// Returns a located diagnostic for unsafe paths, unregistered files, invalid
/// hashes, unsupported schemas, or inconsistent Demo data. Instrument compile
/// validation belongs to the runtime's bundled Sonalloy CLI integration.
pub fn read(root: &Path) -> Result<Bundle, BundleError> {
    let metadata = fs::symlink_metadata(root).map_err(|e| error("root", BundleErrorKind::Io(e)))?;
    if is_link(&metadata) || !metadata.is_dir() {
        return Err(manifest_error("root", "expected a regular directory"));
    }
    let root = fs::canonicalize(root).map_err(|e| error("root", BundleErrorKind::Io(e)))?;
    let manifest: BundleManifest = read_json(&root.join("bundle.json"), "bundle.json")?;
    if manifest.format_version != 1 {
        return Err(manifest_error(
            "format_version",
            "unsupported format version; expected 1",
        ));
    }
    if manifest.demo != "demo.json" {
        return Err(manifest_error("demo", "expected demo.json"));
    }
    let settings = &manifest.render_settings;
    if settings.sample_rate == 0
        || settings.block_size == 0
        || !settings.tail_seconds.is_finite()
        || settings.tail_seconds < 0.0
    {
        return Err(manifest_error(
            "render_settings",
            "expected positive sample rate and block size and a finite non-negative tail",
        ));
    }
    if manifest.files.len() > MAX_FILES {
        return Err(manifest_error("files", "file count exceeds bundle limit"));
    }
    let mut paths = BTreeSet::new();
    let mut folded_paths = BTreeSet::new();
    let mut previous = None;
    for (i, file) in manifest.files.iter().enumerate() {
        let location = format!("files[{i}]");
        validate_path(&file.path).map_err(|reason| manifest_error(&location, reason))?;
        if file.path == "bundle.json"
            || previous.is_some_and(|path: &str| path >= file.path.as_str())
        {
            return Err(manifest_error(
                &location,
                "paths must be strictly sorted and exclude bundle.json",
            ));
        }
        if !folded_paths.insert(file.path.to_lowercase()) {
            return Err(manifest_error(&location, "case-insensitive path collision"));
        }
        if file.sha256.len() != 64
            || !file
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(manifest_error(
                &location,
                "sha256 must contain 64 lowercase hexadecimal digits",
            ));
        }
        paths.insert(file.path.clone());
        previous = Some(&file.path);
    }
    let mut actual = BTreeSet::new();
    let mut total_bytes = 0;
    let mut entry_count = 0;
    enumerate(
        &root,
        &root,
        0,
        &mut actual,
        &mut total_bytes,
        &mut entry_count,
    )?;
    actual.remove("bundle.json");
    if actual != paths {
        let missing = paths.difference(&actual).next();
        let unregistered = actual.difference(&paths).next();
        return Err(manifest_error(
            "files",
            format!(
                "file inventory mismatch (missing: {missing:?}, unregistered: {unregistered:?})"
            ),
        ));
    }
    for file in &manifest.files {
        let digest = hash_file(&root.join(&file.path), &file.path)?;
        if digest != file.sha256 {
            return Err(manifest_error(&file.path, "sha256 mismatch"));
        }
    }
    let demo: DemoDefinition = read_json(&root.join(&manifest.demo), "demo.json")?;
    if demo.schema_version != 1 || demo.parts.is_empty() {
        return Err(demo_error(
            "demo.json",
            "expected schema version 1 with at least one part",
        ));
    }
    let mut ids = BTreeMap::new();
    let mut patterns = Vec::with_capacity(demo.parts.len());
    let mut instrument_definitions = Vec::with_capacity(demo.parts.len());
    for (index, part) in demo.parts.iter().enumerate() {
        let location = format!("parts[{index}] (part {})", part.id);
        validate_path(&part.id).map_err(|e| demo_error(&location, e))?;
        if part.id.contains('/') || ids.insert(part.id.as_str(), index).is_some() {
            return Err(demo_error(
                &location,
                "part ids must be unique single path components",
            ));
        }
        if part.instrument != format!("instruments/{}/definition.json", part.id)
            || part.pattern != format!("patterns/{}.json", part.id)
        {
            return Err(demo_error(
                &location,
                "instrument and pattern paths must match the part id",
            ));
        }
        for reference in [&part.instrument, &part.pattern] {
            if !paths.contains(reference) {
                return Err(demo_error(
                    &location,
                    format!("unregistered reference {reference}"),
                ));
            }
        }
        let (definition, text): (serde_json::Value, String) = read_json_document(
            &root.join(&part.instrument),
            &format!("{location}.instrument"),
        )?;
        validate_asset_paths(
            &definition,
            &format!("instruments/{}", part.id),
            &paths,
            &format!("{location}.instrument"),
        )?;
        instrument_definitions.push(text);
        if !part.gain_db.is_finite() || part.midi_channel.is_some_and(|c| !(1..=16).contains(&c)) {
            return Err(demo_error(&location, "invalid gain or midi channel"));
        }
        let pattern: PatternDefinition =
            read_json(&root.join(&part.pattern), &format!("{location}.pattern"))?;
        validate_pattern(&pattern, &format!("{location}.pattern"))?;
        patterns.push(pattern);
    }
    let longest = patterns
        .iter()
        .max_by_key(|pattern| pattern.length_ticks)
        .expect("demo has parts");
    for (index, pattern) in patterns.iter().enumerate() {
        if pattern.ticks_per_beat != longest.ticks_per_beat
            || !pattern.tempo_changes.iter().eq(longest
                .tempo_changes
                .iter()
                .filter(|change| change.tick <= pattern.length_ticks))
            || !pattern.time_signature_changes.iter().eq(longest
                .time_signature_changes
                .iter()
                .filter(|change| change.tick <= pattern.length_ticks))
        {
            return Err(demo_error(
                format!("parts[{index}].pattern (part {})", demo.parts[index].id),
                "parts must share the same musical timebase within their pattern length",
            ));
        }
    }
    for (index, part) in demo.parts.iter().enumerate() {
        let mut visited = BTreeSet::new();
        let mut current = index;
        while let Some(input) = &demo.parts[current].audio_input {
            if !visited.insert(current) {
                return Err(demo_error(
                    format!("parts[{index}].audio_input (part {})", part.id),
                    "cyclic external audio route",
                ));
            }
            current = *ids.get(input.part.as_str()).ok_or_else(|| {
                demo_error(
                    format!(
                        "parts[{current}].audio_input (part {})",
                        demo.parts[current].id
                    ),
                    "source part does not exist",
                )
            })?;
        }
    }
    if let Some(render) = &manifest.render {
        if render.mix != "render/mix.wav" || !paths.contains(&render.mix) {
            return Err(manifest_error(
                "render.mix",
                "expected registered render/mix.wav",
            ));
        }
        let part_ids = demo
            .parts
            .iter()
            .map(|part| part.id.as_str())
            .collect::<BTreeSet<_>>();
        let stem_ids = render
            .stems
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if stem_ids != part_ids {
            return Err(manifest_error(
                "render.stems",
                "expected exactly one stem for every demo part",
            ));
        }
        for (id, path) in &render.stems {
            if *path != format!("render/stems/{id}.wav") || !paths.contains(path) {
                return Err(manifest_error(
                    format!("render.stems.{id}"),
                    "invalid or unregistered stem reference",
                ));
            }
        }
    } else if paths.iter().any(|path| path.starts_with("render/")) {
        return Err(manifest_error(
            "render",
            "render files require render references",
        ));
    }
    if !demo.mix.fade_out_seconds.is_finite() || demo.mix.fade_out_seconds < 0.0 {
        return Err(demo_error(
            "mix.fade_out_seconds",
            "expected a finite non-negative duration",
        ));
    }
    if let Some(master) = &demo.mix.master
        && (!master.integrated_lufs.is_finite()
            || !(-70.0..=-5.0).contains(&master.integrated_lufs)
            || !master.true_peak_db.is_finite()
            || !(-9.0..=0.0).contains(&master.true_peak_db)
            || !master.loudness_range_lu.is_finite()
            || !(1.0..=50.0).contains(&master.loudness_range_lu))
    {
        return Err(demo_error("mix.master", "invalid loudnorm targets"));
    }
    Ok(Bundle {
        instrument_definitions,
        root,
        manifest,
        demo,
        patterns,
    })
}

fn validate_pattern(pattern: &PatternDefinition, location: &str) -> Result<(), BundleError> {
    if pattern.schema_version != 1
        || pattern.ticks_per_beat == 0
        || pattern.ticks_per_beat > 32_767
        || pattern.length_ticks == 0
    {
        return Err(demo_error(
            location,
            "expected schema version 1, positive length and ppq between 1 and 32767",
        ));
    }
    if pattern.tempo_changes.first().is_none_or(|p| p.tick != 0)
        || pattern
            .tempo_changes
            .windows(2)
            .any(|p| p[0].tick >= p[1].tick)
    {
        return Err(demo_error(
            format!("{location}.tempo_changes"),
            "changes must start at zero and be strictly sorted",
        ));
    }
    for (i, point) in pattern.tempo_changes.iter().enumerate() {
        if !point.bpm.is_finite() || point.bpm <= 0.0 || point.tick > pattern.length_ticks {
            return Err(demo_error(
                format!("{location}.tempo_changes[{i}]"),
                "invalid tempo or position",
            ));
        }
    }
    if pattern
        .time_signature_changes
        .first()
        .is_none_or(|p| p.tick != 0)
        || pattern
            .time_signature_changes
            .windows(2)
            .any(|p| p[0].tick >= p[1].tick)
    {
        return Err(demo_error(
            format!("{location}.time_signature_changes"),
            "changes must start at zero and be strictly sorted",
        ));
    }
    for (i, point) in pattern.time_signature_changes.iter().enumerate() {
        if point.numerator == 0
            || !matches!(point.denominator, 1 | 2 | 4 | 8 | 16 | 32)
            || point.tick > pattern.length_ticks
        {
            return Err(demo_error(
                format!("{location}.time_signature_changes[{i}]"),
                "invalid time signature or position",
            ));
        }
    }
    if pattern.events.len() > 200_000 {
        return Err(demo_error(
            format!("{location}.events"),
            "event count exceeds clip limit",
        ));
    }
    for (i, event) in pattern.events.iter().enumerate() {
        let (tick, valid) = match event {
            PatternEvent::Note {
                tick,
                duration_ticks,
                note,
                velocity,
            } => (
                *tick,
                *note <= 127
                    && *velocity > 0
                    && *velocity <= 127
                    && *duration_ticks > 0
                    && tick
                        .checked_add(*duration_ticks)
                        .is_some_and(|end| end <= pattern.length_ticks),
            ),
            PatternEvent::SustainPedal { tick, .. } => (*tick, true),
            PatternEvent::PitchBend { tick, value } => {
                (*tick, value.is_finite() && (-1.0..=1.0).contains(value))
            }
            PatternEvent::ModWheel { tick, value } | PatternEvent::Aftertouch { tick, value } => {
                (*tick, value.is_finite() && (0.0..=1.0).contains(value))
            }
            PatternEvent::ParameterChange {
                tick,
                parameter,
                native_value,
            } => (
                *tick,
                !parameter.trim().is_empty() && native_value.is_finite(),
            ),
        };
        if tick > pattern.length_ticks || !valid {
            return Err(demo_error(
                format!("{location}.events[{i}]"),
                "invalid performance event",
            ));
        }
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty()
        || path.contains(['\\', ':', '\0'])
        || path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
        })
    {
        return Err("expected a safe slash-separated relative path");
    }
    Ok(())
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn validate_asset_paths(
    value: &serde_json::Value,
    directory: &str,
    files: &BTreeSet<String>,
    location: &str,
) -> Result<(), BundleError> {
    match value {
        serde_json::Value::Object(object) => {
            // AssetReference is the Definition contract's only file path field.
            // The compiler remains responsible for the surrounding generator schema.
            if let Some(path) = object.get("path") {
                let path = path.as_str().ok_or_else(|| {
                    demo_error(format!("{location}.path"), "asset path must be a string")
                })?;
                validate_path(path)
                    .map_err(|reason| demo_error(format!("{location}.path"), reason))?;
                if !path.starts_with("assets/") || !files.contains(&format!("{directory}/{path}")) {
                    return Err(demo_error(
                        format!("{location}.path"),
                        "asset must refer to a registered file inside the instrument assets directory",
                    ));
                }
            }
            for (field, child) in object {
                validate_asset_paths(child, directory, files, &format!("{location}.{field}"))?;
            }
        }
        serde_json::Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                validate_asset_paths(child, directory, files, &format!("{location}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn enumerate(
    root: &Path,
    directory: &Path,
    depth: usize,
    paths: &mut BTreeSet<String>,
    total: &mut u64,
    entry_count: &mut usize,
) -> Result<(), BundleError> {
    if depth > MAX_DEPTH {
        return Err(manifest_error(
            "files",
            "directory depth exceeds bundle limit",
        ));
    }
    let entries = fs::read_dir(directory).map_err(|e| error("files", BundleErrorKind::Io(e)))?;
    for entry in entries {
        *entry_count += 1;
        if *entry_count > MAX_FILES * 2 + 1 {
            return Err(manifest_error(
                "files",
                "directory inventory exceeds bundle limit",
            ));
        }
        let entry = entry.map_err(|e| error("files", BundleErrorKind::Io(e)))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("enumerated path belongs to root")
            .to_str()
            .ok_or_else(|| manifest_error("files", "non-utf8 path"))?
            .replace('\\', "/");
        validate_path(&relative).map_err(|e| manifest_error(&relative, e))?;
        let metadata =
            fs::symlink_metadata(&path).map_err(|e| error(&relative, BundleErrorKind::Io(e)))?;
        if is_link(&metadata) {
            return Err(manifest_error(
                &relative,
                "symbolic links and reparse points are forbidden",
            ));
        }
        if metadata.is_dir() {
            enumerate(root, &path, depth + 1, paths, total, entry_count)?;
        } else if metadata.is_file() {
            *total = total.saturating_add(metadata.len());
            if metadata.len() > MAX_FILE_BYTES
                || *total > MAX_BUNDLE_BYTES
                || paths.len() > MAX_FILES
            {
                return Err(manifest_error(
                    &relative,
                    "file inventory exceeds bundle size limit",
                ));
            }
            paths.insert(relative);
        } else {
            return Err(manifest_error(relative, "expected a regular file"));
        }
    }
    Ok(())
}

fn hash_file(path: &Path, location: &str) -> Result<String, BundleError> {
    let mut file = File::open(path).map_err(|e| error(location, BundleErrorKind::Io(e)))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| error(location, BundleErrorKind::Io(e)))?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > MAX_FILE_BYTES {
            return Err(manifest_error(location, "file exceeds bundle size limit"));
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    location: &str,
) -> Result<T, BundleError> {
    read_json_document(path, location).map(|(value, _)| value)
}

fn read_json_document<T: serde::de::DeserializeOwned>(
    path: &Path,
    location: &str,
) -> Result<(T, String), BundleError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| error(location, BundleErrorKind::Io(e)))?;
    if is_link(&metadata) || !metadata.is_file() || metadata.len() > MAX_JSON_BYTES {
        return Err(manifest_error(
            location,
            "expected a regular json file within size limit",
        ));
    }
    let file = File::open(path).map_err(|e| error(location, BundleErrorKind::Io(e)))?;
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| error(location, BundleErrorKind::Io(e)))?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(manifest_error(location, "json exceeds size limit"));
    }
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let value = serde_path_to_error::deserialize(&mut deserializer).map_err(|e| {
        error(
            format!("{location}.{}", e.path()),
            BundleErrorKind::Schema(e.inner().to_string()),
        )
    })?;
    deserializer
        .end()
        .map_err(|e| error(location, BundleErrorKind::Schema(e.to_string())))?;
    let text = String::from_utf8(bytes)
        .map_err(|e| error(location, BundleErrorKind::Schema(e.to_string())))?;
    Ok((value, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/sonalloy-bundle/basic")
    }

    fn copy_fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!("riffra-bundle-{}", uuid::Uuid::now_v7()));
        let source = fixture();
        let bundle = read(&source).unwrap();
        fs::create_dir(&root).unwrap();
        for path in std::iter::once("bundle.json")
            .chain(bundle.manifest.files.iter().map(|f| f.path.as_str()))
        {
            let destination = root.join(path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(source.join(path), destination).unwrap();
        }
        root
    }

    fn add_render_files(root: &Path, manifest: &mut Value) {
        let files = manifest["files"].as_array_mut().unwrap();
        for path in [
            "render/mix.wav",
            "render/stems/lead.wav",
            "render/stems/duck.wav",
        ] {
            let destination = root.join(path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(&destination, path.as_bytes()).unwrap();
            files.push(json!({
                "path": path,
                "sha256": hash_file(&destination, path).unwrap(),
            }));
        }
        files.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    }

    #[test]
    fn reads_source_order_controls_timebase_and_render_conditions() {
        let bundle = read(&fixture()).unwrap();

        assert_eq!(
            bundle
                .demo
                .parts
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["lead", "duck"]
        );
        assert_eq!(
            bundle.demo.parts[1].audio_input.as_ref().unwrap().part,
            "lead"
        );
        assert_eq!(bundle.patterns[0].events.len(), 8);
        assert!(matches!(
            bundle.patterns[0].events[7],
            PatternEvent::SustainPedal {
                tick: 1920,
                down: false
            }
        ));
        assert_eq!(bundle.patterns[0].tempo_changes[1].bpm, 90.0);
        assert_eq!(bundle.manifest.render_settings.sample_rate, 48000);
        assert!(bundle.manifest.render.is_none());
    }

    #[test]
    fn rejects_invalid_manifest_inventory_and_hash() {
        let root = copy_fixture();
        let original = fs::read(root.join("bundle.json")).unwrap();
        let manifest: Value = serde_json::from_slice(&original).unwrap();
        let cases = [
            ("format_version", json!(2)),
            ("render", json!({"mix":"../outside.wav", "stems":{}})),
            ("unknown", json!(true)),
            (
                "files",
                json!([{"path":"../outside", "sha256":"0".repeat(64)}]),
            ),
            (
                "files",
                json!([{"path":"DEMO.json", "sha256":"0".repeat(64)}, {"path":"demo.json", "sha256":"0".repeat(64)}]),
            ),
        ];
        for (key, value) in cases {
            let mut invalid = manifest.clone();
            invalid[key] = value;
            fs::write(
                root.join("bundle.json"),
                serde_json::to_vec(&invalid).unwrap(),
            )
            .unwrap();
            assert!(read(&root).is_err(), "accepted invalid {key}");
        }
        let mut missing_render = manifest.clone();
        missing_render.as_object_mut().unwrap().remove("render");
        fs::write(
            root.join("bundle.json"),
            serde_json::to_vec(&missing_render).unwrap(),
        )
        .unwrap();
        assert!(read(&root).is_err());
        fs::write(root.join("bundle.json"), original).unwrap();
        fs::write(root.join("extra.txt"), b"unregistered").unwrap();
        assert!(matches!(
            read(&root).unwrap_err().reason,
            BundleErrorKind::Manifest(_)
        ));
        fs::remove_file(root.join("extra.txt")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(fixture().join("demo.json"), root.join("linked.json"))
                .unwrap();
            assert!(
                read(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("symbolic links")
            );
            fs::remove_file(root.join("linked.json")).unwrap();
        }
        fs::write(root.join("demo.json"), b"{}").unwrap();
        assert!(
            read(&root)
                .unwrap_err()
                .to_string()
                .contains("sha256 mismatch")
        );
        fs::remove_file(root.join("demo.json")).unwrap();
        assert!(
            read(&root)
                .unwrap_err()
                .to_string()
                .contains("inventory mismatch")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepts_render_references_for_every_demo_part() {
        let root = copy_fixture();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(root.join("bundle.json")).unwrap()).unwrap();
        add_render_files(&root, &mut manifest);
        manifest["render"] = json!({
            "mix": "render/mix.wav",
            "stems": {
                "duck": "render/stems/duck.wav",
                "lead": "render/stems/lead.wav",
            },
        });
        fs::write(
            root.join("bundle.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        assert!(read(&root).is_ok());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_render_references_missing_a_demo_part_stem() {
        let root = copy_fixture();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(root.join("bundle.json")).unwrap()).unwrap();
        add_render_files(&root, &mut manifest);
        manifest["render"] = json!({
            "mix": "render/mix.wav",
            "stems": { "lead": "render/stems/lead.wav" },
        });
        fs::write(
            root.join("bundle.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let diagnostic = read(&root).unwrap_err();

        assert_eq!(diagnostic.location, "render.stems");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_render_files_without_render_references() {
        let root = copy_fixture();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(root.join("bundle.json")).unwrap()).unwrap();
        add_render_files(&root, &mut manifest);
        fs::write(
            root.join("bundle.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let diagnostic = read(&root).unwrap_err();

        assert_eq!(diagnostic.location, "render");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_escaping_and_unregistered_instrument_asset_references() {
        // Arrange
        let paths = BTreeSet::from(["instruments/lead/assets/tone.wav".to_owned()]);
        for path in [
            "../outside.wav",
            "/outside.wav",
            "C:/outside.wav",
            "assets\\tone.wav",
            "assets/missing.wav",
        ] {
            let definition = json!({ "layers": [{ "generator": { "sample": { "path": path } } }] });

            // Act
            let diagnostic = validate_asset_paths(
                &definition,
                "instruments/lead",
                &paths,
                "parts[0].instrument (part lead)",
            )
            .unwrap_err();

            // Assert
            assert!(
                diagnostic
                    .location
                    .contains("parts[0].instrument (part lead)")
            );
            assert!(diagnostic.location.contains("path"));
        }
    }

    #[test]
    fn rejects_unknown_events_with_part_and_field_diagnostics() {
        let root = copy_fixture();
        let pattern_path = root.join("patterns/lead.json");
        let mut pattern: Value = serde_json::from_slice(&fs::read(&pattern_path).unwrap()).unwrap();
        pattern["events"][2]["type"] = json!("unknown_control");
        fs::write(&pattern_path, serde_json::to_vec(&pattern).unwrap()).unwrap();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(root.join("bundle.json")).unwrap()).unwrap();
        for file in manifest["files"].as_array_mut().unwrap() {
            if file["path"] == "patterns/lead.json" {
                file["sha256"] = json!(hash_file(&pattern_path, "pattern").unwrap());
            }
        }
        fs::write(
            root.join("bundle.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let diagnostic = read(&root).unwrap_err();

        assert!(matches!(diagnostic.reason, BundleErrorKind::Schema(_)));
        assert!(diagnostic.location.contains("part lead"));
        assert!(diagnostic.location.contains("events[2]"));
        fs::remove_dir_all(root).unwrap();
    }
}
