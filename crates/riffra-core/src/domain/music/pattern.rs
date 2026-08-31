//! Musical rhythm and phrase operation values.

use super::pitch::MusicalPitch;
use super::time::{MusicalDuration, MusicalOffset, MusicalPosition};
use crate::DomainError;
use serde::{Deserialize, Serialize};

/// A repeated set of rhythm steps expressed in whole-note coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhythmPattern {
    pub length: MusicalDuration,
    pub steps: Vec<RhythmStep>,
}

/// One onset and duration inside a [`RhythmPattern`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhythmStep {
    pub offset: MusicalOffset,
    pub duration: MusicalDuration,
    pub velocity: Option<u8>,
}

impl RhythmPattern {
    /// Creates a validated rhythm pattern and orders its steps by onset.
    ///
    /// # Errors
    ///
    /// Returns an error when the pattern has no steps, a non-positive length,
    /// an onset outside the pattern, duplicate onsets, or an invalid velocity.
    pub fn new(length: MusicalDuration, steps: Vec<RhythmStep>) -> Result<Self, DomainError> {
        let mut pattern = Self { length, steps };
        pattern.validate_and_normalize()?;
        Ok(pattern)
    }

    /// Validates and orders this pattern in place.
    ///
    /// # Errors
    ///
    /// Returns an error when a pattern invariant is violated.
    pub fn validate_and_normalize(&mut self) -> Result<(), DomainError> {
        let length = MusicalDuration::new(self.length.numerator, self.length.denominator)?;
        if self.steps.is_empty() {
            return Err(invalid_pattern("rhythm pattern requires at least one step"));
        }
        for step in &mut self.steps {
            step.offset = MusicalOffset::new(step.offset.numerator, step.offset.denominator)?;
            step.duration =
                MusicalDuration::new(step.duration.numerator, step.duration.denominator)?;
            if step.velocity.is_some_and(|velocity| velocity > 127) {
                return Err(invalid_pattern("rhythm velocity must be between 0 and 127"));
            }
            if !fraction_less(step.offset, length) {
                return Err(invalid_pattern(
                    "rhythm step offset must be less than pattern length",
                ));
            }
        }
        self.length = length;
        self.steps
            .sort_by(|left, right| compare_offsets(left.offset, right.offset));
        if self
            .steps
            .windows(2)
            .any(|steps| steps[0].offset == steps[1].offset)
        {
            return Err(invalid_pattern("rhythm steps cannot share an offset"));
        }
        Ok(())
    }
}

/// A note relative to the anchor of a phrase placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseNote {
    pub offset: MusicalOffset,
    pub duration: MusicalDuration,
    pub semitones: i16,
    pub velocity: Option<u8>,
}

/// A reusable relative-pitch phrase.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhrasePattern {
    pub length: MusicalDuration,
    pub notes: Vec<PhraseNote>,
}

impl PhrasePattern {
    /// Creates a validated phrase pattern.
    ///
    /// # Errors
    ///
    /// Returns an error when the pattern has no notes, a non-positive length,
    /// an offset outside the pattern, or an invalid velocity.
    pub fn new(length: MusicalDuration, notes: Vec<PhraseNote>) -> Result<Self, DomainError> {
        let mut pattern = Self { length, notes };
        pattern.validate_and_normalize()?;
        Ok(pattern)
    }

    /// Validates and orders this phrase pattern in place.
    ///
    /// # Errors
    ///
    /// Returns an error when a pattern invariant is violated.
    pub fn validate_and_normalize(&mut self) -> Result<(), DomainError> {
        let length = MusicalDuration::new(self.length.numerator, self.length.denominator)?;
        if self.notes.is_empty() {
            return Err(invalid_pattern("phrase pattern requires at least one note"));
        }
        for note in &mut self.notes {
            note.offset = MusicalOffset::new(note.offset.numerator, note.offset.denominator)?;
            note.duration =
                MusicalDuration::new(note.duration.numerator, note.duration.denominator)?;
            if note.velocity.is_some_and(|velocity| velocity > 127) {
                return Err(invalid_pattern("phrase velocity must be between 0 and 127"));
            }
            if !fraction_less(note.offset, length) {
                return Err(invalid_pattern(
                    "phrase note offset must be less than pattern length",
                ));
            }
        }
        self.length = length;
        self.notes
            .sort_by(|left, right| compare_offsets(left.offset, right.offset));
        Ok(())
    }
}

/// A phrase placement and its absolute anchor pitch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhrasePlacement {
    pub position: MusicalPosition,
    pub anchor: MusicalPitch,
    pub repeats: u32,
}

impl PhrasePlacement {
    /// Validates a placement's position and repeat count.
    ///
    /// # Errors
    ///
    /// Returns an error when the position is invalid or repeats is outside the
    /// supported range.
    pub fn validate(&self) -> Result<(), DomainError> {
        MusicalPosition::new(self.position.bar, self.position.beat, self.position.offset)?;
        if !(1..=256).contains(&self.repeats) {
            return Err(invalid_pattern("phrase repeats must be between 1 and 256"));
        }
        Ok(())
    }
}

fn fraction_less(offset: MusicalOffset, length: MusicalDuration) -> bool {
    u128::from(offset.numerator) * u128::from(length.denominator)
        < u128::from(length.numerator) * u128::from(offset.denominator)
}

fn compare_offsets(left: MusicalOffset, right: MusicalOffset) -> std::cmp::Ordering {
    (u128::from(left.numerator) * u128::from(right.denominator))
        .cmp(&(u128::from(right.numerator) * u128::from(left.denominator)))
}

fn invalid_pattern(message: impl Into<String>) -> DomainError {
    DomainError::InvalidMusicalValue(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rhythm_pattern_normalizes_order_and_accepts_arbitrary_lengths() {
        let pattern = RhythmPattern::new(
            "3/8".parse().unwrap(),
            vec![
                RhythmStep {
                    offset: "1/4".parse().unwrap(),
                    duration: "1/8".parse().unwrap(),
                    velocity: Some(112),
                },
                RhythmStep {
                    offset: "0/1".parse().unwrap(),
                    duration: "1/16".parse().unwrap(),
                    velocity: None,
                },
            ],
        )
        .unwrap();

        assert_eq!(pattern.steps[0].offset, "0/1".parse().unwrap());
        assert_eq!(pattern.steps[1].offset, "1/4".parse().unwrap());
    }

    #[test]
    fn pattern_offsets_cannot_repeat_or_reach_the_end() {
        let duplicate = RhythmPattern::new(
            "1/2".parse().unwrap(),
            vec![
                RhythmStep {
                    offset: "1/4".parse().unwrap(),
                    duration: "1/8".parse().unwrap(),
                    velocity: None,
                },
                RhythmStep {
                    offset: "2/8".parse().unwrap(),
                    duration: "1/8".parse().unwrap(),
                    velocity: None,
                },
            ],
        );
        assert!(duplicate.is_err());

        let at_end = RhythmPattern::new(
            "1/2".parse().unwrap(),
            vec![RhythmStep {
                offset: "1/2".parse().unwrap(),
                duration: "1/8".parse().unwrap(),
                velocity: None,
            }],
        );
        assert!(at_end.is_err());
    }

    #[test]
    fn phrase_placement_limits_repetition() {
        let placement = PhrasePlacement {
            position: "1:1".parse().unwrap(),
            anchor: "C4".parse().unwrap(),
            repeats: 257,
        };
        assert!(placement.validate().is_err());
    }
}
