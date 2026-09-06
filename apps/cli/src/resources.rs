use std::path::{Path, PathBuf};

const BUILT_IN_INSTRUMENTS_ROOT_ENV: &str = "RIFFRA_BUILTIN_INSTRUMENTS_ROOT";

/// Resolves resources installed beside the `riffra` executable.
pub fn built_in_instruments_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os(BUILT_IN_INSTRUMENTS_ROOT_ENV).map(PathBuf::from) {
        return validate_override(root);
    }
    let executable = std::env::current_exe()
        .map_err(|error| format!("riffra executable path could not be resolved: {error}"))?;
    built_in_instruments_root_from(&executable)
}

fn validate_override(root: PathBuf) -> Result<PathBuf, String> {
    if root.as_os_str().is_empty() {
        return Err(format!("{BUILT_IN_INSTRUMENTS_ROOT_ENV} must not be empty"));
    }
    Ok(root)
}

fn built_in_instruments_root_from(executable: &Path) -> Result<PathBuf, String> {
    let directory = executable
        .parent()
        .ok_or_else(|| "riffra executable has no parent directory".to_string())?;
    Ok(directory
        .join("riffra-resources")
        .join("instruments")
        .join("builtin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_resource_root_takes_precedence_over_executable_location() {
        let root = PathBuf::from("/tmp/riffra-builtins");
        let executable = Path::new("/opt/riffra/bin/riffra");
        assert_eq!(validate_override(root.clone()).unwrap(), root);
        assert_ne!(built_in_instruments_root_from(executable).unwrap(), root);
    }
}
