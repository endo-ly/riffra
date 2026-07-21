//! Application-wide audio-device preferences.
//!
//! Audio devices belong to the Windows installation, not to a CreativeSession.
//! This module keeps the persisted preference, Tauri boundary, and JUCE runtime
//! consistent without attaching machine-specific settings to a project.

use crate::AppState;
use crate::model::{AudioAccessMode, AudioStatus};
use crate::native_audio::AudioSupervisor;
use crate::storage::replace_file;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, State};

const AUDIO_PREFERENCES_FORMAT: u32 = 2;
const DEFAULT_SHARED_DRIVER: &str = "Windows Audio (Low Latency Mode)";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPreferences {
    pub format_version: u32,
    pub driver: String,
    pub input_device: Option<String>,
    pub input_channel: u32,
    pub output_device: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
}

/// Request payload for a driver/device change. Carries the user-facing knobs
/// that the Audio Runtime and the persisted preferences share, without the
/// `format_version` metadata that belongs to `AudioPreferences` itself. The
/// Tauri command, the `apply_audio_preferences` workflow, and the
/// `AudioSupervisor` runtime call all take this so the parameter list stays
/// narrow enough for the runtime call sites to read at a glance.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDriverConfig {
    pub driver: String,
    pub input_device: Option<String>,
    pub input_channel: u32,
    pub output_device: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
}

impl AudioPreferences {
    /// Projects the persisted preferences into the runtime-call shape. Used
    /// when rolling a failed driver change back to the previous preferences.
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
}

impl Default for AudioPreferences {
    fn default() -> Self {
        Self {
            format_version: AUDIO_PREFERENCES_FORMAT,
            driver: DEFAULT_SHARED_DRIVER.into(),
            input_device: None,
            input_channel: 0,
            output_device: None,
            sample_rate: None,
            buffer_size: None,
        }
    }
}

impl AudioPreferences {
    pub fn validate_and_normalize(mut self) -> Result<Self, String> {
        if self.format_version != AUDIO_PREFERENCES_FORMAT {
            return Err(format!(
                "Audio preferences format {} is unsupported; expected {}.",
                self.format_version, AUDIO_PREFERENCES_FORMAT
            ));
        }
        self.driver = normalize_required_text(&self.driver, "Audio driver")?;
        self.input_device = normalize_optional_text(self.input_device, "Audio input device")?;
        self.output_device = normalize_optional_text(self.output_device, "Audio output device")?;
        if let Some(rate) = self.sample_rate
            && !(8_000..=192_000).contains(&rate)
        {
            return Err("Audio sample rate preference is outside 8-192 kHz.".into());
        }
        if let Some(buffer) = self.buffer_size
            && !(16..=8192).contains(&buffer)
        {
            return Err("Audio buffer preference is outside 16-8192 samples.".into());
        }
        self.format_version = AUDIO_PREFERENCES_FORMAT;
        Ok(self)
    }

    pub fn from_effective_status(status: &AudioStatus) -> Result<Self, String> {
        Self {
            format_version: AUDIO_PREFERENCES_FORMAT,
            driver: status
                .driver
                .clone()
                .ok_or_else(|| "Native audio did not report an active driver.".to_string())?,
            input_device: status.input_device.clone(),
            input_channel: status.input_channel.unwrap_or(0),
            output_device: status.output_device.clone(),
            sample_rate: status.sample_rate,
            buffer_size: status.buffer_size,
        }
        .validate_and_normalize()
    }
}

pub struct AudioPreferencesStore {
    path: PathBuf,
}

impl AudioPreferencesStore {
    pub fn new(data_root: &Path) -> Self {
        Self {
            path: data_root.join("settings").join("audio.json"),
        }
    }

    pub fn load(&self) -> Result<Option<AudioPreferences>, String> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&self.path)
            .map_err(|error| format!("Audio preferences could not be read: {error}"))?;
        let preferences: AudioPreferences = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Audio preferences are invalid: {error}"))?;
        preferences.validate_and_normalize().map(Some)
    }

    pub fn save(&self, preferences: &AudioPreferences) -> Result<(), String> {
        let preferences = preferences.clone().validate_and_normalize()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "Audio preferences path has no parent folder.".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Audio preferences folder could not be created: {error}"))?;
        let temporary = self.path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(&preferences)
            .map_err(|error| format!("Audio preferences could not be encoded: {error}"))?;
        fs::write(&temporary, payload)
            .map_err(|error| format!("Audio preferences could not be written: {error}"))?;
        replace_file(&temporary, &self.path)
            .map_err(|error| format!("Audio preferences could not be finalized: {error}"))
    }
}

pub struct AudioPreferencesContext<'a> {
    pub app: &'a AppHandle,
    pub audio: &'a AudioSupervisor,
    pub data_root: &'a Path,
    pub preferences: &'a Mutex<AudioPreferences>,
    pub safe_mode: bool,
}

pub fn load_or_default(data_root: &Path) -> Result<AudioPreferences, String> {
    let store = AudioPreferencesStore::new(data_root);
    match store.load() {
        Ok(Some(preferences)) => return Ok(preferences),
        Ok(None) => {}
        Err(_) => {
            let preferences = AudioPreferences::default();
            store.save(&preferences)?;
            return Ok(preferences);
        }
    }
    let preferences = AudioPreferences::default();
    store.save(&preferences)?;
    Ok(preferences)
}

fn apply_audio_preferences(
    context: &AudioPreferencesContext<'_>,
    config: AudioDriverConfig,
) -> Result<AudioStatus, String> {
    if context.safe_mode {
        return Err(
            "Safe Mode blocks audio-driver changes; restart Riffra without --safe-mode first."
                .into(),
        );
    }
    let requested = AudioPreferences {
        format_version: AUDIO_PREFERENCES_FORMAT,
        driver: config.driver,
        input_device: config.input_device,
        input_channel: config.input_channel,
        output_device: config.output_device,
        sample_rate: config.sample_rate,
        buffer_size: config.buffer_size,
    }
    .validate_and_normalize()?;
    let previous = context.preferences.lock().map_err(lock_error)?.clone();
    let mut audio = context
        .audio
        .set_audio_driver(context.app, &requested.as_driver_config())?;
    let effective = AudioPreferences::from_effective_status(&audio)?;
    if let Err(error) = AudioPreferencesStore::new(context.data_root).save(&effective) {
        let rollback = context
            .audio
            .set_audio_driver(context.app, &previous.as_driver_config());
        return Err(match rollback {
            Ok(_) => format!(
                "Audio preferences could not be saved; the previous audio device was restored: {error}"
            ),
            Err(rollback_error) => format!(
                "Audio preferences could not be saved and the previous audio device could not be restored: {error}; {rollback_error}"
            ),
        });
    }
    context.audio.set_restart_preferences(effective.clone())?;
    *context.preferences.lock().map_err(lock_error)? = effective;
    audio.message =
        match access_mode_for_driver(audio.driver.as_deref().unwrap_or(&requested.driver)) {
            AudioAccessMode::Shared => audio.message,
            AudioAccessMode::Exclusive => {
                "Exclusive audio is active; other applications using this device will be paused."
                    .into()
            }
            AudioAccessMode::DriverManaged => {
                "Audio sharing is controlled by this driver; other applications may be paused."
                    .into()
            }
        };
    Ok(audio)
}

#[tauri::command]
pub async fn set_audio_driver(
    app: AppHandle,
    config: AudioDriverConfig,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    apply_audio_preferences(
        &AudioPreferencesContext {
            app: &app,
            audio: &state.audio,
            data_root: &state.data_root,
            preferences: &state.audio_preferences,
            safe_mode: state.safe_mode,
        },
        config,
    )
}

pub fn access_mode_for_driver(driver: &str) -> AudioAccessMode {
    if driver.eq_ignore_ascii_case("Windows Audio")
        || driver.eq_ignore_ascii_case("Windows Audio (Low Latency Mode)")
        || driver.eq_ignore_ascii_case("DirectSound")
    {
        AudioAccessMode::Shared
    } else if driver.eq_ignore_ascii_case("Windows Audio (Exclusive Mode)") {
        AudioAccessMode::Exclusive
    } else {
        AudioAccessMode::DriverManaged
    }
}

fn normalize_required_text(value: &str, label: &str) -> Result<String, String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(format!("{label} must not be empty."));
    }
    if normalized.chars().count() > 256 {
        return Err(format!("{label} is too long."));
    }
    Ok(normalized.into())
}

fn normalize_optional_text(value: Option<String>, label: &str) -> Result<Option<String>, String> {
    value
        .map(|value| normalize_required_text(&value, label))
        .transpose()
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal audio-preference lock was poisoned: {error}");
    eprintln!("[riffra] {message}. Aborting to prevent corrupted state from propagating.");
    std::process::abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "riffra-audio-preferences-{name}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn defaults_to_low_latency_shared_windows_audio() {
        let preferences = AudioPreferences::default();
        assert_eq!(preferences.driver, "Windows Audio (Low Latency Mode)");
        assert_eq!(
            access_mode_for_driver(&preferences.driver),
            AudioAccessMode::Shared
        );
    }

    #[test]
    fn creates_shared_defaults_when_preferences_do_not_exist() {
        let root = root("default");
        let _ = fs::remove_dir_all(&root);
        let preferences = load_or_default(&root).unwrap();
        assert_eq!(preferences, AudioPreferences::default());
        assert_eq!(
            AudioPreferencesStore::new(&root).load().unwrap(),
            Some(preferences)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_app_preferences_are_loaded_unchanged() {
        let root = root("existing");
        let _ = fs::remove_dir_all(&root);
        let existing = AudioPreferences {
            driver: "ASIO".into(),
            input_channel: 1,
            sample_rate: Some(48_000),
            buffer_size: Some(128),
            ..AudioPreferences::default()
        };
        AudioPreferencesStore::new(&root).save(&existing).unwrap();
        assert_eq!(load_or_default(&root).unwrap(), existing);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replaces_previous_preference_format_with_current_defaults() {
        let root = root("old-format");
        let _ = fs::remove_dir_all(&root);
        let path = root.join("settings").join("audio.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            br#"{"formatVersion":1,"driver":"Windows Audio","inputDevice":null,"outputDevice":null,"sampleRate":48000,"bufferSize":480}"#,
        )
        .unwrap();

        let preferences = load_or_default(&root).unwrap();
        assert_eq!(preferences, AudioPreferences::default());
        assert_eq!(
            AudioPreferencesStore::new(&root).load().unwrap(),
            Some(AudioPreferences::default())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn classifies_windows_and_driver_managed_backends() {
        assert_eq!(
            access_mode_for_driver("Windows Audio"),
            AudioAccessMode::Shared
        );
        assert_eq!(
            access_mode_for_driver("Windows Audio (Exclusive Mode)"),
            AudioAccessMode::Exclusive
        );
        assert_eq!(
            access_mode_for_driver("ASIO"),
            AudioAccessMode::DriverManaged
        );
    }
}
