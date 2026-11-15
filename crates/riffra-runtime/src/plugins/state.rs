use crate::api::output::{PluginEntry, PluginStateSnapshot};
use riffra_control::{ErrorCode, ProtocolError};

#[derive(Debug, thiserror::Error)]
pub(crate) enum PluginStateError {
    #[error("plugin state schemaVersion must be 2")]
    SchemaVersion,
    #[error("plugin state belongs to a different VST3")]
    DifferentPlugin,
    #[error("plugin state parameter values must be finite")]
    NonFiniteParameters,
    #[error("plugin catalog entry has no stable identifier")]
    MissingIdentifier,
}

impl From<PluginStateError> for ProtocolError {
    fn from(error: PluginStateError) -> Self {
        let code = match error {
            PluginStateError::DifferentPlugin => ErrorCode::CommandFailed,
            _ => ErrorCode::InvalidRequest,
        };
        Self::new(code, error.to_string())
    }
}

pub(crate) fn validate_plugin_state(
    state: &PluginStateSnapshot,
    plugin: &PluginEntry,
) -> Result<(), PluginStateError> {
    if state.schema_version != 2 {
        return Err(PluginStateError::SchemaVersion);
    }
    let identifier = plugin
        .identifier
        .as_deref()
        .ok_or(PluginStateError::MissingIdentifier)?;
    if state.plugin_identifier != identifier {
        return Err(PluginStateError::DifferentPlugin);
    }
    if state
        .parameter_values
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err(PluginStateError::NonFiniteParameters);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::output::{PluginFormat, PluginScanState};

    fn entry(identifier: Option<&str>) -> PluginEntry {
        PluginEntry {
            id: "synth".into(),
            name: "Synth".into(),
            vendor: None,
            version: None,
            identifier: identifier.map(str::to_owned),
            format: PluginFormat::Vst3,
            role: Some(crate::api::output::PluginRole::Instrument),
            path: "Synth.vst3".into(),
            bundle: true,
            modified_at_ms: None,
            scan_state: PluginScanState::Validated,
        }
    }

    fn state(identifier: &str) -> PluginStateSnapshot {
        PluginStateSnapshot {
            schema_version: 2,
            plugin_identifier: identifier.into(),
            plugin_path: "old-machine/Synth.vst3".into(),
            parameter_values: vec![0.5],
            state_data: Some("c3RhdGU=".into()),
        }
    }

    #[test]
    fn matches_plugin_identity_independently_of_the_install_path() {
        assert!(validate_plugin_state(&state("class-a"), &entry(Some("class-a"))).is_ok());
        assert!(matches!(
            validate_plugin_state(&state("class-b"), &entry(Some("class-a"))),
            Err(PluginStateError::DifferentPlugin)
        ));
        assert!(matches!(
            validate_plugin_state(&state("class-a"), &entry(None)),
            Err(PluginStateError::MissingIdentifier)
        ));
    }

    #[test]
    fn rejects_old_schemas_and_non_finite_parameters() {
        let mut old = state("class-a");
        old.schema_version = 1;
        assert!(matches!(
            validate_plugin_state(&old, &entry(Some("class-a"))),
            Err(PluginStateError::SchemaVersion)
        ));

        let mut invalid = state("class-a");
        invalid.parameter_values[0] = f32::NAN;
        assert!(matches!(
            validate_plugin_state(&invalid, &entry(Some("class-a"))),
            Err(PluginStateError::NonFiniteParameters)
        ));
    }
}
