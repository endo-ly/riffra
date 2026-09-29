//! Audio runtime adapter operations.

use super::*;

/// Persists the master gain in canonical session settings and submits its graph
/// for projection.
pub fn set_master_gain_db(
    context: &SessionContext<'_>,
    gain_db: f64,
) -> Result<crate::api::output::ArrangementMutationResult, AdapterError> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    commit_core_application(context, |core, store| {
        core.application(store)
            .update_session_settings(SessionSettingsPatch {
                master_db: Some(gain_db),
                ..SessionSettingsPatch::default()
            })
    })?;
    arrangement_mutation_result(context)
}
