//! Musical note names and pitches.

use crate::DomainError;
use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Write as _;
use std::str::FromStr;

/// A pitch name without an octave that retains its enharmonic spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ts_rs::TS)]
#[ts(type = "string")]
pub struct MusicalNoteName {
    letter: NoteLetter,
    accidental: Accidental,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoteLetter {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
}

impl NoteLetter {
    fn from_index(index: u8) -> Self {
        match index % 7 {
            0 => Self::C,
            1 => Self::D,
            2 => Self::E,
            3 => Self::F,
            4 => Self::G,
            5 => Self::A,
            6 => Self::B,
            _ => unreachable!(),
        }
    }

    fn index(self) -> u8 {
        match self {
            Self::C => 0,
            Self::D => 1,
            Self::E => 2,
            Self::F => 3,
            Self::G => 4,
            Self::A => 5,
            Self::B => 6,
        }
    }

    fn from_char(value: char) -> Option<Self> {
        match value.to_ascii_uppercase() {
            'A' => Some(Self::A),
            'B' => Some(Self::B),
            'C' => Some(Self::C),
            'D' => Some(Self::D),
            'E' => Some(Self::E),
            'F' => Some(Self::F),
            'G' => Some(Self::G),
            _ => None,
        }
    }

    fn semitone(self) -> i16 {
        match self {
            Self::A => 9,
            Self::B => 11,
            Self::C => 0,
            Self::D => 2,
            Self::E => 4,
            Self::F => 5,
            Self::G => 7,
        }
    }
}

impl fmt::Display for NoteLetter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::A => 'A',
            Self::B => 'B',
            Self::C => 'C',
            Self::D => 'D',
            Self::E => 'E',
            Self::F => 'F',
            Self::G => 'G',
        };
        formatter.write_char(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Accidental(i8);

impl Accidental {
    fn semitone(self) -> i16 {
        i16::from(self.0)
    }
}

impl fmt::Display for Accidental {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            0 => Ok(()),
            1 => formatter.write_char('#'),
            2 => formatter.write_str("##"),
            -1 => formatter.write_char('b'),
            -2 => formatter.write_str("bb"),
            _ => unreachable!("accidental is constrained to a double accidental"),
        }
    }
}

impl MusicalNoteName {
    /// Parses a note name without an octave.
    ///
    /// # Errors
    ///
    /// Returns an error when value is not a natural note with at most a
    /// double sharp or double flat.
    pub fn new(value: &str) -> Result<Self, DomainError> {
        let (note, remainder) = parse_note_prefix(value.trim())?;
        if !remainder.is_empty() {
            return Err(invalid_value("note name must not contain an octave"));
        }
        Ok(note)
    }

    /// Returns the pitch class represented by this note name.
    pub fn pitch_class(self) -> u8 {
        let value = (self.letter.semitone() + self.accidental.semitone()).rem_euclid(12);
        u8::try_from(value).expect("pitch class is always within one octave")
    }

    /// Adds an octave to this note name.
    ///
    /// # Errors
    ///
    /// Returns an error when the resulting pitch is outside the MIDI range.
    pub fn with_octave(self, octave: i8) -> Result<MusicalPitch, DomainError> {
        let midi_pitch =
            (i16::from(octave) + 1) * 12 + self.letter.semitone() + self.accidental.semitone();
        if !(0..=127).contains(&midi_pitch) {
            return Err(invalid_value("pitch must be within the MIDI range"));
        }
        Ok(MusicalPitch { note: self, octave })
    }

    pub(crate) fn from_interval(
        root: Self,
        degree: u8,
        semitones: u8,
    ) -> Result<Self, DomainError> {
        let degree_steps = match degree {
            1 | 8 => 0,
            2 | 9 => 1,
            3 => 2,
            4 | 11 => 3,
            5 => 4,
            6 | 13 => 5,
            7 => 6,
            _ => return Err(invalid_harmony("unsupported chord interval degree")),
        };
        let letter = NoteLetter::from_index(root.letter.index() + degree_steps);
        let natural_pitch = letter.semitone();
        let target_pitch = (i16::from(root.pitch_class()) + i16::from(semitones)).rem_euclid(12);
        let accidental = (target_pitch - natural_pitch).rem_euclid(12);
        let accidental = if accidental > 6 {
            accidental - 12
        } else {
            accidental
        };
        if !(-2..=2).contains(&accidental) {
            return Err(invalid_harmony(
                "chord interval requires an unsupported accidental",
            ));
        }
        Ok(Self {
            letter,
            accidental: Accidental(i8::try_from(accidental).expect("accidental is bounded")),
        })
    }
}

impl fmt::Display for MusicalNoteName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}{}", self.letter, self.accidental)
    }
}

impl FromStr for MusicalNoteName {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for MusicalNoteName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MusicalNoteName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

/// A pitch name that retains its enharmonic spelling while exposing its MIDI value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MusicalPitch {
    note: MusicalNoteName,
    octave: i8,
}

impl MusicalPitch {
    /// Parses a pitch name and retains its enharmonic spelling.
    ///
    /// # Errors
    ///
    /// Returns an error when value is not a supported pitch name or falls
    /// outside the MIDI pitch range.
    pub fn new(value: &str) -> Result<Self, DomainError> {
        value.parse()
    }

    /// Returns the MIDI note number represented by this pitch.
    ///
    /// # Panics
    ///
    /// Panics only if the pitch's internal MIDI-range invariant is violated.
    pub fn midi_pitch(self) -> u8 {
        let midi_pitch = (i16::from(self.octave) + 1) * 12
            + self.note.letter.semitone()
            + self.note.accidental.semitone();
        u8::try_from(midi_pitch).expect("a musical pitch always has a valid MIDI value")
    }
}

impl fmt::Display for MusicalPitch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}{}", self.note, self.octave)
    }
}

impl FromStr for MusicalPitch {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (note, octave) = parse_note_prefix(value.trim())?;
        let octave = octave
            .parse::<i16>()
            .map_err(|_| invalid_value("pitch octave must be an integer"))?;
        note.with_octave(
            i8::try_from(octave).map_err(|_| invalid_value("pitch octave is too large"))?,
        )
    }
}

impl Serialize for MusicalPitch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MusicalPitch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

fn parse_note_prefix(value: &str) -> Result<(MusicalNoteName, &str), DomainError> {
    let note = value
        .chars()
        .next()
        .ok_or_else(|| invalid_value("note name must contain a letter"))?;
    let letter = NoteLetter::from_char(note)
        .ok_or_else(|| invalid_value("note name must be between A and G"))?;
    let mut end = note.len_utf8();
    let mut accidental_kind = None;
    let mut accidental_count = 0_u8;
    while let Some(character) = value[end..].chars().next() {
        let kind = match character {
            '#' | '♯' => Some(1_i8),
            'b' | '♭' => Some(-1_i8),
            '𝄪' => Some(2_i8),
            '𝄫' => Some(-2_i8),
            _ => None,
        };
        let Some(kind) = kind else {
            break;
        };
        if kind.abs() == 2 {
            if accidental_count != 0 || accidental_kind.is_some() {
                return Err(invalid_value(
                    "note name must use at most one double accidental",
                ));
            }
            accidental_count = 2;
            accidental_kind = Some(kind.signum());
        } else {
            if accidental_kind.is_some_and(|existing| existing != kind) {
                return Err(invalid_value("note name cannot mix sharps and flats"));
            }
            accidental_kind = Some(kind);
            accidental_count = accidental_count.saturating_add(1);
        }
        if accidental_count > 2 {
            return Err(invalid_value(
                "note name must use at most a double accidental",
            ));
        }
        end += character.len_utf8();
    }
    let sign = accidental_kind.unwrap_or(0);
    let accidental = sign * i8::try_from(accidental_count).expect("accidental count is bounded");
    Ok((
        MusicalNoteName {
            letter,
            accidental: Accidental(accidental),
        },
        &value[end..],
    ))
}

fn invalid_value(message: impl Into<String>) -> DomainError {
    DomainError::InvalidMusicalValue(message.into())
}

fn invalid_harmony(message: impl Into<String>) -> DomainError {
    DomainError::InvalidHarmony(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_enharmonics_share_the_same_midi_value() {
        let sharp = "C#4".parse::<MusicalPitch>().unwrap();
        let flat = "Db4".parse::<MusicalPitch>().unwrap();
        assert_eq!(sharp.midi_pitch(), flat.midi_pitch());
        assert_eq!(sharp.to_string(), "C#4");
        assert_eq!(flat.to_string(), "Db4");
        assert_eq!(serde_json::to_string(&flat).unwrap(), r#""Db4""#);
        assert_eq!("C-1".parse::<MusicalPitch>().unwrap().midi_pitch(), 0);
        assert_eq!("G9".parse::<MusicalPitch>().unwrap().midi_pitch(), 127);
    }

    #[test]
    fn note_names_preserve_double_accidentals_and_reject_invalid_spellings() {
        for value in ["C", "Db", "F##", "Bbb"] {
            let note = value.parse::<MusicalNoteName>().unwrap();
            assert_eq!(note.to_string(), value);
            assert_eq!(
                serde_json::from_value::<MusicalNoteName>(serde_json::json!(value)).unwrap(),
                note
            );
        }
        assert_eq!("C𝄪".parse::<MusicalNoteName>().unwrap().to_string(), "C##");
        assert_eq!("C4".parse::<MusicalPitch>().unwrap().midi_pitch(), 60);
        assert_eq!("F##3".parse::<MusicalPitch>().unwrap().midi_pitch(), 55);
        assert_eq!("Bbb4".parse::<MusicalPitch>().unwrap().midi_pitch(), 69);
        assert!(("C###").parse::<MusicalNoteName>().is_err());
        assert!("C#b".parse::<MusicalNoteName>().is_err());
        assert!("C𝄪#".parse::<MusicalNoteName>().is_err());
        assert!("C4".parse::<MusicalNoteName>().is_err());
        assert!("H4".parse::<MusicalPitch>().is_err());
        assert!("C10".parse::<MusicalPitch>().is_err());
    }
}
