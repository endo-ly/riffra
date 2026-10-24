use crate::api::output::PluginStateSnapshot;
use riffra_control::{ErrorCode, ProtocolError};

#[derive(Debug, thiserror::Error)]
pub(crate) enum PluginStateError {
    #[error("plugin state schemaVersion must be 1")]
    SchemaVersion,
    #[error("plugin state belongs to a different VST3")]
    DifferentPlugin,
    #[error("plugin state parameter values must be finite")]
    NonFiniteParameters,
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
    plugin_path: &str,
) -> Result<(), PluginStateError> {
    if state.schema_version != 1 {
        return Err(PluginStateError::SchemaVersion);
    }
    if state.plugin_path != plugin_path {
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

    #[test]
    fn rejects_incompatible_saved_states() {
        let mut state = PluginStateSnapshot {
            schema_version: 1,
            plugin_path: "Synth.vst3".into(),
            parameter_values: vec![0.5],
            state_data: Some("c3RhdGU=".into()),
        };
        assert!(validate_plugin_state(&state, "Synth.vst3").is_ok());
        assert!(matches!(
            validate_plugin_state(&state, "Other.vst3"),
            Err(PluginStateError::DifferentPlugin)
        ));

        state.schema_version = 2;
        assert!(matches!(
            validate_plugin_state(&state, "Synth.vst3"),
            Err(PluginStateError::SchemaVersion)
        ));

        state.schema_version = 1;
        state.parameter_values[0] = f32::NAN;
        assert!(matches!(
            validate_plugin_state(&state, "Synth.vst3"),
            Err(PluginStateError::NonFiniteParameters)
        ));
    }
}
