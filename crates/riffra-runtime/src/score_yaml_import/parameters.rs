use std::collections::{BTreeMap, BTreeSet};

use crate::plugins::validation::{self, DisplayConversion, DisplayRequest, ParameterInfo};
use crate::score::production::ParameterValue;
use riffra_core::Vst3Plugin;

#[derive(Default)]
pub(super) struct ProbeCache {
    results: BTreeMap<String, validation::ParameterDescription>,
}

fn selector(value: &str) -> Result<(&str, Option<u32>), String> {
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

fn find_parameter<'a>(
    parameters: &'a [ParameterInfo],
    selector_text: &str,
) -> Result<&'a ParameterInfo, String> {
    let (name, index) = selector(selector_text)?;
    let matching: Vec<_> = parameters
        .iter()
        .filter(|parameter| {
            parameter.name == name && index.is_none_or(|index| parameter.index == index)
        })
        .collect();
    match matching.as_slice() {
        [parameter] => Ok(parameter),
        [] => Err(format!("plugin parameter {selector_text:?} was not found")),
        _ => Err(format!(
            "plugin parameter {selector_text:?} is ambiguous; specify name#index"
        )),
    }
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

fn assign_values(
    parameters: &[ParameterInfo],
    base: &[f32],
    conversions: &[Vec<DisplayConversion>],
    params: &BTreeMap<String, ParameterValue>,
) -> Result<Vec<f32>, String> {
    let mut values = base.to_vec();
    let mut assigned = BTreeSet::new();
    let mut display_index = 0;
    for (name, requested) in params {
        let parameter = find_parameter(parameters, name)?;
        if !assigned.insert(parameter.index) {
            return Err(format!("parameter {name:?} is specified more than once"));
        }
        let value = match requested {
            ParameterValue::Normalized(value) => *value as f32,
            ParameterValue::Display(display) => {
                let conversion = conversions
                    .get(display_index)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .find(|value| value.index == parameter.index);
                display_index += 1;
                let Some(value) = conversion.and_then(|value| value.value) else {
                    let reason = conversion
                        .and_then(|value| value.error.as_deref())
                        .unwrap_or("the plugin did not return a conversion");
                    return Err(display_error(parameter, display, reason));
                };
                value
            }
        };
        values[parameter.index as usize] = value;
    }
    Ok(values)
}

impl ProbeCache {
    pub(super) fn apply(
        &mut self,
        scanner: &std::path::Path,
        plugin: &mut Vst3Plugin,
        params: &BTreeMap<String, ParameterValue>,
    ) -> Result<(), String> {
        if params.is_empty() {
            return Ok(());
        }
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
        let key =
            serde_json::to_string(&(&*plugin, &displays)).map_err(|error| error.to_string())?;
        if !self.results.contains_key(&key) {
            self.results.insert(
                key.clone(),
                validation::describe_parameters(scanner, plugin, &displays)?,
            );
        }
        let description = &self.results[&key];
        plugin.parameter_values = assign_values(
            &description.parameters,
            &description.parameter_values,
            &description.conversions,
            params,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::validation::ParameterChoice;

    fn parameter(index: u32, name: &str, choices: &[(f32, &str)]) -> ParameterInfo {
        ParameterInfo {
            index,
            name: name.into(),
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
        let values = assign_values(&parameters, &base, &conversions, &params).unwrap();

        // Assert
        assert_eq!(values, [0.9, 0.5, 0.5]);

        let mut duplicate = BTreeMap::new();
        duplicate.insert("Mode".into(), ParameterValue::Normalized(0.1));
        duplicate.insert("Mode#2".into(), ParameterValue::Display("Warm".into()));
        assert!(
            assign_values(&parameters, &base, &conversions, &duplicate)
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
        let error = assign_values(&parameters, &base, &failed, &unresolvable).unwrap_err();
        assert!(error.contains("Hot"), "{error}");
        assert!(error.contains("Cold, Warm"), "{error}");
    }
}
