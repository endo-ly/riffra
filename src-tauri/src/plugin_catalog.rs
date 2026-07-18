use crate::plugins::ScanReport;
use serde::Deserialize;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCatalog {
    plugins: Vec<StoredPlugin>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPlugin {
    name: String,
    path: PathBuf,
    scan_state: String,
}

pub fn save(data_root: &Path, report: &ScanReport) -> io::Result<()> {
    let catalog_dir = data_root.join("plugins");
    fs::create_dir_all(&catalog_dir)?;
    let current = catalog_dir.join("catalog.json");
    let temporary = catalog_dir.join(format!(".catalog-{}.tmp", std::process::id()));

    let payload = serde_json::to_vec_pretty(report)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    {
        let mut file = File::create(&temporary)?;
        file.write_all(&payload)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temporary, &current) {
        if current.exists() {
            fs::remove_file(&current)?;
            fs::rename(&temporary, &current)?;
        } else {
            return Err(error);
        }
    }
    Ok(())
}

pub fn validated_plugin(
    data_root: &Path,
    requested_path: &Path,
) -> Result<(String, PathBuf), String> {
    let catalog_path = data_root.join("plugins/catalog.json");
    let payload = fs::read(&catalog_path)
        .map_err(|error| format!("Validated plugin catalog could not be read: {error}"))?;
    let catalog = serde_json::from_slice::<StoredCatalog>(&payload)
        .map_err(|error| format!("Validated plugin catalog is invalid: {error}"))?;
    let requested = requested_path
        .canonicalize()
        .map_err(|error| format!("Requested VST3 path is unavailable: {error}"))?;
    let matching = catalog.plugins.into_iter().find(|plugin| {
        plugin
            .path
            .canonicalize()
            .is_ok_and(|catalog_path| catalog_path == requested)
    });
    let plugin = matching.ok_or_else(|| {
        format!(
            "The requested VST3 is not present in the current plugin catalog: {}",
            requested_path.display()
        )
    })?;
    if plugin.scan_state != "validated" {
        return Err(format!(
            "The requested VST3 is not validated (state: {}): {}",
            plugin.scan_state,
            requested_path.display()
        ));
    }
    Ok((plugin.name, plugin.path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{PluginEntry, ScanReport};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "riffra-catalog-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn replaces_catalog_atomically() {
        let root = test_root();
        let mut report = ScanReport {
            root: "C:\\VST3".into(),
            started_at_ms: 1,
            finished_at_ms: 2,
            plugins: vec![PluginEntry {
                id: "vst3-test".into(),
                name: "Test".into(),
                vendor: None,
                version: None,
                format: "VST3",
                path: "C:\\VST3\\Test.vst3".into(),
                bundle: true,
                modified_at_ms: None,
                scan_state: "validated",
            }],
            issues: vec![],
        };
        save(&root, &report).unwrap();
        report.plugins[0].name = "Updated".into();
        save(&root, &report).unwrap();

        let payload = fs::read_to_string(root.join("plugins/catalog.json")).unwrap();
        assert!(payload.contains("Updated"));
        assert!(!payload.contains(".catalog-"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_only_validated_catalog_plugins() {
        let root = test_root();
        let plugin_path = root.join("VST3/Amp.vst3");
        fs::create_dir_all(&plugin_path).unwrap();
        let report = ScanReport {
            root: root.to_string_lossy().into_owned(),
            started_at_ms: 1,
            finished_at_ms: 2,
            plugins: vec![PluginEntry {
                id: "vst3-amp".into(),
                name: "Amp".into(),
                vendor: Some("Vendor".into()),
                version: Some("1.0".into()),
                format: "VST3",
                path: plugin_path.to_string_lossy().into_owned(),
                bundle: true,
                modified_at_ms: None,
                scan_state: "validated",
            }],
            issues: vec![],
        };

        save(&root, &report).unwrap();
        let (name, resolved_path) = validated_plugin(&root, &plugin_path).unwrap();

        assert_eq!(name, "Amp");
        assert_eq!(resolved_path, plugin_path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_non_validated_catalog_plugins() {
        let root = test_root();
        let plugin_path = root.join("VST3/Amp.vst3");
        fs::create_dir_all(&plugin_path).unwrap();
        let report = ScanReport {
            root: root.to_string_lossy().into_owned(),
            started_at_ms: 1,
            finished_at_ms: 2,
            plugins: vec![PluginEntry {
                id: "vst3-amp".into(),
                name: "Amp".into(),
                vendor: None,
                version: None,
                format: "VST3",
                path: plugin_path.to_string_lossy().into_owned(),
                bundle: true,
                modified_at_ms: None,
                scan_state: "quarantined",
            }],
            issues: vec![],
        };

        save(&root, &report).unwrap();
        let error = validated_plugin(&root, &plugin_path).unwrap_err();

        assert!(error.contains("not validated"));
        let _ = fs::remove_dir_all(root);
    }
}
