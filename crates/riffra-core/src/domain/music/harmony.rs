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
    fn resolves_the_riffra_chord_acceptance_corpus() {
        struct AcceptedChord {
            symbol: &'static str,
            name: &'static str,
            root: &'static str,
            bass: Option<&'static str>,
            tones: &'static [&'static str],
        }

        let cases = [
            AcceptedChord {
                symbol: "C",
                name: "C",
                root: "C",
                bass: None,
                tones: &["C", "E", "G"],
            },
            AcceptedChord {
                symbol: "Cm",
                name: "Cm",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G"],
            },
            AcceptedChord {
                symbol: "C5",
                name: "C5",
                root: "C",
                bass: None,
                tones: &["C", "G"],
            },
            AcceptedChord {
                symbol: "Csus2",
                name: "Cadd9omt3",
                root: "C",
                bass: None,
                tones: &["C", "D", "G"],
            },
            AcceptedChord {
                symbol: "Csus4",
                name: "Csus",
                root: "C",
                bass: None,
                tones: &["C", "F", "G"],
            },
            AcceptedChord {
                symbol: "Caug",
                name: "C+",
                root: "C",
                bass: None,
                tones: &["C", "E", "G#"],
            },
            AcceptedChord {
                symbol: "C+",
                name: "C+",
                root: "C",
                bass: None,
                tones: &["C", "E", "G#"],
            },
            AcceptedChord {
                symbol: "Cdim",
                name: "Cdim",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb"],
            },
            AcceptedChord {
                symbol: "C°",
                name: "Cdim",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb"],
            },
            AcceptedChord {
                symbol: "C6",
                name: "C6",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "A"],
            },
            AcceptedChord {
                symbol: "Cm6",
                name: "Cm6",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G", "A"],
            },
            AcceptedChord {
                symbol: "Cmaj7",
                name: "Cmaj7",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "B"],
            },
            AcceptedChord {
                symbol: "C7",
                name: "C7",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "Bb"],
            },
            AcceptedChord {
                symbol: "Cm7",
                name: "Cm7",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G", "Bb"],
            },
            AcceptedChord {
                symbol: "CmMaj7",
                name: "Cmmaj7",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G", "B"],
            },
            AcceptedChord {
                symbol: "Cdim7",
                name: "Cdim7",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb", "Bbb"],
            },
            AcceptedChord {
                symbol: "C°7",
                name: "Cdim7",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb", "Bbb"],
            },
            AcceptedChord {
                symbol: "Cm7b5",
                name: "Cm7b5",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb", "Bb"],
            },
            AcceptedChord {
                symbol: "Cø7",
                name: "Cm7b5",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb", "Bb"],
            },
            AcceptedChord {
                symbol: "C9",
                name: "C9",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "Bb", "D"],
            },
            AcceptedChord {
                symbol: "Cmaj9",
                name: "Cmaj9",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "B", "D"],
            },
            AcceptedChord {
                symbol: "Cm9",
                name: "Cm9",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G", "Bb", "D"],
            },
            AcceptedChord {
                symbol: "C11",
                name: "C9sus",
                root: "C",
                bass: None,
                tones: &["C", "F", "G", "Bb", "D"],
            },
            AcceptedChord {
                symbol: "Cmaj11",
                name: "Cmaj9sus",
                root: "C",
                bass: None,
                tones: &["C", "F", "G", "B", "D"],
            },
            AcceptedChord {
                symbol: "Cm11",
                name: "Cm11",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G", "Bb", "D", "F"],
            },
            AcceptedChord {
                symbol: "C13",
                name: "C13",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "Bb", "D", "A"],
            },
            AcceptedChord {
                symbol: "Cmaj13",
                name: "C69addmaj7",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "B", "D", "A"],
            },
            AcceptedChord {
                symbol: "Cm13",
                name: "Cm13",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G", "Bb", "D", "F", "A"],
            },
            AcceptedChord {
                symbol: "Cadd9",
                name: "Cadd9",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "D"],
            },
            AcceptedChord {
                symbol: "Cadd11",
                name: "Csusadd3",
                root: "C",
                bass: None,
                tones: &["C", "E", "F", "G"],
            },
            AcceptedChord {
                symbol: "C7omit5",
                name: "C7omt5",
                root: "C",
                bass: None,
                tones: &["C", "E", "Bb"],
            },
            AcceptedChord {
                symbol: "C7b5",
                name: "C7b5",
                root: "C",
                bass: None,
                tones: &["C", "E", "Gb", "Bb"],
            },
            AcceptedChord {
                symbol: "C7#5",
                name: "C7#5",
                root: "C",
                bass: None,
                tones: &["C", "E", "G#", "Bb"],
            },
            AcceptedChord {
                symbol: "C7b9",
                name: "C7b9",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "Bb", "Db"],
            },
            AcceptedChord {
                symbol: "C7#9",
                name: "C7#9",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "Bb", "D#"],
            },
            AcceptedChord {
                symbol: "C7#11",
                name: "C7#11",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "Bb", "F#"],
            },
            AcceptedChord {
                symbol: "C7b13",
                name: "C7b13",
                root: "C",
                bass: None,
                tones: &["C", "E", "Bb", "Ab"],
            },
            AcceptedChord {
                symbol: "C7(b9,#11)",
                name: "C7b9#11",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "Bb", "Db", "F#"],
            },
            AcceptedChord {
                symbol: "G7(b9,#9,b13)",
                name: "G7b9#9b13",
                root: "G",
                bass: None,
                tones: &["G", "B", "F", "Ab", "A#", "Eb"],
            },
            AcceptedChord {
                symbol: "C/E",
                name: "C/E",
                root: "C",
                bass: Some("E"),
                tones: &["C", "E", "G"],
            },
            AcceptedChord {
                symbol: "C/Bb",
                name: "C/Bb",
                root: "C",
                bass: Some("Bb"),
                tones: &["C", "E", "G"],
            },
            AcceptedChord {
                symbol: "Abaug/Bb",
                name: "Ab+/Bb",
                root: "Ab",
                bass: Some("Bb"),
                tones: &["Ab", "C", "E"],
            },
            AcceptedChord {
                symbol: "G7(b9,#11)/F",
                name: "G7b9#11/F",
                root: "G",
                bass: Some("F"),
                tones: &["G", "B", "D", "F", "Ab", "C#"],
            },
            AcceptedChord {
                symbol: "F#m7",
                name: "F#m7",
                root: "F#",
                bass: None,
                tones: &["F#", "A", "C#", "E"],
            },
            AcceptedChord {
                symbol: "Gbmaj7",
                name: "Gbmaj7",
                root: "Gb",
                bass: None,
                tones: &["Gb", "Bb", "Db", "F"],
            },
            AcceptedChord {
                symbol: "Bb7",
                name: "Bb7",
                root: "Bb",
                bass: None,
                tones: &["Bb", "D", "F", "Ab"],
            },
            AcceptedChord {
                symbol: "Ebmaj9",
                name: "Ebmaj9",
                root: "Eb",
                bass: None,
                tones: &["Eb", "G", "Bb", "D", "F"],
            },
            AcceptedChord {
                symbol: "Cbmaj7",
                name: "Cbmaj7",
                root: "Cb",
                bass: None,
                tones: &["Cb", "Eb", "Gb", "Bb"],
            },
            AcceptedChord {
                symbol: "C#minomit5maj7add9#11",
                name: "C#dimaddmaj79omt5",
                root: "C#",
                bass: None,
                tones: &["C#", "E", "B#", "D#", "F##"],
            },
            AcceptedChord {
                symbol: "CM",
                name: "C",
                root: "C",
                bass: None,
                tones: &["C", "E", "G"],
            },
            AcceptedChord {
                symbol: "Cmaj",
                name: "C",
                root: "C",
                bass: None,
                tones: &["C", "E", "G"],
            },
            AcceptedChord {
                symbol: "CΔ",
                name: "Cmaj7",
                root: "C",
                bass: None,
                tones: &["C", "E", "G", "B"],
            },
            AcceptedChord {
                symbol: "Cmin",
                name: "Cm",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G"],
            },
            AcceptedChord {
                symbol: "C-",
                name: "Cm",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "G"],
            },
            AcceptedChord {
                symbol: "Cdim",
                name: "Cdim",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb"],
            },
            AcceptedChord {
                symbol: "C°",
                name: "Cdim",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb"],
            },
            AcceptedChord {
                symbol: "Cø",
                name: "Cm7b5",
                root: "C",
                bass: None,
                tones: &["C", "Eb", "Gb", "Bb"],
            },
            AcceptedChord {
                symbol: "Caug",
                name: "C+",
                root: "C",
                bass: None,
                tones: &["C", "E", "G#"],
            },
            AcceptedChord {
                symbol: "C+",
                name: "C+",
                root: "C",
                bass: None,
                tones: &["C", "E", "G#"],
            },
        ];

        for case in cases {
            let chord = HarmonyChord::resolve(case.symbol).unwrap();
            assert_eq!(chord.name, case.name);
            assert_eq!(chord.root.unwrap().to_string(), case.root);
            assert_eq!(
                chord.bass.map(|note| note.to_string()),
                case.bass.map(str::to_owned)
            );
            assert_eq!(
                chord
                    .tones
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                case.tones
            );
        }
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
