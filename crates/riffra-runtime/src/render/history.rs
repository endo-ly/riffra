use crate::api::output::{
    RenderDeleteResult, RenderExportResult, RenderFileStatus, RenderHistoryEntry, RenderKind,
    RenderResult, RenderStemsResult,
};
use crate::asset;
use riffra_core::{AssetId, CreativeSession};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StoredRender {
    Stems(RenderStemsResult),
    Timeline(RenderResult),
}

fn read_render_manifest(path: &Path) -> Result<StoredRender, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("render manifest could not be inspected: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("render manifest is not a regular file".into());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("render manifest could not be read: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("render manifest is invalid: {error}"))
}

/// Copies the completed WAV from a managed Render directory to a new location.
///
/// # Errors
/// Returns an error when the Render is invalid, the destination is unavailable,
/// or a destination file already exists.
pub(crate) fn export_render(
    data_root: &Path,
    render_id: &str,
    output_path: &str,
) -> Result<RenderExportResult, String> {
    let source_directory = managed_render_directory(data_root, render_id)?;
    let manifest_path = source_directory.join("render.json");
    let (sources, source_path, destination) = match read_render_manifest(&manifest_path)? {
        StoredRender::Timeline(render) => {
            let destination = validate_output_file_path(output_path)?;
            let source = managed_render_file(&source_directory, &render.path)?;
            if !source.is_file() {
                return Err(format!("render WAV is missing: {}", source.display()));
            }
            (
                vec![(source.clone(), destination.clone())],
                source,
                destination,
            )
        }
        StoredRender::Stems(render) => {
            let destination_directory = validate_output_directory(output_path)?;
            let mut sources = Vec::with_capacity(render.stems.len());
            for stem in render.stems {
                let source = managed_render_file(&source_directory, &stem.path)?;
                if !source.is_file() {
                    return Err(format!("render stem WAV is missing: {}", source.display()));
                }
                let name = source
                    .file_name()
                    .ok_or_else(|| {
                        format!("render stem path has no file name: {}", source.display())
                    })?
                    .to_os_string();
                sources.push((source, destination_directory.join(name)));
            }
            (sources, source_directory, destination_directory)
        }
    };
    copy_files_no_clobber(&sources)?;
    Ok(RenderExportResult {
        render_id: render_id.to_owned(),
        source_path: source_path.to_string_lossy().into_owned(),
        output_path: destination.to_string_lossy().into_owned(),
        file_count: sources.len(),
    })
}

pub(crate) fn resolve_measurement_sources(
    data_root: &Path,
    render_id: &str,
) -> Result<Vec<crate::analysis::MeasurementSource>, String> {
    let directory = managed_render_directory(data_root, render_id)?;
    let manifest = read_render_manifest(&directory.join("render.json"))?;
    match manifest {
        StoredRender::Timeline(render) => Ok(vec![crate::analysis::MeasurementSource {
            path: managed_render_file(&directory, &render.path)?,
            track_id: render.track_id,
            track_name: None,
        }]),
        StoredRender::Stems(render) => render
            .stems
            .into_iter()
            .map(|stem| {
                Ok(crate::analysis::MeasurementSource {
                    path: managed_render_file(&directory, &stem.path)?,
                    track_id: Some(stem.track_id),
                    track_name: Some(stem.track_name),
                })
            })
            .collect(),
    }
}

pub(crate) fn list_renders(data_root: &Path) -> Result<Vec<RenderHistoryEntry>, String> {
    let Some(root) = render_store_root(data_root)? else {
        return Ok(Vec::new());
    };
    let entries =
        fs::read_dir(&root).map_err(|error| format!("render store could not be read: {error}"))?;
    let mut renders = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("render store entry could not be read: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("render store entry type could not be read: {error}"))?;
        let render_id = entry.file_name().to_string_lossy().into_owned();
        if !render_id.starts_with("render-") {
            continue;
        }
        if file_type.is_symlink() {
            renders.push(damaged_render_history_entry(
                &render_id,
                "render directory cannot be a symbolic link",
            ));
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        renders.push(read_render_history_entry(
            data_root,
            &entry.path(),
            &render_id,
        ));
    }
    renders.sort_by(|left, right| {
        right
            .created_at_ms
            .cmp(&left.created_at_ms)
            .then_with(|| right.render_id.cmp(&left.render_id))
    });
    Ok(renders)
}

fn damaged_render_history_entry(render_id: &str, diagnostic: &str) -> RenderHistoryEntry {
    RenderHistoryEntry {
        render_id: render_id.to_owned(),
        kind: RenderKind::Unknown,
        project_id: None,
        created_at_ms: render_timestamp(render_id),
        files: Vec::new(),
        file_count: 0,
        file_size_bytes: None,
        sample_rate: None,
        duration_ms: None,
        measurements: Vec::new(),
        diagnostics: vec![diagnostic.to_owned()],
    }
}

fn read_render_history_entry(
    data_root: &Path,
    directory: &Path,
    render_id: &str,
) -> RenderHistoryEntry {
    let manifest_path = directory.join("render.json");
    let parse = read_render_manifest(&manifest_path);
    let mut entry = RenderHistoryEntry {
        render_id: render_id.to_owned(),
        kind: RenderKind::Unknown,
        project_id: None,
        created_at_ms: render_timestamp(render_id),
        files: Vec::new(),
        file_count: 0,
        file_size_bytes: None,
        sample_rate: None,
        duration_ms: None,
        measurements: Vec::new(),
        diagnostics: Vec::new(),
    };
    let manifest = match parse {
        Ok(manifest) => manifest,
        Err(error) => {
            entry.diagnostics.push(error);
            return entry;
        }
    };
    let (kind, project_id, created_at_ms, sample_rate, duration_ms, measurements, file_assets) =
        match manifest {
            StoredRender::Timeline(render) => (
                RenderKind::Timeline,
                render.project_id,
                render.created_at_ms,
                Some(render.sample_rate),
                Some(render.duration_ms),
                vec![render.measurements],
                vec![(render.path, render.asset_id)],
            ),
            StoredRender::Stems(render) => {
                let duration_ms = (render.sample_rate != 0).then(|| {
                    (u128::from(render.frames) * 1_000 / u128::from(render.sample_rate))
                        .min(u128::from(u64::MAX)) as u64
                });
                (
                    RenderKind::Stems,
                    Some(render.project_id),
                    render.created_at_ms,
                    Some(render.sample_rate),
                    duration_ms,
                    render
                        .stems
                        .iter()
                        .map(|stem| stem.measurements.clone())
                        .collect(),
                    render
                        .stems
                        .into_iter()
                        .map(|stem| (stem.path, stem.asset_id))
                        .collect(),
                )
            }
        };
    entry.kind = kind;
    entry.project_id = project_id;
    entry.created_at_ms = created_at_ms.or(entry.created_at_ms);
    entry.sample_rate = sample_rate;
    entry.duration_ms = duration_ms;
    entry.measurements = measurements;
    for (stored_path, asset_id) in file_assets {
        entry.files.push(render_file_status(
            data_root,
            directory,
            &stored_path,
            asset_id,
            &mut entry.diagnostics,
        ));
    }
    entry.file_count = entry.files.len();
    entry.file_size_bytes = entry.files.iter().try_fold(0_u64, |total, file| {
        total.checked_add(file.file_size_bytes?)
    });
    entry
}

fn render_file_status(
    data_root: &Path,
    directory: &Path,
    stored_path: &str,
    asset_id: AssetId,
    diagnostics: &mut Vec<String>,
) -> RenderFileStatus {
    let resolved = managed_render_file(directory, stored_path);
    if let Err(error) = &resolved {
        diagnostics.push(error.clone());
    }
    let path = resolved
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| stored_path.to_owned());
    let file_exists = resolved.as_ref().is_ok_and(|path| path.is_file());
    let file_size_bytes = resolved
        .as_ref()
        .ok()
        .and_then(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len());
    let asset_registered = asset::load(data_root, &asset_id).is_some_and(|asset| {
        asset.kind == riffra_core::AssetKind::Audio && asset.content_location == stored_path
    });
    if !asset_registered {
        diagnostics.push(format!(
            "Asset registration does not match render file {stored_path} ({asset_id})"
        ));
    }
    RenderFileStatus {
        path,
        asset_id: Some(asset_id),
        file_exists,
        asset_registered,
        file_size_bytes,
    }
}

fn render_timestamp(render_id: &str) -> Option<u64> {
    render_id
        .strip_prefix("render-")?
        .split_once('-')?
        .0
        .parse()
        .ok()
}

pub(crate) fn delete_render<'a>(
    data_root: &Path,
    project_store: &riffra_host::ProjectStore,
    active_project_id: &str,
    active_session: &CreativeSession,
    history_sessions: impl Iterator<Item = &'a CreativeSession>,
    jobs: &crate::jobs::JobRegistry,
    render_id: &str,
) -> Result<RenderDeleteResult, String> {
    let _render_access = jobs.lock_render_access()?;
    let active_jobs = jobs.render_deletion_blockers(render_id)?;
    if !active_jobs.is_empty() {
        return Err(format!(
            "render deletion is blocked by active jobs: {}",
            active_jobs.join(", ")
        ));
    }
    let _ = render_store_root(data_root)?;
    let directory = render_directory_path(data_root, render_id)?;
    if let Some(result) = finish_interrupted_render_deletion(data_root, render_id)? {
        return Ok(result);
    }
    if !directory.is_dir() {
        return Err(format!("render is not registered: {render_id}"));
    }
    let manifest_path = directory.join("render.json");
    let manifest = read_render_manifest(&manifest_path)?;
    let file_assets = manifest_file_assets(manifest);
    let files = verify_owned_render_directory(data_root, &directory, &manifest_path, &file_assets)?;
    let asset_ids = file_assets
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    if asset_ids
        .iter()
        .map(AssetId::as_str)
        .collect::<BTreeSet<_>>()
        .len()
        != asset_ids.len()
    {
        return Err("render manifest contains a duplicate Asset ID".into());
    }
    let asset_set = asset_ids
        .iter()
        .map(|id| id.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(reference) = session_asset_reference(active_session, &asset_set) {
        return Err(format!(
            "render deletion is blocked by current project {active_project_id} {reference}"
        ));
    }
    for session in history_sessions {
        if let Some(reference) = session_asset_reference(session, &asset_set) {
            return Err(format!(
                "render deletion is blocked by current project {active_project_id} undo/redo history: {reference}"
            ));
        }
    }
    let projects = project_store
        .list()
        .map_err(|error| format!("project references could not be inspected: {error}"))?;
    for project in projects {
        if let Some(error) = project.error {
            return Err(format!(
                "render deletion is unsafe because project {} cannot be checked: {error}",
                project.project_id
            ));
        }
        let store = project_store
            .session_store(&project.project_id)
            .map_err(|error| {
                format!(
                    "project {} could not be opened: {error}",
                    project.project_id
                )
            })?;
        let sessions = store.saved_sessions().map_err(|error| {
            format!(
                "render deletion is unsafe because saved sessions for project {} cannot be checked: {error}",
                project.project_id
            )
        })?;
        for session in sessions {
            if let Some(reference) = session_asset_reference(&session, &asset_set) {
                return Err(format!(
                    "render deletion is blocked by project {} saved session {} {reference}",
                    project.project_id, session.session_id
                ));
            }
        }
    }
    for (reference, asset_id) in crate::recording::referenced_asset_ids(data_root)? {
        if asset_set.contains(asset_id.as_str()) {
            return Err(format!(
                "render deletion is blocked by {reference} ({asset_id})"
            ));
        }
    }
    if let Some((dependent, source)) = asset::render_asset_dependents(data_root, &asset_ids)?
        .into_iter()
        .next()
    {
        return Err(format!(
            "render deletion is blocked by Asset {dependent}, which depends on {source}"
        ));
    }
    let render_root = directory
        .parent()
        .expect("managed render directory has a parent");
    let nonce = riffra_control::new_instance_id();
    let tombstone = render_root.join(format!(".deleting-{render_id}-{nonce}"));
    let marker_path = render_root.join(format!(".deleting-{render_id}-{nonce}.json"));
    let marker = RenderDeletionMarker {
        render_id: render_id.to_owned(),
        asset_ids,
        deleted_file_count: files.len(),
    };
    write_deletion_marker(&marker_path, &marker)?;
    fs::rename(&directory, &tombstone).map_err(|error| {
        let _ = fs::remove_file(&marker_path);
        format!("render directory could not be moved to its deletion area: {error}")
    })?;
    if let Err(error) = asset::remove_render_assets(data_root, &file_assets) {
        return match fs::rename(&tombstone, &directory) {
            Ok(()) => {
                let _ = fs::remove_file(&marker_path);
                Err(error)
            }
            Err(restore_error) => Err(format!(
                "{error}; render files remain at {} and could not be restored to {}: {restore_error}",
                tombstone.display(),
                directory.display()
            )),
        };
    }
    fs::remove_dir_all(&tombstone).map_err(|error| {
        format!(
            "render Assets were removed, but render files remain at {} for retry: {error}",
            tombstone.display()
        )
    })?;
    fs::remove_file(&marker_path).map_err(|error| {
        format!(
            "render files and Assets were removed, but deletion marker remains at {}: {error}",
            marker_path.display()
        )
    })?;
    Ok(RenderDeleteResult {
        render_id: render_id.to_owned(),
        deleted_file_count: files.len(),
    })
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RenderDeletionMarker {
    pub(super) render_id: String,
    pub(super) asset_ids: Vec<AssetId>,
    pub(super) deleted_file_count: usize,
}

fn render_directory_path(data_root: &Path, render_id: &str) -> Result<PathBuf, String> {
    let relative = Path::new(render_id);
    if relative.components().count() != 1
        || relative.file_name().is_none_or(|name| name != render_id)
        || !render_id.starts_with("render-")
    {
        return Err("render id must be a managed render directory name".into());
    }
    Ok(data_root.join("renders").join(relative))
}

fn render_store_root(data_root: &Path) -> Result<Option<PathBuf>, String> {
    let root = data_root.join("renders");
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("render store cannot be a symbolic link".into())
        }
        Ok(metadata) if metadata.is_dir() => Ok(Some(root)),
        Ok(_) => Err("render store is not a directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("render store could not be inspected: {error}")),
    }
}

fn manifest_file_assets(manifest: StoredRender) -> Vec<(AssetId, String)> {
    match manifest {
        StoredRender::Timeline(render) => vec![(render.asset_id, render.path)],
        StoredRender::Stems(render) => render
            .stems
            .into_iter()
            .map(|stem| (stem.asset_id, stem.path))
            .collect(),
    }
}

fn verify_owned_render_directory(
    data_root: &Path,
    directory: &Path,
    manifest_path: &Path,
    file_assets: &[(AssetId, String)],
) -> Result<Vec<PathBuf>, String> {
    if fs::symlink_metadata(directory)
        .map_err(|error| format!("render directory could not be inspected: {error}"))?
        .file_type()
        .is_symlink()
    {
        return Err("render directory cannot be a symbolic link".into());
    }
    let render_root = directory
        .parent()
        .ok_or_else(|| "managed render directory has no parent".to_string())?
        .canonicalize()
        .map_err(|error| format!("render store could not be resolved: {error}"))?;
    let canonical_directory = directory
        .canonicalize()
        .map_err(|error| format!("render directory could not be resolved: {error}"))?;
    if canonical_directory.parent() != Some(render_root.as_path()) {
        return Err("render directory is outside the managed Render store".into());
    }
    let mut expected = BTreeSet::new();
    expected.insert(
        manifest_path
            .canonicalize()
            .map_err(|error| format!("render manifest could not be resolved: {error}"))?,
    );
    let mut files = Vec::with_capacity(file_assets.len() + 1);
    for (id, stored_path) in file_assets {
        let path = managed_render_file(directory, stored_path)?;
        if !path.is_file() {
            return Err(format!("render WAV is missing: {}", path.display()));
        }
        let asset = asset::load(data_root, id)
            .ok_or_else(|| format!("render Asset is not registered: {id}"))?;
        if asset.kind != riffra_core::AssetKind::Audio || asset.content_location != *stored_path {
            return Err(format!(
                "render Asset {id} does not match {}",
                path.display()
            ));
        }
        expected.insert(path.clone());
        files.push(path);
    }
    let locations = file_assets
        .iter()
        .map(|(_, path)| path.clone())
        .collect::<Vec<_>>();
    let owners = asset::render_asset_location_owners(data_root, &locations)?;
    if owners.len() != file_assets.len()
        || owners.iter().any(|(id, path)| {
            !file_assets
                .iter()
                .any(|(expected_id, expected_path)| expected_id == id && expected_path == path)
        })
    {
        return Err("render files have Asset registrations outside their manifest".into());
    }
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("render directory could not be inspected: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("render file could not be inspected: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("render file type could not be inspected: {error}"))?;
        if file_type.is_symlink() || !file_type.is_file() {
            return Err(format!(
                "render directory contains an unmanaged entry: {}",
                entry.path().display()
            ));
        }
        let path = entry
            .path()
            .canonicalize()
            .map_err(|error| format!("render file could not be resolved: {error}"))?;
        if !expected.remove(&path) {
            return Err(format!(
                "render directory contains an unmanaged file: {}",
                path.display()
            ));
        }
    }
    if !expected.is_empty() {
        return Err("render manifest does not match the files in its directory".into());
    }
    files.push(manifest_path.to_path_buf());
    Ok(files)
}

fn session_asset_reference(
    session: &CreativeSession,
    asset_ids: &BTreeSet<&str>,
) -> Option<String> {
    for clip in &session.arrangement.audio_clips {
        if asset_ids.contains(clip.asset_id.as_str()) {
            return Some(format!("audio Clip {} references a Render Asset", clip.id));
        }
    }
    for clip in &session.arrangement.midi_clips {
        if clip
            .asset_id
            .as_ref()
            .is_some_and(|id| asset_ids.contains(id.as_str()))
        {
            return Some(format!("MIDI Clip {} references a Render Asset", clip.id));
        }
    }
    for take in &session.arrangement.takes {
        for (label, id) in [
            (
                "raw audio",
                take.raw_audio.as_ref().map(|source| &source.asset_id),
            ),
            (
                "processed audio",
                take.processed_audio.as_ref().map(|source| &source.asset_id),
            ),
            ("MIDI", take.midi_asset_id.as_ref()),
        ] {
            if id.is_some_and(|id| asset_ids.contains(id.as_str())) {
                return Some(format!(
                    "recording Take {} {label} references a Render Asset",
                    take.id
                ));
            }
        }
    }
    None
}

pub(super) fn write_deletion_marker(
    path: &Path,
    marker: &RenderDeletionMarker,
) -> Result<(), String> {
    let payload = serde_json::to_vec(marker)
        .map_err(|error| format!("render deletion marker could not be encoded: {error}"))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("render deletion marker could not be created: {error}"))?;
    use std::io::Write;
    if let Err(error) = file.write_all(&payload).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(format!(
            "render deletion marker could not be saved: {error}"
        ));
    }
    Ok(())
}

fn finish_interrupted_render_deletion(
    data_root: &Path,
    render_id: &str,
) -> Result<Option<RenderDeleteResult>, String> {
    let render_root = data_root.join("renders");
    if !render_root.is_dir() {
        return Ok(None);
    }
    let prefix = format!(".deleting-{render_id}-");
    let mut markers = Vec::new();
    for entry in fs::read_dir(&render_root)
        .map_err(|error| format!("render deletion areas could not be read: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("render deletion entry could not be read: {error}"))?;
        let path = entry.path();
        if path.file_name().is_some_and(|name| {
            name.to_string_lossy().starts_with(&prefix)
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json")
        }) {
            markers.push(path);
        }
    }
    markers.sort();
    if markers.is_empty() {
        return Ok(None);
    }
    let original = render_root.join(render_id);
    let mut deleted_file_count = 0;
    for marker_path in markers {
        let marker: RenderDeletionMarker = serde_json::from_slice(
            &fs::read(&marker_path)
                .map_err(|error| format!("render deletion marker could not be read: {error}"))?,
        )
        .map_err(|error| format!("render deletion marker is invalid: {error}"))?;
        if marker.render_id != render_id || marker.asset_ids.is_empty() {
            return Err(format!("render deletion marker does not match {render_id}"));
        }
        let tombstone_name = marker_path
            .file_stem()
            .ok_or_else(|| "render deletion marker has no file name".to_string())?;
        let tombstone = render_root.join(tombstone_name);
        let registered = asset::registered_render_asset_count(data_root, &marker.asset_ids)?;
        if !tombstone.exists() && original.is_dir() && registered == marker.asset_ids.len() {
            fs::remove_file(&marker_path).map_err(|error| {
                format!("stale render deletion marker could not be removed: {error}")
            })?;
            continue;
        }
        if !tombstone.exists() && original.exists() {
            return Err(format!(
                "interrupted render deletion has inconsistent files and Asset registrations for {render_id}"
            ));
        }
        if tombstone.exists() && original.exists() {
            return Err(format!(
                "interrupted render deletion has both original and temporary directories for {render_id}"
            ));
        }
        if tombstone.is_dir() && registered == marker.asset_ids.len() {
            fs::rename(&tombstone, &original).map_err(|error| {
                format!("interrupted render deletion could not be restored: {error}")
            })?;
            fs::remove_file(&marker_path)
                .map_err(|error| format!("render deletion marker could not be removed: {error}"))?;
            return Err(format!(
                "interrupted render deletion was restored to {}; retry render clean",
                original.display()
            ));
        }
        if registered != 0 {
            return Err(format!(
                "interrupted render deletion has inconsistent Asset registrations for {render_id}"
            ));
        }
        if tombstone.exists() {
            if fs::symlink_metadata(&tombstone)
                .map_err(|error| format!("render deletion area could not be inspected: {error}"))?
                .file_type()
                .is_symlink()
                || !tombstone.is_dir()
            {
                return Err(format!(
                    "render deletion area is not a managed directory: {}",
                    tombstone.display()
                ));
            }
            fs::remove_dir_all(&tombstone).map_err(|error| {
                format!(
                    "render Asset registrations were removed, but files remain at {}: {error}",
                    tombstone.display()
                )
            })?;
            deleted_file_count = marker.deleted_file_count;
        } else if !original.exists() {
            deleted_file_count = marker.deleted_file_count;
        }
        fs::remove_file(&marker_path).map_err(|error| {
            format!(
                "render deletion marker could not be removed from {}: {error}",
                marker_path.display()
            )
        })?;
    }
    if deleted_file_count > 0 || !original.exists() {
        Ok(Some(RenderDeleteResult {
            render_id: render_id.to_owned(),
            deleted_file_count,
        }))
    } else {
        Ok(None)
    }
}

fn managed_render_directory(data_root: &Path, render_id: &str) -> Result<PathBuf, String> {
    let renders =
        render_store_root(data_root)?.ok_or_else(|| "render store does not exist".to_string())?;
    let relative = Path::new(render_id);
    if relative.components().count() != 1
        || relative.file_name().is_none_or(|name| name != render_id)
        || !render_id.starts_with("render-")
    {
        return Err("render id must be a managed render directory name".into());
    }
    let directory = renders.join(relative);
    if !directory.is_dir() {
        return Err(format!("render is not registered: {render_id}"));
    }
    if fs::symlink_metadata(&directory)
        .map_err(|error| format!("render directory could not be inspected: {error}"))?
        .file_type()
        .is_symlink()
    {
        return Err("render directory cannot be a symbolic link".into());
    }
    let canonical_root = renders
        .canonicalize()
        .map_err(|error| format!("render store could not be resolved: {error}"))?;
    let canonical_directory = directory
        .canonicalize()
        .map_err(|error| format!("render directory could not be resolved: {error}"))?;
    if canonical_directory.parent() != Some(canonical_root.as_path()) {
        return Err("render directory is outside the managed Render store".into());
    }
    Ok(directory)
}

fn managed_render_file(directory: &Path, stored_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(stored_path);
    if !path.is_absolute() {
        return Err(format!(
            "render manifest contains a non-absolute path: {stored_path}"
        ));
    }
    let root = directory
        .canonicalize()
        .map_err(|error| format!("render directory could not be resolved: {error}"))?;
    let source = path
        .canonicalize()
        .map_err(|error| format!("render file could not be resolved: {error}"))?;
    if !source.starts_with(&root) {
        return Err(format!(
            "render manifest points outside its directory: {stored_path}"
        ));
    }
    Ok(source)
}

pub(super) fn validate_output_file_path(path: &str) -> Result<PathBuf, String> {
    let path = validate_output_path(Path::new(path))?;
    if path
        .extension()
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("wav"))
    {
        return Err(format!(
            "render output path must have a .wav extension: {}",
            path.display()
        ));
    }
    Ok(path)
}

pub(super) fn validate_output_directory(path: &str) -> Result<PathBuf, String> {
    let path = validate_output_path(Path::new(path))?;
    if !path.is_dir() {
        return Err(format!(
            "render output directory does not exist: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn validate_output_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!(
            "render output path must be absolute: {}",
            path.display()
        ));
    }
    let file_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("render output path has no file name: {}", path.display()))?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("render output path has no parent: {}", path.display()))?;
    if !parent.is_dir() {
        return Err(format!(
            "render output directory does not exist: {}",
            parent.display()
        ));
    }
    let resolved_parent = parent
        .canonicalize()
        .map_err(|error| format!("render output directory could not be resolved: {error}"))?;
    Ok(resolved_parent.join(file_name))
}

pub(super) fn copy_files_no_clobber(copies: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    let mut created_files = Vec::with_capacity(copies.len());
    let result = (|| {
        for (source, destination) in copies {
            let parent = destination.parent().ok_or_else(|| {
                format!(
                    "render output path has no parent: {}",
                    destination.display()
                )
            })?;
            if !parent.is_dir() {
                return Err(format!(
                    "render output directory does not exist: {}",
                    parent.display()
                ));
            }
            let mut source_file = fs::File::open(source).map_err(|error| {
                format!(
                    "render source could not be opened at {}: {error}",
                    source.display()
                )
            })?;
            let mut temporary = TemporaryRenderExport::create(parent).map_err(|error| {
                format!(
                    "render output could not be prepared in {}: {error}",
                    parent.display()
                )
            })?;
            io::copy(&mut source_file, temporary.file_mut()).map_err(|error| {
                format!(
                    "render output could not be copied to {}: {error}",
                    destination.display()
                )
            })?;
            temporary.file_mut().sync_all().map_err(|error| {
                format!(
                    "render output could not be flushed at {}: {error}",
                    destination.display()
                )
            })?;
            temporary.publish(destination).map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    format!("render output already exists: {}", destination.display())
                } else {
                    format!(
                        "render output could not be finalized at {}: {error}",
                        destination.display()
                    )
                }
            })?;
            created_files.push(destination.clone());
        }
        Ok(())
    })();
    if result.is_err() {
        for path in &created_files {
            if let Err(error) = fs::remove_file(path)
                && path.exists()
            {
                tracing::error!(%error, path = %path.display(), "render export cleanup failed");
            }
        }
    }
    result
}

struct TemporaryRenderExport {
    path: PathBuf,
    file: Option<fs::File>,
    published: bool,
}

impl TemporaryRenderExport {
    fn create(parent: &Path) -> io::Result<Self> {
        loop {
            let path = parent.join(format!(
                ".riffra-render-export-{}.tmp",
                uuid::Uuid::now_v7()
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                        published: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    fn file_mut(&mut self) -> &mut fs::File {
        self.file.as_mut().expect("temporary export file is open")
    }

    fn publish(mut self, destination: &Path) -> io::Result<()> {
        drop(self.file.take());
        rename_without_replace(&self.path, destination)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for TemporaryRenderExport {
    fn drop(&mut self) {
        if !self.published
            && let Err(error) = fs::remove_file(&self.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::error!(%error, path = %self.path.display(), "render export temporary file cleanup failed");
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rename_without_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(windows)]
fn rename_without_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    let source = source.as_os_str().encode_wide().collect::<Vec<_>>();
    let destination = destination.as_os_str().encode_wide().collect::<Vec<_>>();
    if source.contains(&0) || destination.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "render output path contains a null character",
        ));
    }
    let source = source
        .into_iter()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .into_iter()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileW(source.as_ptr(), destination.as_ptr())
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn rename_without_replace(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace file finalization is unavailable on this platform",
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::tests::{create_managed_render, session_with_clips, wav};

    #[test]
    fn render_history_lists_integrity_and_clean_removes_only_managed_files() {
        let root = std::env::temp_dir().join(format!(
            "riffra-render-clean-{}",
            riffra_control::new_instance_id()
        ));
        let output = root.join("exports").join("mix.wav");
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        let render = create_managed_render(&root, &session_with_clips(), 31);
        let render_id = Path::new(&render.path)
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let manifest_path = Path::new(&render.path)
            .parent()
            .unwrap()
            .join("render.json");
        let original = fs::read(&manifest_path).unwrap();
        let dispatcher = crate::Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        drop(dispatcher);
        assert_eq!(fs::read(&manifest_path).unwrap(), original);
        let sources = resolve_measurement_sources(&root, &render_id).unwrap();
        assert_eq!(fs::read(&manifest_path).unwrap(), original);
        assert_eq!(
            sources[0].path,
            Path::new(&render.path).canonicalize().unwrap()
        );
        export_render(&root, &render_id, output.to_str().unwrap()).unwrap();

        let listed = list_renders(&root).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].render_id, render_id);
        assert_eq!(listed[0].kind, RenderKind::Timeline);
        assert_eq!(listed[0].created_at_ms, Some(31));
        assert_eq!(listed[0].file_count, 1);
        assert!(listed[0].file_size_bytes.unwrap() > 0);
        assert!(listed[0].files[0].file_exists);
        assert!(listed[0].files[0].asset_registered);
        assert!(listed[0].diagnostics.is_empty());

        let jobs = crate::jobs::JobRegistry::default();
        let (analysis_id, _) = jobs.start_analysis_measure(Some(render_id.clone()));
        let _ = jobs.start_analysis_measure(None);
        let _ = jobs.start_analysis_measure(Some("render-unrelated".into()));
        let store = riffra_host::ProjectStore::new(&root);
        let project = store.initialize().unwrap();
        for phase in 0..3 {
            if phase == 1 {
                jobs.set_running(&analysis_id, "measuring");
            }
            if phase == 2 {
                jobs.cancel(&analysis_id).unwrap();
            }
            let error = delete_render(
                &root,
                &store,
                &project.project_id,
                &CreativeSession::new(1),
                std::iter::empty(),
                &jobs,
                &render_id,
            )
            .unwrap_err();
            assert!(error.contains(&analysis_id));
            assert!(Path::new(&render.path).is_file());
        }
        jobs.mark_cancelled(&analysis_id);
        let malformed_directory = root.join("renders").join("render-32-damaged");
        fs::create_dir(&malformed_directory).unwrap();
        fs::write(
            malformed_directory.join("render.json"),
            b"not a render manifest",
        )
        .unwrap();
        let listed = list_renders(&root).unwrap();
        let damaged = listed
            .iter()
            .find(|item| item.render_id == "render-32-damaged")
            .unwrap();
        assert_eq!(damaged.kind, RenderKind::Unknown);
        assert_eq!(damaged.created_at_ms, Some(32));
        assert_eq!(damaged.diagnostics.len(), 1);
        assert_eq!(
            fs::read(malformed_directory.join("render.json")).unwrap(),
            b"not a render manifest"
        );

        fs::remove_dir_all(malformed_directory).unwrap();

        let project = riffra_host::ProjectStore::new(&root).initialize().unwrap();
        let deleted = delete_render(
            &root,
            &riffra_host::ProjectStore::new(&root),
            &project.project_id,
            &project.loaded.session,
            std::iter::empty(),
            &jobs,
            &render_id,
        )
        .unwrap();
        assert_eq!(deleted.deleted_file_count, 2);
        assert!(!Path::new(&render.path).exists());
        assert!(output.is_file());
        assert!(asset::load(&root, &render.asset_id).is_none());
        assert!(list_renders(&root).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn render_clean_rejects_current_and_saved_generation_references() {
        let root = std::env::temp_dir().join(format!(
            "riffra-render-referenced-{}",
            riffra_control::new_instance_id()
        ));
        let render = create_managed_render(&root, &session_with_clips(), 41);
        let render_id = Path::new(&render.path)
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let store = riffra_host::ProjectStore::new(&root);
        let project = store.initialize().unwrap();
        let jobs = crate::jobs::JobRegistry::default();
        let mut referenced = session_with_clips();
        referenced.arrangement.midi_clips[0].asset_id = Some(render.asset_id.clone());
        let active_jobs = crate::jobs::JobRegistry::default();
        let _ = active_jobs.start(crate::api::output::JobKind::Render);
        let active_job_error = delete_render(
            &root,
            &store,
            &project.project_id,
            &CreativeSession::new(41),
            std::iter::empty(),
            &active_jobs,
            &render_id,
        )
        .unwrap_err();
        assert!(active_job_error.contains("active jobs"));
        let current_error = delete_render(
            &root,
            &store,
            &project.project_id,
            &referenced,
            std::iter::empty(),
            &jobs,
            &render_id,
        )
        .unwrap_err();
        assert!(current_error.contains("current project"));
        assert!(current_error.contains("Clip clip"));

        let session_store = store.session_store(&project.project_id).unwrap();
        session_store.save(&referenced).unwrap();
        session_store.save(&CreativeSession::new(42)).unwrap();
        let generation_error = delete_render(
            &root,
            &store,
            &project.project_id,
            &CreativeSession::new(42),
            std::iter::empty(),
            &jobs,
            &render_id,
        )
        .unwrap_err();
        assert!(generation_error.contains("saved session"));
        assert!(generation_error.contains("Clip clip"));
        assert!(Path::new(&render.path).is_file());
        assert!(asset::load(&root, &render.asset_id).is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn render_clean_rejects_recording_and_provenance_references() {
        let root = std::env::temp_dir().join(format!(
            "riffra-render-dependent-{}",
            riffra_control::new_instance_id()
        ));
        let render = create_managed_render(&root, &session_with_clips(), 51);
        let render_id = Path::new(&render.path)
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let store = riffra_host::ProjectStore::new(&root);
        let project = store.initialize().unwrap();
        let jobs = crate::jobs::JobRegistry::default();
        let child_path = root.join("derived.wav");
        fs::write(&child_path, wav(48_000, 128)).unwrap();
        let child_id = asset::register_derived(
            &root,
            std::slice::from_ref(&render.asset_id),
            riffra_core::AssetKind::Audio,
            "Derived Render",
            &child_path.to_string_lossy(),
            riffra_core::ProvenanceOperation::Processed,
            serde_json::Map::new(),
        )
        .unwrap();
        let dependency_error = delete_render(
            &root,
            &store,
            &project.project_id,
            &CreativeSession::new(52),
            std::iter::empty(),
            &jobs,
            &render_id,
        )
        .unwrap_err();
        assert!(dependency_error.contains(&child_id.to_string()));

        let capture_directory = root.join("recordings").join("inbox").join("take-1");
        fs::create_dir_all(&capture_directory).unwrap();
        let mut capture = crate::api::output::RecordingCapture::start("capture:1", "session-1", 1);
        capture.raw_audio_asset_id = Some(render.asset_id.clone());
        fs::write(
            capture_directory.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({ "capture": capture })).unwrap(),
        )
        .unwrap();
        let recording_error = delete_render(
            &root,
            &store,
            &project.project_id,
            &CreativeSession::new(52),
            std::iter::empty(),
            &jobs,
            &render_id,
        )
        .unwrap_err();
        assert!(recording_error.contains("recording"));
        assert!(recording_error.contains("raw audio"));
        assert!(Path::new(&render.path).is_file());
        assert!(asset::load(&root, &render.asset_id).is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn render_clean_recovers_interrupted_file_and_asset_removal() {
        let root = std::env::temp_dir().join(format!(
            "riffra-render-clean-retry-{}",
            riffra_control::new_instance_id()
        ));
        let store = riffra_host::ProjectStore::new(&root);
        let project = store.initialize().unwrap();
        let session = CreativeSession::new(61);
        let jobs = crate::jobs::JobRegistry::default();
        let render = create_managed_render(&root, &session_with_clips(), 61);
        let directory = Path::new(&render.path).parent().unwrap().to_path_buf();
        let render_id = directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let render_root = directory.parent().unwrap();
        let marker_name = format!(".deleting-{render_id}-interrupted");
        let tombstone = render_root.join(&marker_name);
        let marker_path = render_root.join(format!("{marker_name}.json"));
        let marker = RenderDeletionMarker {
            render_id: render_id.clone(),
            asset_ids: vec![render.asset_id.clone()],
            deleted_file_count: 2,
        };

        fs::rename(&directory, &tombstone).unwrap();
        write_deletion_marker(&marker_path, &marker).unwrap();
        let restored = delete_render(
            &root,
            &store,
            &project.project_id,
            &session,
            std::iter::empty(),
            &jobs,
            &render_id,
        )
        .unwrap_err();
        assert!(restored.contains("was restored"));
        assert!(directory.is_dir());
        assert!(asset::load(&root, &render.asset_id).is_some());
        delete_render(
            &root,
            &store,
            &project.project_id,
            &session,
            std::iter::empty(),
            &jobs,
            &render_id,
        )
        .unwrap();

        let second_render = create_managed_render(&root, &session_with_clips(), 62);
        let second_directory = Path::new(&second_render.path)
            .parent()
            .unwrap()
            .to_path_buf();
        let second_id = second_directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let second_tombstone_name = format!(".deleting-{second_id}-committed");
        let second_tombstone = second_directory
            .parent()
            .unwrap()
            .join(&second_tombstone_name);
        let second_marker_path = second_directory
            .parent()
            .unwrap()
            .join(format!("{second_tombstone_name}.json"));
        fs::rename(&second_directory, &second_tombstone).unwrap();
        write_deletion_marker(
            &second_marker_path,
            &RenderDeletionMarker {
                render_id: second_id.clone(),
                asset_ids: vec![second_render.asset_id.clone()],
                deleted_file_count: 2,
            },
        )
        .unwrap();
        asset::remove_render_assets(
            &root,
            &[(second_render.asset_id.clone(), second_render.path.clone())],
        )
        .unwrap();
        let completed = delete_render(
            &root,
            &store,
            &project.project_id,
            &session,
            std::iter::empty(),
            &jobs,
            &second_id,
        )
        .unwrap();
        assert_eq!(completed.deleted_file_count, 2);
        assert!(!second_tombstone.exists());
        assert!(!second_marker_path.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
