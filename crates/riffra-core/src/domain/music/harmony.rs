//! Canonical harmony values and chord resolution.

use super::pitch::MusicalNoteName;
use crate::DomainError;
use chordparser::parsing::Parser;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A resolved chord symbol or explicitly supplied sonority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyChord {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub root: Option<MusicalNoteName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub bass: Option<MusicalNoteName>,
    pub tones: Vec<MusicalNoteName>,
}

impl HarmonyChord {
    /// Resolves a human-readable chord symbol into Riffra's canonical chord.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidHarmony`] when the symbol is empty or
    /// cannot be resolved by the supported chord-symbol grammar.
    pub fn resolve(symbol: &str) -> Result<Self, DomainError> {
        let symbol = symbol.trim();
        if symbol.is_empty() {
            return Err(invalid_harmony("chord symbol must not be empty"));
        }
        let mut parser = Parser::new();
        let chord = parser
            .parse(symbol)
            .map_err(|_| invalid_harmony("chord symbol could not be resolved"))?;
        let root = MusicalNoteName::new(&chord.root.to_string())?;
        let tones = chord
            .intervals
            .iter()
            .map(|interval| {
                MusicalNoteName::from_interval(root, interval.to_degree().numeric(), interval.st())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bass = chord
            .bass
            .as_ref()
            .map(|bass| MusicalNoteName::new(&bass.to_string()))
            .transpose()?;
        let name = if chord.normalized.trim().is_empty() {
            symbol.to_owned()
        } else {
            normalize_chord_name(chord.normalized, bass)
        };
        let chord = Self {
            name,
            root: Some(root),
            bass,
            tones,
        };
        chord.validate()?;
        Ok(chord)
    }

    /// Creates a canonical chord from octave-free explicit tones.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidHarmony`] when no tones are provided or
    /// when the optional label is empty.
    pub fn from_explicit(
        tones: Vec<MusicalNoteName>,
        root: Option<MusicalNoteName>,
        bass: Option<MusicalNoteName>,
        label: Option<String>,
    ) -> Result<Self, DomainError> {
        if tones.is_empty() {
            return Err(invalid_harmony(
                "explicit harmony requires at least one tone",
            ));
        }
        let name = match label {
            Some(label) if !label.trim().is_empty() => label.trim().to_owned(),
            Some(_) => return Err(invalid_harmony("harmony label must not be empty")),
            None => tones
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        };
        let chord = Self {
            name,
            root,
            bass,
            tones,
        };
        chord.validate()?;
        Ok(chord)
    }

    /// Validates the canonical chord invariants.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidHarmony`] when the name or tone list is
    /// empty.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.name.trim().is_empty() {
            return Err(invalid_harmony("harmony chord name must not be empty"));
        }
        if self.tones.is_empty() {
            return Err(invalid_harmony("harmony chord requires at least one tone"));
        }
        Ok(())
    }
}

fn normalize_chord_name(value: String, bass: Option<MusicalNoteName>) -> String {
    let value = value
        .replace("major", "maj")
        .replace("Maj", "maj")
        .replace("Ma", "maj")
        .replace("min", "m")
        .replace("mi", "m")
        .chars()
        .filter(|character| !matches!(character, '(' | ')' | ',' | ' '))
        .collect::<String>();
    let base = value.split('/').next().unwrap_or_default().to_owned();
    match bass {
        Some(bass) => format!("{base}/{bass}"),
        None => value,
    }
}

/// A canonical harmony event on the arrangement timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyEvent {
    pub id: String,
    #[ts(type = "number")]
    pub start_tick: crate::TimelineTick,
    #[ts(type = "number")]
    pub end_tick: crate::TimelineTick,
    pub chord: HarmonyChord,
}

impl HarmonyEvent {
    /// Validates an event's identity, range, and chord.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidHarmony`] when an invariant is violated.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.id.trim().is_empty() {
            return Err(invalid_harmony("harmony event id must not be empty"));
        }
        if self.end_tick <= self.start_tick {
            return Err(invalid_harmony("harmony event end must be after its start"));
        }
        self.chord.validate()
    }
}

fn invalid_harmony(message: impl Into<String>) -> DomainError {
    DomainError::InvalidHarmony(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_common_and_enharmonic_chords_with_canonical_spelling() {
        let cases = [
            ("Dm9", "Dm9", "D", None, vec!["D", "F", "A", "C", "E"]),
            (
                "G7(b9,#11)/F",
                "G7b9#11/F",
                "G",
                Some("F"),
                vec!["G", "B", "D", "F", "Ab", "C#"],
            ),
            ("Cdim7", "Cdim7", "C", None, vec!["C", "Eb", "Gb", "Bbb"]),
            ("Cm7b5", "Cm7b5", "C", None, vec!["C", "Eb", "Gb", "Bb"]),
            ("Cø", "Cm7b5", "C", None, vec!["C", "Eb", "Gb", "Bb"]),
            ("Cmaj9", "Cmaj9", "C", None, vec!["C", "E", "G", "B", "D"]),
            ("Abaug/Bb", "Ab+/Bb", "Ab", Some("Bb"), vec!["Ab", "C", "E"]),
        ];

        for (symbol, name, root, bass, tones) in cases {
            let chord = HarmonyChord::resolve(symbol).unwrap();
            assert_eq!(chord.name, name);
            assert_eq!(chord.root.unwrap().to_string(), root);
            assert_eq!(
                chord.bass.map(|note| note.to_string()),
                bass.map(str::to_owned)
            );
            assert_eq!(
                chord
                    .tones
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                tones
            );
        }

        let enharmonic = HarmonyChord::resolve("C#minomit5maj7add9#11").unwrap();
        assert_eq!(
            enharmonic
                .tones
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["C#", "E", "B#", "D#", "F##"]
        );
    }

    #[test]
    fn explicit_tones_support_rootless_and_non_chord_bass_sonorities() {
        let chord = HarmonyChord::from_explicit(
            vec![
                "Bb".parse().unwrap(),
                "C".parse().unwrap(),
                "E".parse().unwrap(),
            ],
            None,
            Some("F".parse().unwrap()),
            Some("custom sonority".into()),
        )
        .unwrap();

        assert_eq!(chord.name, "custom sonority");
        assert_eq!(chord.root, None);
        assert_eq!(chord.bass.unwrap().to_string(), "F");
        assert!(HarmonyChord::from_explicit(Vec::new(), None, None, None).is_err());
        let doubled =
            HarmonyChord::from_explicit(vec!["Bbb".parse().unwrap()], None, None, None).unwrap();
        assert_eq!(doubled.name, "Bbb");
    }

    #[test]
    fn harmony_event_requires_a_positive_range() {
        let event = HarmonyEvent {
            id: "harmony:test".into(),
            start_tick: crate::TimelineTick(10),
            end_tick: crate::TimelineTick(10),
            chord: HarmonyChord::resolve("C").unwrap(),
        };
        assert!(event.validate().is_err());
    }
}
