use crate::plugins::ScanReport;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{PluginEntry, ScanReport};
    use crate::storage::now_ms;

    #[test]
    fn replaces_catalog_atomically() {
        let root = std::env::temp_dir().join(format!("riffra-catalog-{}", now_ms()));
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
}
