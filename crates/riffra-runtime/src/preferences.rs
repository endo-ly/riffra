//! Persisted audio-device preferences and platform defaults.

use crate::{AudioAccessMode, AudioState, AudioStatus};
use riffra_host::replace_file;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[cfg(windows)]
const DEFAULT_DRIVER: &str = "Windows Audio (Low Latency Mode)";
#[cfg(unix)]
const DEFAULT_DRIVER: &str = "ALSA";
#[cfg(not(any(windows, unix)))]
const DEFAULT_DRIVER: &str = "Windows Audio (Low Latency Mode)";

/// Persisted audio-device selection used by the live Host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPreferences {
    /// Native driver name.
    pub driver: String,
    /// Optional input device name.
    pub input_device: Option<String>,
    /// Input channel index.
    pub input_channel: u32,
    /// Optional output device name.
    pub output_device: Option<String>,
    /// Requested sample rate.
    pub sample_rate: Option<u32>,
    /// Requested buffer size.
    pub buffer_size: Option<u32>,
}

/// Audio preference input shared by shell adapters and the Host workflow.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDriverConfig {
    /// Native driver name.
    pub driver: String,
    /// Optional input device name.
    pub input_device: Option<String>,
    /// Input channel index.
    pub input_channel: u32,
    /// Optional output device name.
    pub output_device: Option<String>,
    /// Requested sample rate.
    pub sample_rate: Option<u32>,
    /// Requested buffer size.
    pub buffer_size: Option<u32>,
}

impl AudioPreferences {
    /// Converts persisted preferences into an update request.
    pub fn as_driver_config(&self) -> AudioDriverConfig {
        AudioDriverConfig {
            driver: self.driver.clone(),
            input_device: self.input_device.clone(),
            input_channel: self.input_channel,
            output_device: self.output_device.clone(),
            sample_rate: self.sample_rate,
            buffer_size: self.buffer_size,
        }
    }

    /// Validates and normalizes user-provided values.
    pub fn validate_and_normalize(mut self) -> Result<Self, String> {
        self.driver = normalize_required_text(&self.driver, "Audio driver")?;
        self.input_device = normalize_optional_text(self.input_device, "Audio input device")?;
        self.output_device = normalize_optional_text(self.output_device, "Audio output device")?;
        if let Some(rate) = self.sample_rate
            && !(8_000..=192_000).contains(&rate)
        {
            return Err("Audio sample rate preference is outside 8-192 kHz".into());
        }
        if let Some(buffer) = self.buffer_size
            && !(16..=8192).contains(&buffer)
        {
            return Err("Audio buffer preference is outside 16-8192 samples".into());
        }
        Ok(self)
    }

    /// Derives effective preferences from a native status response.
    pub fn from_effective_status(status: &AudioStatus) -> Result<Self, String> {
        Self {
            driver: status
                .driver
                .clone()
                .ok_or_else(|| "native audio did not report an active driver".to_string())?,
            input_device: status.input_device.clone(),
            input_channel: status.input_channel.unwrap_or_default(),
            output_device: status.output_device.clone(),
            sample_rate: status.sample_rate,
            buffer_size: status.buffer_size,
        }
        .validate_and_normalize()
    }
}

impl Default for AudioPreferences {
    fn default() -> Self {
        Self {
            driver: DEFAULT_DRIVER.into(),
            input_device: None,
            input_channel: 0,
            output_device: None,
            sample_rate: None,
            buffer_size: None,
        }
    }
}

/// Durable store for Host-wide audio preferences.
pub struct AudioPreferencesStore {
    path: PathBuf,
}

impl AudioPreferencesStore {
    /// Creates the store rooted at a Host data directory.
    pub fn new(data_root: &Path) -> Self {
        Self {
            path: data_root.join("settings").join("audio.json"),
        }
    }

    /// Loads preferences, returning `None` when no file exists.
    pub fn load(&self) -> Result<Option<AudioPreferences>, String> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&self.path)
            .map_err(|error| format!("audio preferences could not be read: {error}"))?;
        serde_json::from_slice::<AudioPreferences>(&bytes)
            .map_err(|error| format!("audio preferences are invalid: {error}"))?
            .validate_and_normalize()
            .map(Some)
    }

    /// Atomically saves normalized preferences.
    pub fn save(&self, preferences: &AudioPreferences) -> Result<(), String> {
        let preferences = preferences.clone().validate_and_normalize()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "audio preferences path has no parent folder".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("audio preferences folder could not be created: {error}"))?;
        let temporary = self.path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(&preferences)
            .map_err(|error| format!("audio preferences could not be encoded: {error}"))?;
        fs::write(&temporary, payload)
            .map_err(|error| format!("audio preferences could not be written: {error}"))?;
        replace_file(&temporary, &self.path)
            .map_err(|error| format!("audio preferences could not be finalized: {error}"))
    }
}

/// Loads persisted preferences or writes platform defaults.
pub fn load_or_default(data_root: &Path) -> Result<AudioPreferences, String> {
    let store = AudioPreferencesStore::new(data_root);
    match store.load()? {
        Some(preferences) => Ok(preferences),
        None => {
            let preferences = AudioPreferences::default();
            store.save(&preferences)?;
            Ok(preferences)
        }
    }
}

/// Reports how a native driver shares its device.
pub fn access_mode_for_driver(driver: &str) -> AudioAccessMode {
    if driver.eq_ignore_ascii_case("Windows Audio")
        || driver.eq_ignore_ascii_case("Windows Audio (Low Latency Mode)")
        || driver.eq_ignore_ascii_case("DirectSound")
        || driver.eq_ignore_ascii_case("ALSA")
    {
        AudioAccessMode::Shared
    } else if driver.eq_ignore_ascii_case("Windows Audio (Exclusive Mode)") {
        AudioAccessMode::Exclusive
    } else {
        AudioAccessMode::DriverManaged
    }
}

/// Confirms that a native status reflects the requested device configuration.
pub fn active_device_matches_preferences(
    status: &AudioStatus,
    preferences: &AudioPreferences,
) -> bool {
    matches!(status.state, AudioState::Ready | AudioState::Muted)
        && status
            .driver
            .as_deref()
            .is_some_and(|driver| driver.eq_ignore_ascii_case(&preferences.driver))
        && optional_text_matches(&status.input_device, &preferences.input_device)
        && status.input_channel == Some(preferences.input_channel)
        && optional_text_matches(&status.output_device, &preferences.output_device)
        && preferences
            .sample_rate
            .is_none_or(|sample_rate| status.sample_rate == Some(sample_rate))
        && preferences
            .buffer_size
            .is_none_or(|buffer_size| status.buffer_size == Some(buffer_size))
}

fn normalize_required_text(value: &str, label: &str) -> Result<String, String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if normalized.chars().count() > 256 {
        return Err(format!("{label} is too long"));
    }
    Ok(normalized.into())
}

fn normalize_optional_text(value: Option<String>, label: &str) -> Result<Option<String>, String> {
    value
        .map(|value| normalize_required_text(&value, label))
        .transpose()
}

fn optional_text_matches(actual: &Option<String>, expected: &Option<String>) -> bool {
    expected.as_deref().is_none_or(|expected| {
        actual
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_driver_matches_the_native_platform() {
        #[cfg(windows)]
        assert_eq!(
            AudioPreferences::default().driver,
            "Windows Audio (Low Latency Mode)"
        );
        #[cfg(unix)]
        assert_eq!(AudioPreferences::default().driver, "ALSA");
    }

    #[test]
    fn preferences_round_trip_through_the_store() {
        let root = std::env::temp_dir().join(format!(
            "riffra-runtime-preferences-{}",
            riffra_host::now_ms()
        ));
        let store = AudioPreferencesStore::new(&root);
        let preferences = AudioPreferences {
            driver: "ALSA".into(),
            input_device: Some("Input".into()),
            input_channel: 1,
            output_device: Some("Output".into()),
            sample_rate: Some(48_000),
            buffer_size: Some(256),
        };

        store.save(&preferences).unwrap();

        assert_eq!(store.load().unwrap(), Some(preferences));
        let _ = std::fs::remove_dir_all(root);
    }
}
