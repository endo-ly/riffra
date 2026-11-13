use std::path::{Path, PathBuf};

use crate::api::output::{PluginEntry, PluginRole, PluginStateSnapshot};
use crate::score::production::{self, EffectSpec, SourceSpec, Vst3Spec};
use base64::Engine as _;
use riffra_core::Vst3Plugin;

pub(super) struct Library<'a> {
    score_dir: &'a Path,
    root: Option<PathBuf>,
}

pub(super) struct Instrument {
    pub(super) source: SourceSpec,
    pub(super) range: Option<(u8, u8)>,
    pub(super) notes: Option<String>,
}

pub(super) struct ResolvedPlugin {
    pub(super) entry: PluginEntry,
    pub(super) plugin: Vst3Plugin,
    pub(super) range: Option<(u8, u8)>,
    pub(super) notes: Option<String>,
}

pub(super) fn resolve_plugin(
    data_root: &Path,
    vst3: &Vst3Spec,
    role: PluginRole,
    scanner: &Path,
    cache: &mut super::parameters::ProbeCache,
) -> Result<ResolvedPlugin, String> {
    let name = vst3
        .plugin
        .as_deref()
        .ok_or_else(|| "VST3 plugin name is required".to_owned())?;
    let entry = crate::plugins::catalog::selected_named_plugin(
        data_root,
        name,
        vst3.identifier.as_deref(),
        role,
    )?;
    let mut plugin = Vst3Plugin::new(entry.path.clone());
    if let Some(path) = &vst3.state {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("could not read plugin state {path}: {error}"))?;
        let mut snapshot: PluginStateSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid plugin state {path}: {error}"))?;
        snapshot.plugin_path.clone_from(&entry.path);
        crate::plugins::validate_plugin_state(&snapshot, &entry.path)
            .map_err(|error| error.to_string())?;
        if let Some(data) = &snapshot.state_data {
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|error| format!("invalid plugin state base64: {error}"))?;
        }
        plugin.parameter_values = snapshot.parameter_values;
        plugin.state_data = snapshot.state_data;
    }
    if !vst3.patch.is_empty() {
        plugin.state_data = Some(
            crate::plugins::state_document::patch(plugin.state_data.as_deref(), &vst3.patch)
                .map_err(|error| error.to_string())?,
        );
    }
    cache.apply(scanner, &mut plugin, &vst3.params)?;
    Ok(ResolvedPlugin {
        entry,
        plugin,
        range: None,
        notes: None,
    })
}

impl<'a> Library<'a> {
    pub(super) fn new(score_dir: &'a Path) -> Self {
        let root = score_dir
            .ancestors()
            .map(|path| path.join("library"))
            .find(|path| path.is_dir());
        Self { score_dir, root }
    }

    fn template(&self, category: &str, name: &str) -> Result<(PathBuf, String), String> {
        let root = self
            .root
            .as_ref()
            .ok_or_else(|| "no library directory exists above the score".to_owned())?;
        let path = root.join(category).join(format!("{name}.yaml"));
        let text = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "could not read library template {}: {error}",
                path.display()
            )
        })?;
        Ok((path, text))
    }

    pub(super) fn instrument(&self, source: &SourceSpec) -> Result<Instrument, String> {
        let mut overlay = source.clone();
        if let Some(vst3) = &mut overlay.vst3 {
            rebase_state(vst3, self.score_dir)?;
        }
        let Some(name) = &source.library else {
            return Ok(Instrument {
                source: overlay,
                range: None,
                notes: None,
            });
        };
        let (path, text) = self.template("instruments", name)?;
        let mut template = production::parse_instrument_template(&text)
            .map_err(|error| library_error(&path, &error))?;
        if let Some(vst3) = &mut template.source.vst3 {
            rebase_state(vst3, path.parent().expect("template parent"))?;
        }
        let vst3 = merge_vst3(template.source.vst3, overlay.vst3)?;
        Ok(Instrument {
            source: SourceSpec {
                library: Some(name.clone()),
                vst3,
                velocity: overlay.velocity.or(template.source.velocity),
            },
            range: template.range,
            notes: template.notes,
        })
    }

    pub(super) fn effect(&self, effect: &EffectSpec) -> Result<EffectSpec, String> {
        let mut overlay = effect.clone();
        if let Some(vst3) = &mut overlay.vst3 {
            rebase_state(vst3, self.score_dir)?;
        }
        let Some(name) = &effect.library else {
            return Ok(overlay);
        };
        let (path, text) = self.template("effects", name)?;
        let mut template = production::parse_effect_template(&text)
            .map_err(|error| library_error(&path, &error))?;
        rebase_state(&mut template.vst3, path.parent().expect("template parent"))?;
        Ok(EffectSpec {
            id: effect.id.clone(),
            library: Some(name.clone()),
            vst3: merge_vst3(Some(template.vst3), overlay.vst3)?,
            bypassed: overlay.bypassed.or(template.bypassed),
        })
    }
}

fn library_error(path: &Path, error: &crate::score::Error) -> String {
    let line = error.detail["line"].as_u64().unwrap_or(0);
    let column = error.detail["column"].as_u64().unwrap_or(0);
    format!("{}:{line}:{column}: {error}", path.display())
}

fn rebase_state(vst3: &mut Vst3Spec, base: &Path) -> Result<(), String> {
    if let Some(path) = &mut vst3.state {
        let absolute = base.join(&*path);
        *path = absolute
            .to_str()
            .ok_or_else(|| "state path is not valid UTF-8".to_owned())?
            .to_owned();
    }
    Ok(())
}

fn merge_vst3(
    base: Option<Vst3Spec>,
    overlay: Option<Vst3Spec>,
) -> Result<Option<Vst3Spec>, String> {
    let (mut base, overlay) = match (base, overlay) {
        (Some(base), Some(overlay)) => (base, overlay),
        (base, overlay) => return Ok(base.or(overlay)),
    };
    let changed_plugin = overlay
        .plugin
        .as_ref()
        .is_some_and(|plugin| Some(plugin) != base.plugin.as_ref())
        || overlay
            .identifier
            .as_ref()
            .is_some_and(|identifier| Some(identifier) != base.identifier.as_ref());
    if changed_plugin {
        if overlay.plugin.is_none() {
            return Err(
                "changing plugin identity requires a complete inline VST3 specification".into(),
            );
        }
        if overlay.state.is_none() && !overlay.patch.is_empty() {
            return Err("patch requires state".into());
        }
        return Ok(Some(overlay));
    }
    if overlay.plugin.is_some() {
        base.plugin = overlay.plugin;
    }
    if overlay.identifier.is_some() {
        base.identifier = overlay.identifier;
    }
    if overlay.state.is_some() {
        base.state = overlay.state;
        base.patch = overlay.patch;
    } else {
        base.patch.extend(overlay.patch);
    }
    base.params.extend(overlay.params);
    Ok(Some(base))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn templates_merge_with_paths_relative_to_the_declaring_file() {
        let root = std::env::temp_dir().join(format!("riffra-library-{}", uuid::Uuid::now_v7()));
        let templates = root.join("library/instruments");
        let score_dir = root.join("music/song");
        std::fs::create_dir_all(&templates).unwrap();
        std::fs::create_dir_all(&score_dir).unwrap();
        std::fs::write(templates.join("bass.yaml"), "source:\n  vst3:\n    plugin: Synth\n    state: ../states/base.json\n    patch:\n      - {op: set, path: Plugin/Mode, value: base}\n    params: {Mix: 0.25}\n  velocity: [[1, 1], [127, 100]]\nrange: [28, 72]\nnotes: bass\n").unwrap();
        let library = Library::new(&score_dir);
        let source = SourceSpec {
            library: Some("bass".into()),
            vst3: Some(Vst3Spec {
                plugin: None,
                identifier: None,
                state: Some("states/song.json".into()),
                patch: Vec::new(),
                params: BTreeMap::new(),
            }),
            velocity: Some(vec![(1, 2), (127, 127)]),
        };
        let result = library.instrument(&source).unwrap();
        let vst3 = result.source.vst3.unwrap();
        assert_eq!(vst3.plugin.as_deref(), Some("Synth"));
        assert_eq!(
            Path::new(vst3.state.as_deref().unwrap()),
            score_dir.join("states/song.json")
        );
        assert!(vst3.patch.is_empty());
        assert_eq!(vst3.params.len(), 1);
        assert_eq!(result.source.velocity.unwrap()[0], (1, 2));
        assert_eq!(result.range, Some((28, 72)));
        assert_eq!(result.notes.as_deref(), Some("bass"));

        let mut source = source;
        source.vst3.as_mut().unwrap().state = None;
        let inherited = library.instrument(&source).unwrap().source.vst3.unwrap();
        assert_eq!(
            Path::new(inherited.state.as_deref().unwrap()),
            templates.join("../states/base.json")
        );
        assert_eq!(inherited.patch.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_state_uses_the_current_catalog_path_and_rejects_invalid_state() {
        use crate::api::output::{PluginFormat, PluginScanState, ScanReport};
        let root =
            std::env::temp_dir().join(format!("riffra-plugin-state-{}", uuid::Uuid::now_v7()));
        let plugin_path = root.join("Synth.vst3");
        std::fs::create_dir_all(&plugin_path).unwrap();
        let state_path = root.join("state.json");
        let entry = PluginEntry {
            id: "synth".into(),
            name: "Synth".into(),
            vendor: None,
            version: None,
            identifier: Some("synth-class".into()),
            format: PluginFormat::Vst3,
            role: Some(PluginRole::Instrument),
            path: plugin_path.to_string_lossy().into_owned(),
            bundle: true,
            modified_at_ms: None,
            scan_state: PluginScanState::Validated,
        };
        crate::plugins::catalog::save(
            &root,
            &ScanReport {
                root: root.to_string_lossy().into_owned(),
                started_at_ms: 0,
                finished_at_ms: 0,
                plugins: vec![entry],
                issues: vec![],
            },
        )
        .unwrap();
        let snapshot = PluginStateSnapshot {
            schema_version: 1,
            plugin_path: "old-machine/Synth.vst3".into(),
            parameter_values: vec![0.75],
            state_data: Some("c3RhdGU=".into()),
        };
        std::fs::write(&state_path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let spec = Vst3Spec {
            plugin: Some("Synth".into()),
            identifier: Some("synth-class".into()),
            state: Some(state_path.to_string_lossy().into_owned()),
            patch: Vec::new(),
            params: BTreeMap::new(),
        };
        let mut cache = super::super::parameters::ProbeCache::default();
        let scanner = Path::new("unused-scanner");
        let resolved =
            resolve_plugin(&root, &spec, PluginRole::Instrument, scanner, &mut cache).unwrap();
        assert_eq!(resolved.plugin.path, plugin_path.to_string_lossy());
        assert_eq!(resolved.plugin.parameter_values, [0.75]);
        assert_eq!(resolved.plugin.state_data.as_deref(), Some("c3RhdGU="));
        assert!(resolve_plugin(&root, &spec, PluginRole::Effect, scanner, &mut cache).is_err());

        let mut patched = spec.clone();
        patched.patch = vec![crate::api::params::PluginStatePatchOperation::Set {
            path: "Plugin/Mode".into(),
            value: crate::api::output::PluginStateValue::Text("Warm".into()),
        }];
        assert!(
            resolve_plugin(&root, &patched, PluginRole::Instrument, scanner, &mut cache).is_err()
        );

        let mut invalid = snapshot;
        invalid.state_data = Some("invalid-base64".into());
        std::fs::write(&state_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(
            resolve_plugin(&root, &spec, PluginRole::Instrument, scanner, &mut cache)
                .err()
                .unwrap()
                .contains("base64")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
