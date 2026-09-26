use crate::BuiltInInstrumentCatalog;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub(crate) fn prepare_empty_built_in_resource_root(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("manifest.json"),
        br#"{"sourceRelease":"vtest","presets":[]}"#,
    )
    .unwrap();
}

pub(crate) fn prepare_built_in_resource_root(root: &Path) -> PathBuf {
    prepare_empty_built_in_resource_root(root);
    root.to_path_buf()
}

pub(crate) fn empty_built_in_catalog() -> &'static BuiltInInstrumentCatalog {
    static CATALOG: OnceLock<BuiltInInstrumentCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "riffra-runtime-empty-builtins-{}-{}",
            std::process::id(),
            riffra_control::new_instance_id()
        ));
        prepare_empty_built_in_resource_root(&root);
        BuiltInInstrumentCatalog::load(root).unwrap()
    })
}

#[cfg(test)]
pub(crate) fn write_validated_plugin_catalog(
    data_root: &Path,
    plugins: &[(PathBuf, crate::plugins::PluginRole)],
) {
    use crate::plugins::{PluginEntry, PluginFormat, PluginScanState, ScanReport};

    let entries = plugins
        .iter()
        .enumerate()
        .map(|(index, (path, role))| {
            fs::create_dir_all(path).unwrap();
            PluginEntry {
                id: format!("vst3-test-{index}"),
                name: path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Test Plugin")
                    .to_owned(),
                vendor: None,
                version: None,
                format: PluginFormat::Vst3,
                role: Some(*role),
                path: path.to_string_lossy().into_owned(),
                bundle: true,
                modified_at_ms: None,
                scan_state: PluginScanState::Validated,
            }
        })
        .collect();
    let report = ScanReport {
        root: data_root.to_string_lossy().into_owned(),
        started_at_ms: 0,
        finished_at_ms: 0,
        plugins: entries,
        issues: Vec::new(),
    };
    crate::plugins::save(data_root, &report).unwrap();
}
