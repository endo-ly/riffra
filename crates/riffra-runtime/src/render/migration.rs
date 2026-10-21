//! Upgrades saved Render measurements while the Data Root is exclusively owned.

use std::{fs, path::Path};

/// Persists explicit section measurement states before Render services are exposed.
pub(crate) fn migrate_measurement_states(data_root: &Path) -> Result<(), String> {
    let Some(root) = super::history::render_store_root(data_root)? else {
        return Ok(());
    };
    for entry in
        fs::read_dir(root).map_err(|e| format!("render migration store could not be read: {e}"))?
    {
        let entry = entry.map_err(|e| format!("render migration entry could not be read: {e}"))?;
        if !entry.file_name().to_string_lossy().starts_with("render-")
            || !entry
                .file_type()
                .map_err(|e| format!("render migration entry could not be inspected: {e}"))?
                .is_dir()
        {
            continue;
        }
        let path = entry.path().join("render.json");
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(format!(
                    "render migration manifest could not be inspected: {e}"
                ));
            }
        }
        let bytes = fs::read(&path)
            .map_err(|e| format!("render migration manifest could not be read: {e}"))?;
        let Ok(mut manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let changed = if let Some(stems) = manifest
            .get_mut("stems")
            .and_then(serde_json::Value::as_array_mut)
        {
            let mut changed = false;
            for stem in stems {
                changed |= upgrade_measurements(stem.get_mut("measurements"));
            }
            changed
        } else {
            upgrade_measurements(manifest.get_mut("measurements"))
        };
        if !changed
            || serde_json::from_value::<super::history::StoredRender>(manifest.clone()).is_err()
        {
            continue;
        }
        let temporary = path.with_extension("json.tmp");
        let result = (|| {
            let mut file = fs::File::create(&temporary)?;
            serde_json::to_writer_pretty(&mut file, &manifest)?;
            file.sync_all()?;
            drop(file);
            riffra_host::replace_file(&temporary, &path)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "render manifest migration could not be saved: {error}"
            ));
        }
    }
    Ok(())
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
