//! Track, marker, range, and automation command helpers.

use super::{DispatchError, HostDispatcher};
use crate::api::params::TimebaseUpdateParams;
use riffra_core::ProjectTimebase;

impl HostDispatcher<'_> {
    pub(super) fn timebase_update(
        &self,
        current: ProjectTimebase,
        params: TimebaseUpdateParams,
    ) -> Result<ProjectTimebase, DispatchError> {
        if params.bpm.is_none()
            && params.time_signature_numerator.is_none()
            && params.time_signature_denominator.is_none()
        {
            return Err(DispatchError::invalid_request(
                "timebase update requires at least one field",
            ));
        }
        Ok(ProjectTimebase {
            ppq: current.ppq,
            bpm: params.bpm.unwrap_or(current.bpm),
            time_signature_numerator: params
                .time_signature_numerator
                .unwrap_or(current.time_signature_numerator),
            time_signature_denominator: params
                .time_signature_denominator
                .unwrap_or(current.time_signature_denominator),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use crate::test_support::{command, mutated_session, output_value};
    use riffra_host::now_ms;
    use serde_json::json;
    use std::fs;

    #[test]
    fn track_list_omits_device_parameter_values() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-track-list-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let added = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Keys","kind":"instrument"})),
                None,
            )
            .unwrap();
        let track_id = mutated_session(&added).arrangement.tracks[0].id.clone();
        let instrument = dispatcher
            .dispatch(
                command(
                    "instrument.vst3.set",
                    json!({
                        "trackId": track_id,
                        "pluginPath": "C:\\Plugins\\Synth.vst3"
                    }),
                ),
                None,
            )
            .unwrap();
        let device_id = mutated_session(&instrument).arrangement.tracks[0]
            .instrument
            .as_ref()
            .unwrap()
            .id
            .clone();
        dispatcher
            .dispatch(
                command(
                    "device.parameter.set",
                    json!({
                        "trackId": track_id,
                        "deviceId": device_id,
                        "parameterIndex": 0,
                        "value": 0.5
                    }),
                ),
                None,
            )
            .unwrap();
        dispatcher
            .dispatch(
                command(
                    "effect.add",
                    json!({"trackId": track_id, "pluginPath": "C:/Plugins/Delay.vst3"}),
                ),
                None,
            )
            .unwrap();

        let listed = output_value(
            &dispatcher
                .dispatch(command("track.list", json!({})), None)
                .unwrap(),
        );
        let track = &listed[0];
        assert_eq!(track["name"], "Keys");
        assert!(track["instrument"].get("parameterValues").is_none());
        let effects = track["effects"].as_array().unwrap();
        assert_eq!(effects.len(), 1);
        assert!(effects[0].get("parameterValues").is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timebase_update_patches_only_the_requested_fields() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-timebase-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();

        let updated = dispatcher
            .dispatch(command("timebase.update", json!({"bpm": 140.0})), None)
            .unwrap();
        let session = mutated_session(&updated);
        assert_eq!(session.arrangement.timebase.ppq, 960);
        assert_eq!(session.arrangement.timebase.bpm, 140.0);
        assert_eq!(session.arrangement.timebase.time_signature_numerator, 4);
        assert_eq!(session.arrangement.timebase.time_signature_denominator, 4);

        let updated = dispatcher
            .dispatch(
                command(
                    "timebase.update",
                    json!({
                        "bpm": 100.0,
                        "timeSignatureNumerator": 7,
                        "timeSignatureDenominator": 8
                    }),
                ),
                None,
            )
            .unwrap();
        assert_eq!(
            mutated_session(&updated).arrangement.timebase,
            riffra_core::ProjectTimebase {
                ppq: 960,
                bpm: 100.0,
                time_signature_numerator: 7,
                time_signature_denominator: 8,
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn marker_and_ranges_convert_positions_using_the_current_meter() {
        let root =
            std::env::temp_dir().join(format!("riffra-dispatcher-musical-position-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();

        dispatcher
            .dispatch(
                command(
                    "timebase.update",
                    json!({"timeSignatureNumerator":3,"timeSignatureDenominator":4}),
                ),
                None,
            )
            .unwrap();
        let marker = dispatcher
            .dispatch(
                command("marker.add", json!({"name":"Chorus","position":"5:1"})),
                None,
            )
            .unwrap();
        assert_eq!(
            mutated_session(&marker).arrangement.markers[0].tick,
            4 * 3 * 960
        );

        let looped = dispatcher
            .dispatch(
                command(
                    "loop-range.set",
                    json!({"enabled":true,"start":"5:1","end":"9:1"}),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&looped);
        assert_eq!(session.arrangement.loop_range.start_tick.0, 4 * 3 * 960);
        assert_eq!(session.arrangement.loop_range.end_tick.0, 8 * 3 * 960);

        let punched = dispatcher
            .dispatch(
                command(
                    "punch-range.set",
                    json!({"enabled":true,"start":"9:1","end":"13:1"}),
                ),
                None,
            )
            .unwrap();
        let punch = mutated_session(&punched).arrangement.punch_range.unwrap();
        assert_eq!(punch.start_tick.0, 8 * 3 * 960);
        assert_eq!(punch.end_tick.0, 12 * 3 * 960);
        let _ = fs::remove_dir_all(root);
    }
}
