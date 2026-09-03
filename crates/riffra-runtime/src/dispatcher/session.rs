//! session command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "session.get"
            | "session.inspect"
            | "session.settings.update"
            | "history.get"
            | "undo"
            | "redo"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "session.get" => dispatcher.session(canonical.session.clone()),
        "session.inspect" => {
            let params: SessionInspectionQuery = decode(request.params)?;
            let inspection = inspect_canonical_state(&canonical, params)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
            dispatcher.value("sessionInspection", inspection)
        }
        "session.settings.update" => {
            let params: SessionSettingsPatch = decode(request.params)?;
            let effect = if params.metronome_enabled.is_some() {
                CanonicalMutationEffect::ProjectArrangement
            } else {
                CanonicalMutationEffect::CanonicalOnly
            };
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_session_settings(params)?,
                effect,
            )
        }
        "history.get" => dispatcher.value("history", canonical.history),
        "undo" => dispatcher.session(dispatcher.core.application(&dispatcher.storage).undo()?),
        "redo" => dispatcher.session(dispatcher.core.application(&dispatcher.storage).redo()?),
        _ => unreachable!("unsupported session command family"),
    })
}
