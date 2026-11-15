use super::DispatchError;
use crate::api::output::{
    AutomationEvaluation, AutomationEvaluationInterpolation, AutomationEvaluationPoint,
    AutomationValueUnit,
};
use crate::api::params::AutomationEvaluateParams;
use riffra_core::{
    AutomationInterpolation, AutomationLane, AutomationTarget, BusMixParameter, CreativeSession,
    ProjectTimebase, TrackMixParameter,
};

const MAX_POINTS: usize = 4096;

pub(super) fn evaluate(
    session: &CreativeSession,
    params: AutomationEvaluateParams,
) -> Result<AutomationEvaluation, DispatchError> {
    let arrangement = &session.arrangement;
    if !arrangement.has_automation_target(&params.target) {
        return Err(DispatchError::invalid_request("unknown automation target"));
    }
    if params.sample_rate == 0 {
        return Err(DispatchError::invalid_request(
            "sample rate must be positive",
        ));
    }
    let timebase = &arrangement.timebase;
    let start = timebase
        .musical_position_to_tick(params.start)
        .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
    let end = timebase
        .musical_position_to_tick(params.end)
        .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
    let boundaries = timebase
        .musical_grid_boundaries(start, end, params.grid, MAX_POINTS)
        .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
    let lane = arrangement
        .automation_lanes
        .iter()
        .find(|lane| lane.target == params.target);
    let (unit, static_value) = static_value(session, &params.target);
    let prepared = lane
        .map(|lane| prepare_lane(lane, timebase, params.sample_rate))
        .transpose()?;
    if prepared.as_ref().is_none_or(|points| points.is_empty()) && static_value.is_none() {
        return Err(DispatchError::invalid_request(
            "device parameter has no automation lane",
        ));
    }
    let interpolation = match lane
        .filter(|lane| !lane.points.is_empty())
        .map(|lane| lane.interpolation)
    {
        Some(AutomationInterpolation::Step) => AutomationEvaluationInterpolation::Step,
        Some(AutomationInterpolation::Linear) => AutomationEvaluationInterpolation::Linear,
        Some(AutomationInterpolation::LinearAmplitude { duration_seconds }) => {
            AutomationEvaluationInterpolation::LinearAmplitude { duration_seconds }
        }
        None => AutomationEvaluationInterpolation::Static,
    };
    let amplitude = matches!(
        interpolation,
        AutomationEvaluationInterpolation::LinearAmplitude { .. }
    );
    let step = matches!(interpolation, AutomationEvaluationInterpolation::Step);
    let mut points = Vec::with_capacity(boundaries.len() - 1);
    for tick in boundaries.into_iter().take_while(|tick| *tick != end) {
        let sample = sample_at(timebase, tick.0, params.sample_rate)?;
        let value = match prepared.as_deref().filter(|points| !points.is_empty()) {
            Some(points) => f64::from(value_at(points, sample, step, amplitude)),
            None => static_value.expect("static value validated"),
        };
        points.push(AutomationEvaluationPoint {
            position: timebase.tick_to_musical_position(tick),
            tick: tick.0,
            sample,
            value,
        });
    }
    Ok(AutomationEvaluation {
        target: params.target,
        unit,
        sample_rate: params.sample_rate,
        interpolation,
        points,
    })
}

fn static_value(
    session: &CreativeSession,
    target: &AutomationTarget,
) -> (AutomationValueUnit, Option<f64>) {
    let arrangement = &session.arrangement;
    match target {
        AutomationTarget::TrackMix {
            track_id,
            parameter,
        } => {
            let track = arrangement
                .tracks
                .iter()
                .find(|track| track.id == *track_id)
                .expect("target validated");
            match parameter {
                TrackMixParameter::Volume => (AutomationValueUnit::Db, Some(track.gain_db)),
                TrackMixParameter::Pan => (AutomationValueUnit::Pan, Some(track.pan)),
            }
        }
        AutomationTarget::BusMix { bus_id, parameter } => {
            let bus = arrangement
                .mixer
                .buses
                .iter()
                .find(|bus| bus.id == *bus_id)
                .expect("target validated");
            match parameter {
                BusMixParameter::Gain => (AutomationValueUnit::Db, Some(bus.gain_db)),
                BusMixParameter::Pan => (AutomationValueUnit::Pan, Some(bus.pan)),
            }
        }
        AutomationTarget::MasterGain => (AutomationValueUnit::Db, Some(session.settings.master_db)),
        AutomationTarget::SendLevel { send_id } => (
            AutomationValueUnit::Db,
            Some(
                arrangement
                    .mixer
                    .sends
                    .iter()
                    .find(|send| send.id == *send_id)
                    .expect("target validated")
                    .level_db,
            ),
        ),
        AutomationTarget::SidechainSendLevel { sidechain_send_id } => (
            AutomationValueUnit::Db,
            Some(
                arrangement
                    .mixer
                    .sidechain_sends
                    .iter()
                    .find(|send| send.id == *sidechain_send_id)
                    .expect("target validated")
                    .level_db,
            ),
        ),
        AutomationTarget::DeviceParameter { .. } => (AutomationValueUnit::Normalized, None),
    }
}

fn sample_at(timebase: &ProjectTimebase, tick: u64, rate: u32) -> Result<u64, DispatchError> {
    let sample = timebase.ticks_to_seconds(tick) * f64::from(rate);
    if !sample.is_finite() || sample >= (i64::MAX - i64::from(i32::MAX)) as f64 {
        return Err(DispatchError::invalid_request(
            "automation position exceeds the sample range",
        ));
    }
    Ok(sample.round() as u64)
}

fn prepare_lane(
    lane: &AutomationLane,
    timebase: &ProjectTimebase,
    rate: u32,
) -> Result<Vec<(u64, f32)>, DispatchError> {
    let mut points: Vec<(u64, f32)> = Vec::new();
    for point in &lane.points {
        let sample = sample_at(timebase, point.tick.0, rate)?;
        if let AutomationInterpolation::LinearAmplitude { duration_seconds } = lane.interpolation {
            let value = 10.0_f64.powf(point.value / 20.0) as f32;
            if let Some(previous) = points.last().map(|point| point.1) {
                while points.last().is_some_and(|point| point.0 >= sample) {
                    points.pop();
                }
                let duration = (duration_seconds * f64::from(rate)).round().max(1.0);
                if !duration.is_finite()
                    || duration >= i64::MAX as f64
                    || sample > i64::MAX as u64 - duration as u64
                {
                    return Err(DispatchError::invalid_request(
                        "automation transition exceeds the sample range",
                    ));
                }
                points.push((sample, previous));
                points.push((sample + duration as u64, value));
            } else {
                points.push((sample, value));
            }
        } else {
            points.push((sample, point.value.max(-1000.0) as f32));
        }
    }
    Ok(points)
}

fn value_at(points: &[(u64, f32)], sample: u64, step: bool, amplitude: bool) -> f32 {
    let index = points.partition_point(|point| point.0 <= sample);
    let value = if index == 0 {
        points[0].1
    } else if index == points.len() {
        points[index - 1].1
    } else {
        let left = points[index - 1];
        let right = points[index];
        if step {
            left.1
        } else {
            left.1 + (right.1 - left.1) * ((sample - left.0) as f32 / (right.0 - left.0) as f32)
        }
    };
    if amplitude {
        if value > 0.0 {
            20.0 * value.log10()
        } else {
            -1000.0
        }
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{AutomationPoint, TimelineTick, Track};

    #[test]
    fn evaluation_uses_static_mix_values_and_sampled_lane_interpolation() {
        let mut session = CreativeSession::new(1);
        let mut track = Track::audio("track:1".into(), "Track".into());
        track.gain_db = -3.0;
        session.arrangement.tracks.push(track);
        let target = AutomationTarget::TrackMix {
            track_id: "track:1".into(),
            parameter: TrackMixParameter::Volume,
        };
        let params = AutomationEvaluateParams {
            target: target.clone(),
            start: "1:1".parse().unwrap(),
            end: "1:2".parse().unwrap(),
            grid: "1/16".parse().unwrap(),
            sample_rate: 100,
        };
        let static_result = evaluate(&session, params.clone()).unwrap();
        assert!(matches!(
            static_result.interpolation,
            AutomationEvaluationInterpolation::Static
        ));
        assert!(static_result.points.iter().all(|point| point.value == -3.0));
        assert_eq!(
            static_result
                .points
                .iter()
                .map(|point| point.sample)
                .collect::<Vec<_>>(),
            [0, 13, 25, 38]
        );

        session.arrangement.automation_lanes.push(AutomationLane {
            id: "lane".into(),
            target,
            interpolation: AutomationInterpolation::LinearAmplitude {
                duration_seconds: 0.5,
            },
            points: [
                (0, 0.0),
                (960, -6.020599913279624),
                (1440, -12.041199826559248),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (tick, value))| AutomationPoint {
                id: index.to_string(),
                tick: TimelineTick(tick),
                value,
            })
            .collect(),
        });
        let prepared = prepare_lane(
            &session.arrangement.automation_lanes[0],
            &session.arrangement.timebase,
            100,
        )
        .unwrap();
        assert_eq!(prepared, [(0, 1.0), (50, 1.0), (75, 0.5), (125, 0.25)]);
        assert!((value_at(&prepared, 100, false, true) - -8.519_375).abs() < 0.0001);
        let result = evaluate(
            &session,
            AutomationEvaluateParams {
                start: "1:2".parse().unwrap(),
                end: "1:3".parse().unwrap(),
                ..params
            },
        )
        .unwrap();
        assert_eq!(
            result
                .points
                .iter()
                .map(|point| point.sample)
                .collect::<Vec<_>>(),
            [50, 63, 75, 88]
        );
        assert!((result.points[2].value - -6.0206).abs() < 0.0001);
        assert_eq!(value_at(&[(0, -3.0), (50, -9.0)], 25, true, false), -3.0);
        assert_eq!(value_at(&[(0, -3.0), (50, -9.0)], 25, false, false), -6.0);
        assert_eq!(value_at(&[(0, -3.0), (50, -9.0)], 50, true, false), -9.0);
        session.arrangement.tracks[0]
            .effects
            .push(riffra_core::EffectDevice {
                id: "device:1".into(),
                name: "Effect".into(),
                bypassed: false,
                plugin: riffra_core::Vst3Plugin {
                    path: "effect.vst3".into(),
                    parameter_values: vec![],
                    state_data: None,
                    disabled_placeholder: false,
                },
            });
        let device = AutomationEvaluateParams {
            target: AutomationTarget::DeviceParameter {
                device_id: "device:1".into(),
                parameter_id: "parameter:1".into(),
            },
            start: "1:1".parse().unwrap(),
            end: "1:2".parse().unwrap(),
            grid: "1/4".parse().unwrap(),
            sample_rate: 100,
        };
        assert!(
            evaluate(&session, device)
                .unwrap_err()
                .to_string()
                .contains("no automation lane")
        );
    }
}
