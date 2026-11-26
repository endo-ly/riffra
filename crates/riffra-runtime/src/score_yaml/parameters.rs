use crate::plugins::validation::{
    DisplayConversion, DisplayRequest, ParameterDescription, ParameterInfo,
};
use crate::score::production::ParameterValue;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn selector(value: &str) -> Result<(&str, Option<u32>), String> {
    if let Some((name, suffix)) = value.rsplit_once('#')
        && !suffix.is_empty()
        && suffix.bytes().all(|byte| byte.is_ascii_digit())
    {
        let index = suffix
            .parse()
            .map_err(|_| format!("parameter index is out of range: {value}"))?;
        if name.is_empty() {
            return Err(format!("parameter name is empty: {value}"));
        }
        return Ok((name, Some(index)));
    }
    Ok((value, None))
}

pub(super) fn find_parameter<'a>(
    parameters: &'a [ParameterInfo],
    selector_text: &str,
) -> Result<&'a ParameterInfo, String> {
    lookup(parameters, selector_text)?.ok_or_else(|| not_found(selector_text))
}

fn lookup<'a>(
    parameters: &'a [ParameterInfo],
    selector_text: &str,
) -> Result<Option<&'a ParameterInfo>, String> {
    let (name, index) = selector(selector_text)?;
    let matching: Vec<_> = parameters
        .iter()
        .filter(|parameter| {
            parameter.name == name && index.is_none_or(|index| parameter.index == index)
        })
        .collect();
    match matching.as_slice() {
        [parameter] => Ok(Some(parameter)),
        [] => Ok(None),
        _ => Err(format!(
            "plugin parameter {selector_text:?} is ambiguous; specify name#index"
        )),
    }
}

fn not_found(selector_text: &str) -> String {
    format!("plugin parameter {selector_text:?} was not found")
}

fn display_error(parameter: &ParameterInfo, display: &str, reason: &str) -> String {
    let choices = parameter
        .choices
        .iter()
        .map(|choice| choice.display_value.as_str())
        .collect::<Vec<_>>();
    let message = format!(
        "plugin parameter {:?} cannot use display value {display:?}: {reason}",
        parameter.name
    );
    if choices.is_empty() {
        message
    } else {
        format!("{message}; choices: {}", choices.join(", "))
    }
}

/// Builds one display request per display-valued parameter, in map order.
pub(super) fn display_requests(
    params: &BTreeMap<String, ParameterValue>,
) -> Result<Vec<DisplayRequest>, String> {
    let mut displays = Vec::new();
    for (name, value) in params {
        if let ParameterValue::Display(display) = value {
            let (name, index) = selector(name)?;
            displays.push(DisplayRequest {
                name: name.into(),
                index,
                display: display.clone(),
            });
        }
    }
    Ok(displays)
}

fn display_conversion(
    parameter: &ParameterInfo,
    conversions: &[Vec<DisplayConversion>],
    request_index: usize,
    display: &str,
) -> Result<f32, String> {
    let conversion = conversions
        .get(request_index)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .find(|value| value.index == parameter.index);
    match conversion {
        Some(conversion) if conversion.value.is_some() => {
            Ok(conversion.value.expect("conversion value"))
        }
        Some(conversion) => Err(display_error(
            parameter,
            display,
            conversion
                .error
                .as_deref()
                .unwrap_or("the plugin refused the conversion"),
        )),
        None => Err(display_error(
            parameter,
            display,
            "the plugin did not return a conversion",
        )),
    }
}

/// Resolves one automation display value which was requested for a use.
pub(super) fn automation_value(
    parameter: &ParameterInfo,
    conversions: &[Vec<DisplayConversion>],
    request_index: usize,
    display: &str,
) -> Result<f32, String> {
    display_conversion(parameter, conversions, request_index, display)
}

/// Assigns the requested values whose parameters exist and lists the selectors
/// that matched no parameter.
fn assign_present<'a>(
    parameters: &[ParameterInfo],
    base: &[f32],
    conversions: &[Vec<DisplayConversion>],
    params: &'a BTreeMap<String, ParameterValue>,
) -> Result<(Vec<f32>, Vec<&'a str>), String> {
    let mut values = base.to_vec();
    let mut assigned = BTreeSet::new();
    let mut missing = Vec::new();
    let mut display_index = 0;
    for (name, requested) in params {
        let parameter = lookup(parameters, name)?;
        let request_index = display_index;
        if matches!(requested, ParameterValue::Display(_)) {
            display_index += 1;
        }
        let Some(parameter) = parameter else {
            missing.push(name.as_str());
            continue;
        };
        if !assigned.insert(parameter.index) {
            return Err(format!("parameter {name:?} is specified more than once"));
        }
        values[parameter.index as usize] = match requested {
            ParameterValue::Normalized(value) => *value as f32,
            ParameterValue::Display(display) => {
                display_conversion(parameter, conversions, request_index, display)?
            }
        };
    }
    Ok((values, missing))
}

/// Requested parameter values resolved against the parameters the plugin finally exposes.
pub(super) struct Settled {
    pub(super) values: Vec<f32>,
    /// The plugin's description after the values that reconfigured it were applied,
    /// when the first description did not expose every requested parameter.
    pub(super) reconfigured: Option<ParameterDescription>,
}

/// Assigns requested values, describing the plugin again while applied values change
/// which parameters it exposes, as an effect type selecting its controls does.
///
/// `describe` receives the values to apply and returns the plugin's description after
/// they settled; each round must expose more of the requested parameters.
pub(super) fn settle(
    first: &ParameterDescription,
    conversions: &[Vec<DisplayConversion>],
    params: &BTreeMap<String, ParameterValue>,
    mut describe: impl FnMut(Vec<f32>) -> Result<ParameterDescription, String>,
) -> Result<Settled, String> {
    let (mut values, mut missing) = assign_present(
        &first.parameters,
        &first.parameter_values,
        conversions,
        params,
    )?;
    let mut reconfigured = None;
    while let Some(name) = missing.first() {
        if missing.len() == params.len() {
            return Err(not_found(name));
        }
        let description = describe(values)?;
        let (next, still_missing) = assign_present(
            &description.parameters,
            &description.parameter_values,
            &description.conversions,
            params,
        )?;
        if still_missing.len() >= missing.len() {
            return Err(not_found(still_missing[0]));
        }
        values = next;
        missing = still_missing;
        reconfigured = Some(description);
    }
    Ok(Settled {
        values,
        reconfigured,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::validation::ParameterChoice;

    fn parameter(index: u32, name: &str, choices: &[(f32, &str)]) -> ParameterInfo {
        ParameterInfo {
            index,
            parameter_id: Some(format!("id-{index}")),
            name: name.into(),
            default_value: 0.0,
            automatable: true,
            discrete: false,
            choices: choices
                .iter()
                .map(|(value, display)| ParameterChoice {
                    value: *value,
                    display_value: (*display).into(),
                })
                .collect(),
        }
    }

    fn conversion(index: u32, value: Option<f32>, error: Option<&str>) -> DisplayConversion {
        DisplayConversion {
            index,
            value,
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn selectors_require_an_exact_name_or_an_explicit_index() {
        // Arrange
        let parameters = [
            parameter(1, "Mode", &[]),
            parameter(4, "Mode", &[]),
            parameter(5, "Mix", &[]),
        ];

        // Act & Assert
        assert!(
            find_parameter(&parameters, "Mode")
                .unwrap_err()
                .contains("specify name#index")
        );
        assert_eq!(find_parameter(&parameters, "Mode#4").unwrap().index, 4);
        assert_eq!(
            find_parameter(&parameters, "Mix#0").unwrap_err(),
            "plugin parameter \"Mix#0\" was not found"
        );
    }

    #[test]
    fn parameter_values_keep_unmentioned_parameters_and_resolve_displays() {
        // Arrange
        let parameters = [
            parameter(0, "Gain", &[]),
            parameter(2, "Mode", &[(0.0, "Cold"), (0.5, "Warm")]),
        ];
        let base = [0.25, 0.5, 0.75];
        let mut params = BTreeMap::new();
        params.insert("Gain".into(), ParameterValue::Normalized(0.9));
        params.insert("Mode#2".into(), ParameterValue::Display("Warm".into()));
        let conversions = [vec![conversion(2, Some(0.5), None)]];

        // Act
        let (values, missing) = assign_present(&parameters, &base, &conversions, &params).unwrap();

        // Assert
        assert_eq!(values, [0.9, 0.5, 0.5]);
        assert!(missing.is_empty());

        let mut duplicate = BTreeMap::new();
        duplicate.insert("Mode".into(), ParameterValue::Normalized(0.1));
        duplicate.insert("Mode#2".into(), ParameterValue::Display("Warm".into()));
        assert!(
            assign_present(&parameters, &base, &conversions, &duplicate)
                .unwrap_err()
                .contains("specified more than once")
        );

        let mut unresolvable = BTreeMap::new();
        unresolvable.insert("Mode".into(), ParameterValue::Display("Hot".into()));
        let failed = [vec![conversion(
            2,
            None,
            Some("Plugin cannot convert this display text unambiguously."),
        )]];
        let error = assign_present(&parameters, &base, &failed, &unresolvable).unwrap_err();
        assert!(error.contains("Hot"), "{error}");
        assert!(error.contains("Cold, Warm"), "{error}");
    }

    #[test]
    fn values_that_reconfigure_the_plugin_settle_before_its_new_controls_resolve() {
        // Arrange
        let description = |names: [&str; 3],
                           values: [f32; 3],
                           state: &str|
         -> ParameterDescription {
            serde_json::from_value(serde_json::json!({
                "type": "pluginParameters",
                "path": "effect.vst3",
                "parameters": names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| serde_json::json!({"index": index, "name": name, "choices": []}))
                    .collect::<Vec<_>>(),
                "parameterValues": values,
                "stateData": state,
                "conversions": []
            }))
            .unwrap()
        };
        let delay = description(
            ["Delay Time", "Output Mix", "FX Type"],
            [0.4, 0.5, 0.0],
            "delay",
        );
        let mut params = BTreeMap::new();
        params.insert("FX Type".into(), ParameterValue::Normalized(0.5));
        params.insert("Output Mix".into(), ParameterValue::Normalized(0.7));
        params.insert("Pre-Delay".into(), ParameterValue::Normalized(0.25));
        let mut applied = Vec::new();

        // Act
        let settled = settle(&delay, &[], &params, |values| {
            applied.push(values);
            Ok(description(
                ["Pre-Delay", "Output Mix", "FX Type"],
                [0.3, 0.5, 0.5],
                "reverb",
            ))
        })
        .unwrap();

        // Assert
        assert_eq!(applied, [vec![0.4, 0.7, 0.5]]);
        assert_eq!(settled.values, [0.25, 0.7, 0.5]);
        assert_eq!(
            settled.reconfigured.unwrap().state_data.as_deref(),
            Some("reverb")
        );
        params.insert("Shimmer".into(), ParameterValue::Normalized(0.1));
        let error = settle(&delay, &[], &params, |_| {
            Ok(description(
                ["Pre-Delay", "Output Mix", "FX Type"],
                [0.3, 0.5, 0.5],
                "reverb",
            ))
        })
        .err()
        .unwrap();
        assert_eq!(error, "plugin parameter \"Shimmer\" was not found");
        let unknown = BTreeMap::from([("Shimmer".to_owned(), ParameterValue::Normalized(0.1))]);
        let error = settle(&delay, &[], &unknown, |_| unreachable!())
            .err()
            .unwrap();
        assert_eq!(error, "plugin parameter \"Shimmer\" was not found");
    }
}
