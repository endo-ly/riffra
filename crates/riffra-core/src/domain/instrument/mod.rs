//! Canonical instrument assignments for Instrument Tracks.

use crate::domain::plugin::Vst3Plugin;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// One instrument assigned to a timeline Track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrackInstrument {
    pub id: String,
    pub name: String,
    pub bypassed: bool,
    pub source: TrackInstrumentSource,
    #[serde(default)]
    pub midi: InstrumentMidiSettings,
}

/// MIDI note transformation and playability settings for one Instrument.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentMidiSettings {
    pub transpose_semitones: i16,
    pub playable_notes: Option<PlayableNotes>,
    pub keyswitches: Vec<InstrumentKeyswitch>,
}

/// Notes an Instrument can play, either as an inclusive range or an explicit set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", deny_unknown_fields, rename_all = "camelCase")]
#[ts(rename_all_fields = "camelCase")]
pub enum PlayableNotes {
    Range { start: u8, end: u8 },
    Set { notes: Vec<u8> },
}

/// A MIDI note reserved for a named Instrument articulation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentKeyswitch {
    pub note: u8,
    pub name: String,
}

/// A note that cannot be represented as a valid MIDI note or is outside the configured range.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentMidiWarning {
    pub clip_id: String,
    pub note_id: String,
    pub source_note: u8,
    pub output_note: Option<u8>,
    pub reason: InstrumentMidiWarningReason,
}

/// Why a MIDI note does not match its Instrument settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InstrumentMidiWarningReason {
    OutsideMidiRange,
    OutsidePlayableRange,
}

impl InstrumentMidiSettings {
    /// Validates and trims setting labels before they are persisted.
    pub fn validate_and_normalize(&mut self) -> Result<(), String> {
        self.validate()?;
        for keyswitch in &mut self.keyswitches {
            keyswitch.name = keyswitch.name.trim().to_owned();
        }
        Ok(())
    }

    /// Validates settings that can be persisted on an Instrument.
    pub fn validate(&self) -> Result<(), String> {
        if !(-127..=127).contains(&self.transpose_semitones) {
            return Err("instrument transpose must be between -127 and 127 semitones".into());
        }
        match &self.playable_notes {
            Some(PlayableNotes::Range { start, end }) if *start > 127 || *end > 127 => {
                return Err("playable note range must be within MIDI notes 0..=127".into());
            }
            Some(PlayableNotes::Range { start, end }) if start > end => {
                return Err("playable note range start must not exceed its end".into());
            }
            Some(PlayableNotes::Set { notes }) if notes.iter().any(|note| *note > 127) => {
                return Err("playable note set must contain MIDI notes 0..=127".into());
            }
            Some(PlayableNotes::Set { notes }) if notes.is_empty() => {
                return Err("playable note set must not be empty".into());
            }
            Some(PlayableNotes::Set { notes }) => {
                let unique = notes
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>();
                if unique.len() != notes.len() {
                    return Err("playable note set must not contain duplicates".into());
                }
            }
            _ => {}
        }
        let mut keyswitch_notes = std::collections::BTreeSet::new();
        for keyswitch in &self.keyswitches {
            if keyswitch.note > 127 {
                return Err("keyswitch notes must be within MIDI notes 0..=127".into());
            }
            if keyswitch.name.trim().is_empty() {
                return Err("keyswitch names must not be empty".into());
            }
            if !keyswitch_notes.insert(keyswitch.note) {
                return Err("keyswitch note numbers must be unique".into());
            }
        }
        Ok(())
    }

    /// Maps a source note to the note sent to the Instrument.
    pub fn output_note(&self, source_note: u8) -> Option<u8> {
        if self
            .keyswitches
            .iter()
            .any(|keyswitch| keyswitch.note == source_note)
        {
            return Some(source_note);
        }
        let note = i16::from(source_note) + self.transpose_semitones;
        u8::try_from(note).ok().filter(|note| *note <= 127)
    }

    /// Returns whether the supplied Instrument note is playable.
    pub fn is_playable(&self, output_note: u8) -> bool {
        match &self.playable_notes {
            None => true,
            Some(PlayableNotes::Range { start, end }) => (*start..=*end).contains(&output_note),
            Some(PlayableNotes::Set { notes }) => notes.contains(&output_note),
        }
    }

    /// Reports notes in the selected Track that cannot be played as configured.
    pub fn warnings(
        &self,
        track_id: &str,
        clips: &[super::MidiClip],
    ) -> Vec<InstrumentMidiWarning> {
        let mut warnings = Vec::new();
        for clip in clips.iter().filter(|clip| clip.track_id == track_id) {
            for note in &clip.notes {
                let output_note = self.output_note(note.note);
                let reason = match output_note {
                    None => Some(InstrumentMidiWarningReason::OutsideMidiRange),
                    Some(output_note)
                        if !self.keyswitches.iter().any(|key| key.note == note.note)
                            && !self.is_playable(output_note) =>
                    {
                        Some(InstrumentMidiWarningReason::OutsidePlayableRange)
                    }
                    Some(_) => None,
                };
                if let Some(reason) = reason {
                    warnings.push(InstrumentMidiWarning {
                        clip_id: clip.id.clone(),
                        note_id: note.id.clone(),
                        source_note: note.note,
                        output_note,
                        reason,
                    });
                }
            }
        }
        warnings
    }
}

/// The implementation source of a Track instrument.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", deny_unknown_fields, rename_all = "camelCase")]
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
    Vst3(Vst3Plugin),
}

/// Resource origin for an internal instrument definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", deny_unknown_fields, rename_all = "camelCase")]
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
            source: TrackInstrumentSource::Vst3(Vst3Plugin::new(path)),
            midi: InstrumentMidiSettings::default(),
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
            midi: InstrumentMidiSettings::default(),
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
            midi: InstrumentMidiSettings::default(),
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

    /// Returns the VST3 plugin when this is an external instrument.
    pub fn as_vst3(&self) -> Option<&Vst3Plugin> {
        match &self.source {
            TrackInstrumentSource::Vst3(plugin) => Some(plugin),
            TrackInstrumentSource::Internal { .. } => None,
        }
    }

    /// Returns the mutable VST3 plugin when this is an external instrument.
    pub fn as_vst3_mut(&mut self) -> Option<&mut Vst3Plugin> {
        match &mut self.source {
            TrackInstrumentSource::Vst3(plugin) => Some(plugin),
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
            TrackInstrumentSource::Vst3(_) => None,
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
            TrackInstrumentSource::Vst3(_) => None,
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

/// Validates and normalizes one instrument assignment.
pub(crate) fn validate_and_normalize(instrument: &mut TrackInstrument) -> Result<(), String> {
    if instrument.id.trim().is_empty() || instrument.name.trim().is_empty() {
        return Err("Track instruments require non-empty ids and names.".into());
    }
    instrument.id = instrument.id.trim().to_owned();
    instrument.name = instrument.name.trim().to_owned();
    instrument.midi.validate_and_normalize()?;

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
        TrackInstrumentSource::Vst3(plugin) => plugin.validate_and_normalize()?,
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
    fn tagged_sources_reject_unknown_keys() {
        // Arrange
        let mut source = serde_json::to_value(&built_in().source).unwrap();
        let mut resource = source["resource"].clone();
        source["presetId"] = "01-clean-sub-bass".into();
        resource["definitionJson"] = "{}".into();

        // Act
        let source_error = serde_json::from_value::<TrackInstrumentSource>(source).unwrap_err();
        let resource_error =
            serde_json::from_value::<InternalInstrumentResource>(resource).unwrap_err();

        // Assert
        assert!(
            source_error
                .to_string()
                .contains("unknown field `presetId`")
        );
        assert!(
            resource_error
                .to_string()
                .contains("unknown field `definitionJson`")
        );
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
    fn missing_instrument_midi_settings_load_as_defaults() {
        let mut encoded = serde_json::to_value(built_in()).unwrap();
        encoded.as_object_mut().unwrap().remove("midi");

        let decoded = serde_json::from_value::<TrackInstrument>(encoded).unwrap();

        assert_eq!(decoded.midi, InstrumentMidiSettings::default());
    }

    #[test]
    fn midi_mapping_preserves_keyswitches_and_reports_unplayable_notes() {
        let settings = InstrumentMidiSettings {
            transpose_semitones: 12,
            playable_notes: Some(PlayableNotes::Range { start: 64, end: 80 }),
            keyswitches: vec![InstrumentKeyswitch {
                note: 24,
                name: "Sustain".into(),
            }],
        };
        let clip = super::super::MidiClip {
            id: "clip-1".into(),
            name: "Clip".into(),
            track_id: "track-1".into(),
            asset_id: None,
            start_tick: crate::TimelineTick(0),
            duration_ticks: 960,
            notes: vec![
                super::super::MidiNote {
                    id: "note-low".into(),
                    note: 50,
                    start_tick: crate::TimelineTick(0),
                    duration_ticks: 120,
                    velocity: 100,
                    channel: 1,
                },
                super::super::MidiNote {
                    id: "note-high".into(),
                    note: 120,
                    start_tick: crate::TimelineTick(120),
                    duration_ticks: 120,
                    velocity: 100,
                    channel: 1,
                },
                super::super::MidiNote {
                    id: "keyswitch".into(),
                    note: 24,
                    start_tick: crate::TimelineTick(240),
                    duration_ticks: 120,
                    velocity: 100,
                    channel: 1,
                },
            ],
            events: Vec::new(),
            instrument_control_events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        };

        assert_eq!(settings.output_note(50), Some(62));
        assert_eq!(settings.output_note(24), Some(24));
        assert_eq!(settings.output_note(120), None);
        let warnings = settings.warnings("track-1", &[clip]);
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].note_id, "note-low");
        assert_eq!(warnings[0].output_note, Some(62));
        assert_eq!(
            warnings[0].reason,
            InstrumentMidiWarningReason::OutsidePlayableRange
        );
        assert_eq!(warnings[1].note_id, "note-high");
        assert_eq!(warnings[1].output_note, None);
        assert_eq!(
            warnings[1].reason,
            InstrumentMidiWarningReason::OutsideMidiRange
        );
    }

    #[test]
    fn each_instrument_kind_keeps_the_identities_that_load_it() {
        let built_in = built_in();
        let instrument_id = format!("user:{}", Uuid::now_v7());
        let snapshot_id = Uuid::now_v7().to_string();
        let user_snapshot = TrackInstrument::user_snapshot(
            "device:instrument".into(),
            "User Piano".into(),
            instrument_id.clone(),
            snapshot_id.clone(),
            r#"{"schemaVersion":1}"#.into(),
        )
        .unwrap();
        let vst3 = TrackInstrument::vst3(
            "device:instrument".into(),
            "Synth".into(),
            "Synth.vst3".into(),
        )
        .unwrap();

        for instrument in [&built_in, &user_snapshot, &vst3] {
            let encoded = serde_json::to_value(instrument).unwrap();
            assert_eq!(
                serde_json::from_value::<TrackInstrument>(encoded).unwrap(),
                *instrument
            );
        }

        assert_eq!(built_in.built_in_preset_id(), Some("01-clean-sub-bass"));
        assert_eq!(
            user_snapshot.user_snapshot_ids(),
            Some((instrument_id.as_str(), snapshot_id.as_str()))
        );
        assert_eq!(
            serde_json::to_value(&vst3.source).unwrap(),
            serde_json::json!({
                "type": "vst3",
                "path": "Synth.vst3",
                "parameterValues": [],
                "disabledPlaceholder": false
            })
        );
    }
}
