use std::path::{Path, PathBuf};

/// Explicit native executable paths required by a live Host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeBinaries {
    /// Isolated real-time audio engine.
    pub audio: PathBuf,
    /// Isolated VST3 scanner.
    pub plugin_scan: PathBuf,
    /// Isolated offline renderer.
    pub render: PathBuf,
}

impl RuntimeBinaries {
    /// Creates explicit binary paths.
    pub fn new(audio: PathBuf, plugin_scan: PathBuf, render: PathBuf) -> Self {
        Self {
            audio,
            plugin_scan,
            render,
        }
    }

    /// Resolves the three native executables beside a distribution binary.
    pub fn beside(executable: &Path) -> Result<Self, String> {
        let directory = executable
            .parent()
            .ok_or_else(|| "runtime executable has no parent directory".to_string())?;
        Ok(Self::new(
            directory.join(format!("riffra-audio{}", std::env::consts::EXE_SUFFIX)),
            directory.join(format!(
                "riffra-plugin-scan{}",
                std::env::consts::EXE_SUFFIX
            )),
            directory.join(format!("riffra-render{}", std::env::consts::EXE_SUFFIX)),
        ))
    }

    /// Resolves native executables beside the current process.
    pub fn beside_current_executable() -> Result<Self, String> {
        std::env::current_exe()
            .map_err(|error| format!("current executable could not be resolved: {error}"))
            .and_then(|path| Self::beside(&path))
    }
}
