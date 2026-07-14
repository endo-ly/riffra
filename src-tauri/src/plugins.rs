use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_ENTRIES: usize = 100_000;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub format: &'static str,
    pub path: String,
    pub bundle: bool,
    pub modified_at_ms: Option<u64>,
    pub scan_state: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanIssue {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub root: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub plugins: Vec<PluginEntry>,
    pub issues: Vec<ScanIssue>,
}

pub fn discover(root: &Path) -> ScanReport {
    discover_with_cancel(root, None).unwrap_or_else(|message| ScanReport {
        root: root.to_string_lossy().into_owned(),
        started_at_ms: epoch_ms(SystemTime::now()),
        finished_at_ms: epoch_ms(SystemTime::now()),
        plugins: Vec::new(),
        issues: vec![issue(root, message)],
    })
}

pub fn discover_with_cancel(
    root: &Path,
    cancelled: Option<&AtomicBool>,
) -> Result<ScanReport, String> {
    let started_at_ms = epoch_ms(SystemTime::now());
    let mut plugins = Vec::new();
    let mut issues = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    let mut visited = HashSet::new();
    let mut entries_seen = 0usize;

    while let Some(directory) = pending.pop() {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err("VST3 scan cancelled; the previous catalog remains unchanged.".into());
        }
        let canonical = match directory.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                issues.push(issue(&directory, format!("Folder is unavailable: {error}")));
                continue;
            }
        };
        if !visited.insert(canonical) {
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                issues.push(issue(&directory, format!("Folder cannot be read: {error}")));
                continue;
            }
        };

        for entry in entries {
            if entries_seen.is_multiple_of(128)
                && cancelled.is_some_and(|flag| flag.load(Ordering::Acquire))
            {
                return Err("VST3 scan cancelled; the previous catalog remains unchanged.".into());
            }
            entries_seen += 1;
            if entries_seen > MAX_ENTRIES {
                issues.push(issue(
                    root,
                    "Scan stopped at the 100,000-entry safety limit.".into(),
                ));
                pending.clear();
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    issues.push(issue(
                        &directory,
                        format!("An entry could not be read: {error}"),
                    ));
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    issues.push(issue(&path, format!("Entry type is unavailable: {error}")));
                    continue;
                }
            };
            if is_vst3(&path) {
                plugins.push(plugin_entry(root, &path, file_type.is_dir()));
            } else if file_type.is_dir() && !file_type.is_symlink() {
                pending.push(path);
            }
        }
    }

    plugins.sort_by_key(|left| left.name.to_lowercase());
    Ok(ScanReport {
        root: root.to_string_lossy().into_owned(),
        started_at_ms,
        finished_at_ms: epoch_ms(SystemTime::now()),
        plugins,
        issues,
    })
}

fn is_vst3(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("vst3"))
}

fn plugin_entry(root: &Path, path: &Path, bundle: bool) -> PluginEntry {
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Unknown VST3")
        .to_owned();
    let vendor = path
        .parent()
        .filter(|parent| *parent != root)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.eq_ignore_ascii_case("vst3"))
        .map(str::to_owned);
    let path_string = path.to_string_lossy().into_owned();
    PluginEntry {
        id: format!("vst3-{:016x}", fnv1a(path_string.to_lowercase().as_bytes())),
        name,
        vendor,
        version: None,
        format: "VST3",
        path: path_string,
        bundle,
        modified_at_ms: path
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .map(epoch_ms),
        scan_state: "discovered",
    }
}

fn issue(path: &Path, message: String) -> ScanIssue {
    ScanIssue {
        path: path.to_string_lossy().into_owned(),
        message,
    }
}

fn epoch_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_file_and_bundle_plugins_without_entering_bundles() {
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-scan-{}",
            epoch_ms(SystemTime::now())
        ));
        let vendor = root.join("Vendor");
        fs::create_dir_all(vendor.join("Amp.vst3/Contents/x86_64-win")).unwrap();
        fs::write(root.join("Loose.vst3"), []).unwrap();
        fs::write(vendor.join("Amp.vst3/Contents/x86_64-win/Amp.vst3"), []).unwrap();

        let report = discover(&root);
        assert_eq!(report.plugins.len(), 2);
        assert!(
            report
                .plugins
                .iter()
                .any(|plugin| plugin.name == "Amp" && plugin.bundle)
        );
        assert!(
            report
                .plugins
                .iter()
                .any(|plugin| plugin.name == "Loose" && !plugin.bundle)
        );
        let _ = fs::remove_dir_all(root);
    }
}
