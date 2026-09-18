//! Canonical instrument assignments for Instrument Tracks.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// One instrument assigned to a timeline Track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackInstrument {
    pub id: String,
    pub name: String,
    pub bypassed: bool,
    pub source: TrackInstrumentSource,
}

/// The implementation source of a Track instrument.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(rename_all_fields = "camelCase")]
pub enum TrackInstrumentSource {
    /// An instrument definition rendered by Riffra's built-in runtime.
    Internal {
        #[serde(rename = "definitionJson")]
        #[ts(rename = "definitionJson")]
        definition_json: String,
        resource: InternalInstrumentResource,
    },
    /// An external VST3 instrument and its persisted plugin state.
    Vst3 {
        path: String,
        #[serde(default)]
        #[serde(rename = "parameterValues")]
        #[ts(rename = "parameterValues")]
        parameter_values: Vec<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        #[serde(rename = "stateData")]
        #[ts(rename = "stateData")]
        state_data: Option<String>,
        #[serde(default)]
        #[serde(rename = "disabledPlaceholder")]
        #[ts(rename = "disabledPlaceholder")]
        disabled_placeholder: bool,
    },
}

/// Resource origin for an internal instrument definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(rename_all_fields = "camelCase")]
pub enum InternalInstrumentResource {
    /// A preset shipped in the Riffra resource bundle.
    BuiltInPreset {
        #[serde(rename = "presetId")]
        #[ts(rename = "presetId")]
        preset_id: String,
    },
    /// A package snapshot owned by the Project at Apply time.
    UserSnapshot {
        #[serde(rename = "instrumentId")]
        #[ts(rename = "instrumentId")]
        instrument_id: String,
        #[serde(rename = "snapshotId")]
        #[ts(rename = "snapshotId")]
        snapshot_id: String,
    },
}

impl TrackInstrument {
    /// Creates a VST3 instrument assignment.
    pub fn vst3(id: String, name: String, path: String) -> Result<Self, String> {
        let mut instrument = Self {
            id,
            name,
            bypassed: false,
            source: TrackInstrumentSource::Vst3 {
                path,
                parameter_values: Vec::new(),
                state_data: None,
                disabled_placeholder: false,
            },
        };
        validate_and_normalize(&mut instrument)?;
        Ok(instrument)
    }

    /// Creates an assignment from a resolved built-in preset definition.
    pub fn built_in(
        id: String,
        name: String,
        preset_id: String,
        definition_json: String,
    ) -> Result<Self, String> {
        let mut instrument = Self {
            id,
            name,
            bypassed: false,
            source: TrackInstrumentSource::Internal {
                definition_json,
                resource: InternalInstrumentResource::BuiltInPreset { preset_id },
            },
        };
        validate_and_normalize(&mut instrument)?;
        Ok(instrument)
    }

    /// Creates an assignment from a User Instrument package snapshot.
    pub fn user_snapshot(
        id: String,
        name: String,
        instrument_id: String,
        snapshot_id: String,
        definition_json: String,
    ) -> Result<Self, String> {
        let mut instrument = Self {
            id,
            name,
            bypassed: false,
            source: TrackInstrumentSource::Internal {
                definition_json,
                resource: InternalInstrumentResource::UserSnapshot {
                    instrument_id,
                    snapshot_id,
                },
            },
        };
        validate_and_normalize(&mut instrument)?;
        Ok(instrument)
    }

    /// Returns the stable slot identity.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the user-facing instrument name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns whether the instrument output is bypassed.
    pub fn bypassed(&self) -> bool {
        self.bypassed
    }

    /// Changes the output bypass state.
    pub fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    /// Returns the VST3 source when this is an external instrument.
    pub fn as_vst3(&self) -> Option<Vst3InstrumentSource<'_>> {
        match &self.source {
            TrackInstrumentSource::Vst3 {
                path,
                parameter_values,
                state_data,
                disabled_placeholder,
            } => Some(Vst3InstrumentSource {
                path,
                parameter_values,
                state_data: state_data.as_deref(),
                disabled_placeholder: *disabled_placeholder,
            }),
            TrackInstrumentSource::Internal { .. } => None,
        }
    }

    /// Returns the mutable VST3 source when this is an external instrument.
    pub fn as_vst3_mut(&mut self) -> Option<Vst3InstrumentSourceMut<'_>> {
        match &mut self.source {
            TrackInstrumentSource::Vst3 {
                path,
                parameter_values,
                state_data,
                disabled_placeholder,
            } => Some(Vst3InstrumentSourceMut {
                path,
                parameter_values,
                state_data,
                disabled_placeholder,
            }),
            TrackInstrumentSource::Internal { .. } => None,
        }
    }

    /// Returns the internal source when this is a built-in instrument.
    pub fn as_internal(&self) -> Option<(&str, &InternalInstrumentResource)> {
        match &self.source {
            TrackInstrumentSource::Internal {
                definition_json,
                resource,
            } => Some((definition_json, resource)),
            TrackInstrumentSource::Vst3 { .. } => None,
        }
    }

    /// Returns the built-in preset ID, if this is a built-in instrument.
    pub fn built_in_preset_id(&self) -> Option<&str> {
        match &self.source {
            TrackInstrumentSource::Internal {
                resource: InternalInstrumentResource::BuiltInPreset { preset_id },
                ..
            } => Some(preset_id),
            TrackInstrumentSource::Internal { .. } => None,
            TrackInstrumentSource::Vst3 { .. } => None,
        }
    }

    /// Returns the User Instrument and Project snapshot IDs, if assigned.
    pub fn user_snapshot_ids(&self) -> Option<(&str, &str)> {
        match &self.source {
            TrackInstrumentSource::Internal {
                resource:
                    InternalInstrumentResource::UserSnapshot {
                        instrument_id,
                        snapshot_id,
                    },
                ..
            } => Some((instrument_id, snapshot_id)),
            _ => None,
        }
    }
}

/// Read-only VST3 fields used by runtime adapters without exposing enum
/// matching at every call site.
pub struct Vst3InstrumentSource<'a> {
    pub path: &'a str,
    pub parameter_values: &'a [f32],
    pub state_data: Option<&'a str>,
    pub disabled_placeholder: bool,
}

/// Mutable VST3 fields used by Core-owned plugin state operations.
pub struct Vst3InstrumentSourceMut<'a> {
    pub path: &'a mut String,
    pub parameter_values: &'a mut Vec<f32>,
    pub state_data: &'a mut Option<String>,
    pub disabled_placeholder: &'a mut bool,
}

/// Validates and normalizes one instrument assignment.
pub(crate) fn validate_and_normalize(instrument: &mut TrackInstrument) -> Result<(), String> {
    if instrument.id.trim().is_empty() || instrument.name.trim().is_empty() {
        return Err("Track instruments require non-empty ids and names.".into());
    }
    instrument.id = instrument.id.trim().to_owned();
    instrument.name = instrument.name.trim().to_owned();

    match &mut instrument.source {
        TrackInstrumentSource::Internal {
            definition_json,
            resource,
        } => {
            if definition_json.trim().is_empty() {
                return Err("internal instrument definition must not be empty".into());
            }
            match resource {
                InternalInstrumentResource::BuiltInPreset { preset_id } => {
                    if preset_id.trim().is_empty() {
                        return Err("built-in instrument preset id must not be empty".into());
                    }
                    *preset_id = preset_id.trim().to_owned();
                }
                InternalInstrumentResource::UserSnapshot {
                    instrument_id,
                    snapshot_id,
                } => {
                    validate_user_instrument_id(instrument_id)?;
                    validate_uuid(snapshot_id, "user instrument snapshot id")?;
                }
            }
        }
        TrackInstrumentSource::Vst3 {
            path,
            parameter_values,
            state_data,
            ..
        } => {
            if path.trim().is_empty() {
                return Err("VST3 instrument path must not be empty".into());
            }
            *path = path.trim().to_owned();
            for value in parameter_values {
                *value = if value.is_finite() {
                    value.clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            if let Some(state) = state_data.as_mut()
                && state.chars().count() > 4_000_000
            {
                *state = state.chars().take(4_000_000).collect();
            }
        }
    }
    Ok(())
}

fn validate_user_instrument_id(value: &str) -> Result<(), String> {
    let Some(uuid) = value.strip_prefix("user:") else {
        return Err("user instrument id must use the user: prefix".into());
    };
    validate_uuid(uuid, "user instrument id")
}

fn validate_uuid(value: &str, label: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(value).map_err(|_| format!("{label} must be a UUID"))?;
    if parsed.to_string() != value {
        return Err(format!("{label} must use canonical lowercase UUID form"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built_in() -> TrackInstrument {
        TrackInstrument::built_in(
            "device:instrument".into(),
            "Clean Sub Bass".into(),
            "01-clean-sub-bass".into(),
            r#"{"schemaVersion":1}"#.into(),
        )
        .unwrap()
    }

    #[test]
    fn built_in_round_trips_without_vst3_state() {
        let instrument = built_in();

        let encoded = serde_json::to_value(&instrument).unwrap();
        let decoded: TrackInstrument = serde_json::from_value(encoded).unwrap();

        assert_eq!(decoded, instrument);
        assert_eq!(decoded.built_in_preset_id(), Some("01-clean-sub-bass"));
        assert!(decoded.as_vst3().is_none());
    }

    #[test]
    fn built_in_definition_is_retained_as_opaque_payload() {
        let instrument = TrackInstrument::built_in(
            "device:instrument".into(),
            "Opaque".into(),
            "opaque".into(),
            r#"{"completelyOpaque":"value"}"#.into(),
        )
        .unwrap();

        assert_eq!(
            instrument.as_internal().map(|(definition, _)| definition),
            Some(r#"{"completelyOpaque":"value"}"#)
        );
    }

    #[test]
    fn user_snapshot_round_trips_with_canonical_ids() {
        let instrument_id = format!("user:{}", Uuid::now_v7());
        let snapshot_id = Uuid::now_v7().to_string();
        let instrument = TrackInstrument::user_snapshot(
            "device:instrument".into(),
            "User Piano".into(),
            instrument_id.clone(),
            snapshot_id.clone(),
            r#"{"schemaVersion":1}"#.into(),
        )
        .unwrap();

        let encoded = serde_json::to_value(&instrument).unwrap();
        let decoded: TrackInstrument = serde_json::from_value(encoded).unwrap();

        assert_eq!(decoded, instrument);
        assert_eq!(
            decoded.user_snapshot_ids(),
            Some((instrument_id.as_str(), snapshot_id.as_str()))
        );
        assert!(decoded.built_in_preset_id().is_none());
    }

    #[test]
    fn vst3_state_is_normalized_without_rack_conversion() {
        let mut instrument = TrackInstrument {
            id: "device:instrument".into(),
            name: "Synth".into(),
            bypassed: false,
            source: TrackInstrumentSource::Vst3 {
                path: "  Synth.vst3 ".into(),
                parameter_values: vec![-1.0, 0.5, 2.0, f32::NAN],
                state_data: Some("state".into()),
                disabled_placeholder: false,
            },
        };

        validate_and_normalize(&mut instrument).unwrap();

        let vst3 = match instrument.source {
            TrackInstrumentSource::Vst3 {
                path,
                parameter_values,
                ..
            } => (path, parameter_values),
            TrackInstrumentSource::Internal { .. } => unreachable!(),
        };
        assert_eq!(vst3.0, "Synth.vst3");
        assert_eq!(vst3.1, [0.0, 0.5, 1.0, 0.0]);
    }
}
