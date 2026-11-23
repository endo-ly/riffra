//! Execution projection of one rig outside the Project.

use super::contract::{GraphPluginDevice, RigProjection};
use super::project::project_effects;
use super::resolve::resolve_effects;
use crate::api::output::ProjectionDiagnostics;
use riffra_core::Rig;

/// Projects one rig for the realtime sidecar.
///
/// The prepared rig chain survives projections, so its projection carries
/// bypassed effects and the native side applies the bypass. Unavailable
/// effects are removed by the same rule the arrangement graph applies.
pub(crate) fn project_rig(rig: &Rig, revision: u64) -> RigProjection {
    let resources = resolve_effects(&rig.effects);
    let mut diagnostics = ProjectionDiagnostics::default();
    let effects: Vec<GraphPluginDevice> =
        project_effects(&rig.effects, &resources, &mut diagnostics, true);
    if !diagnostics.missing_device_ids.is_empty() {
        tracing::warn!(
            rig_id = %rig.id,
            missing_device_ids = ?diagnostics.missing_device_ids,
            "rig projection omitted unavailable devices"
        );
    }
    RigProjection {
        rig_id: rig.id.clone(),
        revision,
        audio_input_channel: rig.audio_input.channel_index,
        gain_db: rig.gain_db,
        effects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::EffectDevice;

    fn plugin_device(id: &str, path: &str, bypassed: bool) -> EffectDevice {
        let mut device = EffectDevice::new(id.into(), "Effect".into(), path.into()).unwrap();
        device.bypassed = bypassed;
        device
    }

    #[test]
    fn projection_carries_available_effects_with_their_bypass_and_the_channel_gain() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-rig-projection-{}",
            riffra_control::new_instance_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let existing = root.join("effect.vst3");
        std::fs::write(&existing, b"plugin").unwrap();
        let mut rig = Rig::create("Live".into(), 2).unwrap();
        rig.gain_db = -3.0;
        rig.effects = vec![
            plugin_device("device:kept", &existing.to_string_lossy(), false),
            plugin_device("device:bypassed", &existing.to_string_lossy(), true),
            plugin_device(
                "device:missing",
                &root.join("missing.vst3").to_string_lossy(),
                false,
            ),
        ];

        // Act
        let projection = project_rig(&rig, 7);

        // Assert
        assert_eq!(projection.rig_id, rig.id);
        assert_eq!(projection.revision, 7);
        assert_eq!(projection.audio_input_channel, 2);
        assert_eq!(projection.gain_db, -3.0);
        assert_eq!(
            projection
                .effects
                .iter()
                .map(|device| (device.id.as_str(), device.state.bypassed))
                .collect::<Vec<_>>(),
            [("device:kept", false), ("device:bypassed", true)]
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
