//! track command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "track.list"
            | "track.add"
            | "track.update"
            | "track.remove"
            | "track.duplicate"
            | "track.reorder"
            | "track.audio-input.set"
            | "track.audio-input.clear"
            | "track.midi-input.set"
            | "track.midi-input.clear"
            | "marker.add"
            | "marker.update"
            | "marker.remove"
            | "timebase.update"
            | "loop-range.set"
            | "punch-range.set"
            | "automation.set"
            | "automation.clear"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "track.list" => dispatcher.value(
            "tracks",
            canonical
                .session
                .arrangement
                .tracks
                .iter()
                .map(TrackSummary::from_track)
                .collect::<Vec<_>>(),
        ),
        "track.add" => {
            let params: TrackAddParams = decode(request.params)?;
            dispatcher.application_mutation(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .add_track_with_created_ids(params.name, parse_track_kind(&params.kind)?)?,
                CanonicalMutationEffect::ProjectArrangement,
            )
        }
        "track.update" => {
            let params: TrackUpdateParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_track(&params.track_id, params.patch)?,
            )
        }
        "track.remove" => {
            let params: TrackIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .remove_track(&params.track_id)?,
            )
        }
        "track.duplicate" => {
            let params: TrackIdParams = decode(request.params)?;
            dispatcher.application_mutation(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .duplicate_track_with_created_ids(&params.track_id)?,
                CanonicalMutationEffect::ProjectArrangement,
            )
        }
        "track.reorder" => {
            let params: ReorderParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .reorder_track(&params.track_id, params.target_index)?,
            )
        }
        "track.audio-input.set" => {
            let params: AudioInputParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_audio_input(&params.track_id, Some(params.channel_index))?,
            )
        }
        "track.audio-input.clear" => {
            let params: TrackIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_audio_input(&params.track_id, None)?,
            )
        }
        "track.midi-input.set" => {
            let params: MidiInputParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_midi_input(
                        &params.track_id,
                        MidiInputRoute {
                            device_id: params.device_id,
                            channel: params.channel,
                        },
                    )?,
            )
        }
        "track.midi-input.clear" => {
            let params: TrackIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_midi_input(&params.track_id, MidiInputRoute::default())?,
            )
        }
        "marker.add" => {
            let params: MarkerAddParams = decode(request.params)?;
            let tick = canonical
                .session
                .arrangement
                .timebase
                .musical_position_to_tick(params.position)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
            dispatcher.application_mutation(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .add_marker_with_created_ids(tick, params.name)?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "marker.update" => {
            let params: MarkerUpdateParams = decode(request.params)?;
            let tick = params
                .position
                .map(|position| {
                    canonical
                        .session
                        .arrangement
                        .timebase
                        .musical_position_to_tick(position)
                        .map_err(|error| DispatchError::invalid_request(error.to_string()))
                })
                .transpose()?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_marker(
                        &params.marker_id,
                        MarkerPatch {
                            name: params.name,
                            tick,
                        },
                    )?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "marker.remove" => {
            let params: MarkerIdParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .remove_marker(&params.marker_id)?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "timebase.update" => {
            let params: TimebasePatchParams = decode(request.params)?;
            if params.is_empty() {
                return Err(DispatchError::invalid_request(
                    "timebase update requires at least one field",
                ));
            }
            let current = canonical.session.arrangement.timebase;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_timebase(ProjectTimebase {
                        ppq: current.ppq,
                        bpm: params.bpm.unwrap_or(current.bpm),
                        time_signature_numerator: params
                            .time_signature_numerator
                            .unwrap_or(current.time_signature_numerator),
                        time_signature_denominator: params
                            .time_signature_denominator
                            .unwrap_or(current.time_signature_denominator),
                    })?,
            )
        }
        "loop-range.set" => {
            let params: RangeParams = decode(request.params)?;
            let start = canonical
                .session
                .arrangement
                .timebase
                .musical_position_to_tick(params.start)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
            let end = canonical
                .session
                .arrangement
                .timebase
                .musical_position_to_tick(params.end)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_loop_range(params.enabled, start, end)?,
            )
        }
        "punch-range.set" => {
            let params: RangeParams = decode(request.params)?;
            let start = canonical
                .session
                .arrangement
                .timebase
                .musical_position_to_tick(params.start)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
            let end = canonical
                .session
                .arrangement
                .timebase
                .musical_position_to_tick(params.end)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_punch_range(params.enabled, start, end)?,
            )
        }
        "automation.set" => {
            let params: AutomationParams = decode(request.params)?;
            dispatcher.application_mutation(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_automation_with_created_ids(
                        &params.track_id,
                        parse_automation_parameter(&params.parameter)?,
                        params.points,
                    )?,
                CanonicalMutationEffect::ProjectArrangement,
            )
        }
        "automation.clear" => {
            let params: AutomationClearParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_automation(
                        &params.track_id,
                        parse_automation_parameter(&params.parameter)?,
                        Vec::new(),
                    )?,
            )
        }
        _ => unreachable!("unsupported track command family"),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackAddParams {
    name: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackUpdateParams {
    track_id: String,
    #[serde(flatten)]
    patch: TrackPatch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderParams {
    track_id: String,
    target_index: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioInputParams {
    pub(crate) track_id: String,
    pub(crate) channel_index: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MidiInputParams {
    pub(crate) track_id: String,
    pub(crate) device_id: Option<String>,
    pub(crate) channel: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerAddParams {
    name: String,
    position: riffra_core::MusicalPosition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerUpdateParams {
    marker_id: String,
    name: Option<String>,
    position: Option<riffra_core::MusicalPosition>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerIdParams {
    marker_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct TimebasePatchParams {
    bpm: Option<f64>,
    time_signature_numerator: Option<u8>,
    time_signature_denominator: Option<u8>,
}

impl TimebasePatchParams {
    fn is_empty(&self) -> bool {
        self.bpm.is_none()
            && self.time_signature_numerator.is_none()
            && self.time_signature_denominator.is_none()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RangeParams {
    enabled: bool,
    start: riffra_core::MusicalPosition,
    end: riffra_core::MusicalPosition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomationParams {
    track_id: String,
    parameter: String,
    points: Vec<AutomationPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomationClearParams {
    track_id: String,
    parameter: String,
}
#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use riffra_control::ControlCommand;
    use riffra_host::now_ms;
    use serde_json::{Value, json};
    use std::fs;

    fn request(command: &str, params: Value) -> ControlCommand {
        ControlCommand {
            name: command.into(),
            params,
        }
    }

    #[test]
    fn track_list_omits_device_parameter_values() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-track-list-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let added = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(added.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        let instrument = dispatcher
            .dispatch(request(
                "instrument.vst3.set",
                json!({
                    "trackId": track_id,
                    "pluginPath": "C:\\Plugins\\Synth.vst3"
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession =
            serde_json::from_value(instrument.value["canonical"]["session"].clone()).unwrap();
        let device_id = session.arrangement.tracks[0]
            .instrument
            .as_ref()
            .unwrap()
            .id
            .clone();
        dispatcher
            .dispatch(request(
                "device.parameter.set",
                json!({
                    "trackId": track_id,
                    "deviceId": device_id,
                    "parameterIndex": 0,
                    "value": 0.5
                }),
            ))
            .unwrap();

        let listed = dispatcher
            .dispatch(request("track.list", json!({})))
            .unwrap();
        let track = &listed.value[0];
        assert_eq!(track["name"], "Keys");
        assert!(track["instrument"].get("parameterValues").is_none());
        assert!(
            track["rack"]["devices"]
                .as_array()
                .unwrap()
                .iter()
                .all(|device| device.get("parameterValues").is_none())
        );
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
            .dispatch(request("timebase.update", json!({"bpm": 140.0})))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(updated.value).unwrap();
        assert_eq!(session.arrangement.timebase.ppq, 960);
        assert_eq!(session.arrangement.timebase.bpm, 140.0);
        assert_eq!(session.arrangement.timebase.time_signature_numerator, 4);
        assert_eq!(session.arrangement.timebase.time_signature_denominator, 4);

        let updated = dispatcher
            .dispatch(request(
                "timebase.update",
                json!({
                    "bpm": 100.0,
                    "timeSignatureNumerator": 7,
                    "timeSignatureDenominator": 8
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(updated.value).unwrap();
        assert_eq!(
            session.arrangement.timebase,
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
    fn timebase_update_rejects_ppq_as_an_external_field() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-ppq-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let error = dispatcher
            .dispatch(request("timebase.update", json!({"ppq": 960})))
            .unwrap_err();
        assert!(matches!(error, super::DispatchError::InvalidRequest(_)));
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
            .dispatch(request(
                "timebase.update",
                json!({"timeSignatureNumerator":3,"timeSignatureDenominator":4}),
            ))
            .unwrap();
        let marker = dispatcher
            .dispatch(request(
                "marker.add",
                json!({"name":"Chorus","position":"5:1"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(marker.value).unwrap();
        assert_eq!(session.arrangement.markers[0].tick, 4 * 3 * 960);

        let looped = dispatcher
            .dispatch(request(
                "loop-range.set",
                json!({"enabled":true,"start":"5:1","end":"9:1"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(looped.value).unwrap();
        assert_eq!(session.arrangement.loop_range.start_tick.0, 4 * 3 * 960);
        assert_eq!(session.arrangement.loop_range.end_tick.0, 8 * 3 * 960);

        let punched = dispatcher
            .dispatch(request(
                "punch-range.set",
                json!({"enabled":true,"start":"9:1","end":"13:1"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(punched.value).unwrap();
        let punch = session.arrangement.punch_range.unwrap();
        assert_eq!(punch.start_tick.0, 8 * 3 * 960);
        assert_eq!(punch.end_tick.0, 12 * 3 * 960);

        assert!(
            dispatcher
                .dispatch(request("marker.add", json!({"name":"Invalid","tick":960})))
                .is_err()
        );
        assert!(
            dispatcher
                .dispatch(request(
                    "loop-range.set",
                    json!({"enabled":true,"startTick":960,"endTick":1920}),
                ))
                .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }
}
