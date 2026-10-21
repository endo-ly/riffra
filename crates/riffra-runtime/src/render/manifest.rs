//! Shared loading and atomic upgrades of saved Render manifests.

use crate::api::output::{RenderResult, RenderStemsResult};
use serde_json::Value;
use std::{fs, io::Write, path::Path, sync::Mutex};

// Only manifests requiring an upgrade take this lock. Re-reading under the lock
// ensures concurrent first readers persist the upgrade once.
static MIGRATION_GATE: Mutex<()> = Mutex::new(());

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub(super) enum StoredRender {
    Stems(RenderStemsResult),
    Timeline(RenderResult),
}

pub(super) fn read(path: &Path) -> Result<StoredRender, String> {
    let mut document = read_document(path)?;
    if !upgrade_document(&mut document) {
        return decode(document);
    }
    let _migration = MIGRATION_GATE
        .lock()
        .map_err(|_| "render manifest migration lock was poisoned".to_string())?;
    let mut document = read_document(path)?;
    let changed = upgrade_document(&mut document);
    let replacement = changed
        .then(|| serde_json::to_vec_pretty(&document))
        .transpose()
        .map_err(|error| format!("render manifest could not be encoded: {error}"))?;
    let manifest = decode(document)?;
    if let Some(bytes) = replacement {
        save(path, &bytes)?;
    }
    Ok(manifest)
}

fn read_document(path: &Path) -> Result<Value, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("render manifest could not be inspected: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("render manifest is not a regular file".into());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("render manifest could not be read: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("render manifest is invalid: {error}"))
}

fn decode(document: Value) -> Result<StoredRender, String> {
    serde_json::from_value(document).map_err(|error| format!("render manifest is invalid: {error}"))
}

fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        riffra_host::replace_file(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "render manifest migration could not be saved: {error}"
        ));
    }
    Ok(())
}

fn upgrade_document(document: &mut Value) -> bool {
    if let Some(stems) = document.get_mut("stems").and_then(Value::as_array_mut) {
        let mut changed = false;
        for stem in stems {
            changed |= upgrade_measurements(stem.get_mut("measurements"));
        }
        changed
    } else {
        upgrade_measurements(document.get_mut("measurements"))
    }
}

fn upgrade_measurements(value: Option<&mut serde_json::Value>) -> bool {
    let Some(measurements) = value.and_then(serde_json::Value::as_object_mut) else {
        return false;
    };
    if measurements.contains_key("sectionLoudnessState") {
        return false;
    }
    let Some(sections) = measurements
        .get("sections")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    let state = if sections.iter().all(|section| {
        section
            .get("loudness")
            .is_some_and(serde_json::Value::is_object)
    }) {
        serde_json::json!({"state":"measured"})
    } else {
        serde_json::json!({"state":"unavailable", "reason":"saved section loudness results are incomplete; remeasure the wav to obtain section loudness"})
    };
    measurements.insert("sectionLoudnessState".into(), state);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn derives_completion_only_from_saved_section_results_and_preserves_values() {
        let silent = json!({"loudness":{"integratedLufs":null,"truePeak":null},"bands":{"lowDb":-42,"midDb":null,"highDb":null}});
        let missing = json!({"loudness":null,"bands":{"lowDb":-42,"midDb":null,"highDb":null}});
        for (sections, expected) in [
            (json!([]), "measured"),
            (json!([silent.clone()]), "measured"),
            (json!([missing.clone()]), "unavailable"),
            (json!([silent, missing]), "unavailable"),
        ] {
            let original = json!({"sections":sections,"samplePeakDbfs":-1.0,"loudness":{"state":"unavailable","reason":"meter unavailable"}});
            let mut measurements = original.clone();

            assert!(upgrade_measurements(Some(&mut measurements)));
            assert_eq!(measurements["sectionLoudnessState"]["state"], expected);
            if expected == "unavailable" {
                assert!(
                    measurements["sectionLoudnessState"]["reason"]
                        .as_str()
                        .unwrap()
                        .contains("remeasure")
                );
            }
            assert!(!upgrade_measurements(Some(&mut measurements)));
            measurements
                .as_object_mut()
                .unwrap()
                .remove("sectionLoudnessState");
            assert_eq!(measurements, original);
        }
    }
}
