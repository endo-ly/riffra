//! Asset Application Operations that bridge the canonical Asset store and the
//! Audio Runtime.
//!
//! [`preview_asset`] is the single Production Intent for auditioning a canonical
//! Asset: it validates the AssetId, loads the canonical Asset, confirms it is a
//! previewable audio kind, resolves its content location, checks the file
//! exists, and asks the Audio Runtime to start the preview. React never resolves
//! an AssetId to a path itself, so the Storage layout stays internal to Rust.

use std::path::{Path, PathBuf};

use crate::asset::{AssetId, AssetKind, load, resolve_content_location};
use crate::model::AudioStatus;
use crate::native_audio::AudioSupervisor;

/// Concrete dependencies an Asset Application Operation needs.
pub struct AssetPreviewContext<'a> {
    pub audio: &'a AudioSupervisor,
    pub data_root: &'a Path,
    pub safe_mode: bool,
}

/// Preview tuning for [`preview_asset`]. Mirrors the runtime's existing preview
/// parameters; every field is optional so a caller can omit the slice/gain
/// tuning it does not care about.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPreviewOptions {
    #[serde(default)]
    pub start_ms: u64,
    #[serde(default)]
    pub end_ms: Option<u64>,
    #[serde(default)]
    pub looped: bool,
    #[serde(default = "default_gain")]
    pub gain: f32,
    #[serde(default)]
    pub voice_key: Option<i32>,
}

fn default_gain() -> f32 {
    1.0
}

/// Returns true when an [`AssetKind`] carries audio content the runtime can
/// audition. MIDI payloads, rack definitions, and generation definitions are not
/// previewable here.
fn is_previewable(kind: AssetKind) -> bool {
    matches!(kind, AssetKind::Audio | AssetKind::Sample)
}

/// Starts an Audio Runtime preview for a canonical Asset. The AssetId is the
/// only identifier React supplies; content-location resolution, file-existence
/// checks, and the runtime call all stay inside Rust.
pub fn preview_asset(
    context: &AssetPreviewContext<'_>,
    asset_id: AssetId,
    options: AssetPreviewOptions,
) -> Result<AudioStatus, String> {
    if context.safe_mode {
        return Err(
            "Safe Mode blocks live sample preview; offline analysis and export remain available."
                .into(),
        );
    }
    let asset = load(context.data_root, &asset_id)
        .ok_or_else(|| format!("Preview references an unregistered asset: {asset_id}"))?;
    if !is_previewable(asset.kind) {
        return Err(format!(
            "Asset {asset_id} ({}) cannot be previewed as audio.",
            asset.name
        ));
    }
    let location = resolve_content_location(context.data_root, &asset_id)
        .ok_or_else(|| format!("Asset {asset_id} has no resolvable content location."))?;
    let path = PathBuf::from(&location);
    if !path.is_file() {
        return Err(format!("Preview source does not exist: {}", path.display()));
    }
    context.audio.preview_sample(
        &path,
        options.start_ms,
        options.end_ms,
        options.looped,
        options.gain,
        options.voice_key,
    )
}
