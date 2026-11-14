use std::path::Path;

use crate::api::output::{PluginEntry, PluginRole, PluginStateSnapshot};
use crate::score::production::Vst3Spec;
use base64::Engine as _;
use riffra_core::Vst3Plugin;

pub(super) struct ResolvedPlugin {
    pub(super) entry: PluginEntry,
    pub(super) plugin: Vst3Plugin,
}

#[derive(Debug)]
pub(super) struct ResolveError {
    pub(super) field: &'static str,
    pub(super) message: String,
}

impl ResolveError {
    fn new(field: &'static str, message: impl ToString) -> Self {
        Self {
            field,
            message: message.to_string(),
        }
    }
}

pub(super) fn resolve_plugin(
    data_root: &Path,
    base: &Path,
    vst3: &Vst3Spec,
    role: PluginRole,
    dependencies: &mut super::Dependencies,
) -> Result<ResolvedPlugin, ResolveError> {
    let name = &vst3.plugin;
    let entry = crate::plugins::catalog::selected_named_plugin(
        data_root,
        name,
        vst3.identifier.as_deref(),
        role,
    )
    .map_err(|error| ResolveError::new("plugin", error))?;
    let mut plugin = Vst3Plugin::new(entry.path.clone());
    if let Some(path) = &vst3.state {
        let path = super::local_input_path(base, path)
            .map_err(|error| ResolveError::new("state", error))?;
        super::record_dependency(dependencies, &path)
            .map_err(|error| ResolveError::new("state", error))?;
        let bytes = std::fs::read(&path).map_err(|error| {
            ResolveError::new(
                "state",
                format!("could not read plugin state {}: {error}", path.display()),
            )
        })?;
        let snapshot: PluginStateSnapshot = serde_json::from_slice(&bytes).map_err(|error| {
            ResolveError::new(
                "state",
                format!("invalid plugin state {}: {error}", path.display()),
            )
        })?;
        crate::plugins::validate_plugin_state(&snapshot, &entry)
            .map_err(|error| ResolveError::new("state", error))?;
        if let Some(data) = &snapshot.state_data {
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|error| {
                    ResolveError::new("state", format!("invalid plugin state base64: {error}"))
                })?;
        }
        plugin.parameter_values = snapshot.parameter_values;
        plugin.state_data = snapshot.state_data;
    }
    if !vst3.patch.is_empty() {
        plugin.state_data = Some(
            crate::plugins::state_document::patch(plugin.state_data.as_deref(), &vst3.patch)
                .map_err(|error| ResolveError::new("patch", error))?,
        );
    }
    Ok(ResolvedPlugin { entry, plugin })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
            schema_version: 2,
            plugin_identifier: "synth-class".into(),
            plugin_path: "old-machine/Synth.vst3".into(),
            parameter_values: vec![0.75],
            state_data: Some("c3RhdGU=".into()),
        };
        std::fs::write(&state_path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let spec = Vst3Spec {
            plugin: "Synth".into(),
            identifier: Some("synth-class".into()),
            state: Some("state.json".into()),
            patch: Vec::new(),
            params: BTreeMap::new(),
        };
        let mut dependencies = super::super::Dependencies::new();
        let resolved = resolve_plugin(
            &root,
            &root,
            &spec,
            PluginRole::Instrument,
            &mut dependencies,
        )
        .unwrap();
        assert_eq!(resolved.plugin.path, plugin_path.to_string_lossy());
        assert_eq!(resolved.plugin.parameter_values, [0.75]);
        assert_eq!(resolved.plugin.state_data.as_deref(), Some("c3RhdGU="));
        assert!(
            resolve_plugin(&root, &root, &spec, PluginRole::Effect, &mut dependencies).is_err()
        );

        let mut outside = spec.clone();
        outside.state = Some("../state.json".into());
        assert!(
            resolve_plugin(
                &root,
                &root,
                &outside,
                PluginRole::Instrument,
                &mut dependencies
            )
            .err()
            .unwrap()
            .message
            .contains("relative")
        );
        outside.state = Some(state_path.to_string_lossy().into_owned());
        assert!(
            resolve_plugin(
                &root,
                &root,
                &outside,
                PluginRole::Instrument,
                &mut dependencies
            )
            .err()
            .unwrap()
            .message
            .contains("relative")
        );

        let mut patched = spec.clone();
        patched.patch = vec![crate::api::params::PluginStatePatchOperation::Set {
            path: "Plugin/Mode".into(),
            value: crate::api::output::PluginStateValue::Text("Warm".into()),
        }];
        assert!(
            resolve_plugin(
                &root,
                &root,
                &patched,
                PluginRole::Instrument,
                &mut dependencies
            )
            .is_err()
        );

        let mut different = snapshot.clone();
        different.plugin_identifier = "other-class".into();
        std::fs::write(&state_path, serde_json::to_vec(&different).unwrap()).unwrap();
        assert!(
            resolve_plugin(
                &root,
                &root,
                &spec,
                PluginRole::Instrument,
                &mut dependencies
            )
            .err()
            .unwrap()
            .message
            .contains("different VST3")
        );

        let mut invalid = snapshot;
        invalid.state_data = Some("invalid-base64".into());
        std::fs::write(&state_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(
            resolve_plugin(
                &root,
                &root,
                &spec,
                PluginRole::Instrument,
                &mut dependencies
            )
            .err()
            .unwrap()
            .message
            .contains("base64")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
