//! Musical time values and their conversion to the canonical timeline.

use crate::DomainError;
use crate::domain::timeline::{ProjectTimebase, TimelineTick};
use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// A non-negative normalized fraction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MusicalFraction {
    pub numerator: u32,
    pub denominator: u32,
}

impl MusicalFraction {
    /// Creates and reduces a non-negative fraction.
    ///
    /// # Errors
    ///
    /// Returns an error when denominator is zero.
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, DomainError> {
        if denominator == 0 {
            return Err(invalid_value("fraction denominator must be positive"));
        }
        let divisor = gcd(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }
}

impl Default for MusicalFraction {
    fn default() -> Self {
        Self {
            numerator: 0,
            denominator: 1,
        }
    }
}

impl Serialize for MusicalFraction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct FractionFields {
            numerator: u32,
            denominator: u32,
        }

        let fraction = MusicalFraction::new(self.numerator, self.denominator)
            .map_err(serde::ser::Error::custom)?;
        FractionFields {
            numerator: fraction.numerator,
            denominator: fraction.denominator,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MusicalFraction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct FractionFields {
            numerator: u32,
            denominator: u32,
        }

        let fields = FractionFields::deserialize(deserializer)?;
        MusicalFraction::new(fields.numerator, fields.denominator).map_err(D::Error::custom)
    }
}

/// A one-origin bar and beat position with an optional beat-relative offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MusicalPosition {
    pub bar: u32,
    pub beat: u32,
    pub offset: MusicalFraction,
}

impl MusicalPosition {
    /// Creates a position and validates its beat-relative offset.
    ///
    /// # Errors
    ///
    /// Returns an error when the bar or beat is zero, or when the offset is
    /// not a proper fraction.
    pub fn new(bar: u32, beat: u32, offset: MusicalFraction) -> Result<Self, DomainError> {
        if bar == 0 {
            return Err(invalid_value("bar must be one or greater"));
        }
        if beat == 0 {
            return Err(invalid_value("beat must be one or greater"));
        }
        if offset.denominator == 0 {
            return Err(invalid_value(
                "position offset denominator must be positive",
            ));
        }
        if offset.numerator >= offset.denominator {
            return Err(invalid_value("position offset must be less than one beat"));
        }
        let offset = MusicalFraction::new(offset.numerator, offset.denominator)?;
        Ok(Self { bar, beat, offset })
    }
}

impl fmt::Display for MusicalPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.bar, self.beat)?;
        if self.offset.numerator != 0 {
            write!(
                formatter,
                "+{}/{}",
                self.offset.numerator, self.offset.denominator
            )?;
        }
        Ok(())
    }
}

impl FromStr for MusicalPosition {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let (bar, beat_with_offset) = value
            .split_once(':')
            .ok_or_else(|| invalid_value("position must use bar:beat notation"))?;
        let (beat, offset) = match beat_with_offset.split_once('+') {
            Some((beat, fraction)) => (beat, parse_fraction(fraction, "position offset")?),
            None => (
                beat_with_offset,
                MusicalFraction::new(0, 1).expect("constant fraction is valid"),
            ),
        };
        let bar = parse_u32(bar, "bar")?;
        let beat = parse_u32(beat, "beat")?;
        MusicalPosition::new(bar, beat, offset)
    }
}

impl Serialize for MusicalPosition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MusicalPosition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

/// A positive duration expressed as a fraction of a whole note.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MusicalDuration {
    pub numerator: u32,
    pub denominator: u32,
}

impl MusicalDuration {
    /// Creates and reduces a positive musical duration.
    ///
    /// # Errors
    ///
    /// Returns an error when the numerator is zero or the denominator is zero.
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, DomainError> {
        if numerator == 0 {
            return Err(invalid_value("duration numerator must be positive"));
        }
        let fraction = MusicalFraction::new(numerator, denominator)?;
        Ok(Self {
            numerator: fraction.numerator,
            denominator: fraction.denominator,
        })
    }
}

impl fmt::Display for MusicalDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.numerator, self.denominator)
    }
}

impl FromStr for MusicalDuration {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let fraction = parse_fraction(value.trim(), "duration")?;
        MusicalDuration::new(fraction.numerator, fraction.denominator)
    }
}

impl Serialize for MusicalDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MusicalDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

/// A non-negative offset expressed as a fraction of a whole note.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MusicalOffset {
    pub numerator: u32,
    pub denominator: u32,
}

impl MusicalOffset {
    /// Creates and reduces a non-negative musical offset.
    ///
    /// # Errors
    ///
    /// Returns an error when the denominator is zero.
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, DomainError> {
        let fraction = MusicalFraction::new(numerator, denominator)?;
        Ok(Self {
            numerator: fraction.numerator,
            denominator: fraction.denominator,
        })
    }
}

impl fmt::Display for MusicalOffset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.numerator, self.denominator)
    }
}

impl FromStr for MusicalOffset {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let fraction = parse_fraction(value.trim(), "offset")?;
        Self::new(fraction.numerator, fraction.denominator)
    }
}

impl Serialize for MusicalOffset {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MusicalOffset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

impl ProjectTimebase {
    /// Converts a project-wide bar/beat position to an absolute timeline tick.
    ///
    /// # Errors
    ///
    /// Returns an error when the timebase or position is invalid, or when the
    /// resulting tick cannot be represented.
    pub fn musical_position_to_tick(
        self,
        position: MusicalPosition,
    ) -> Result<TimelineTick, DomainError> {
        let position = MusicalPosition::new(position.bar, position.beat, position.offset)?;
        let ticks_per_beat = self.ticks_per_notated_beat()?;
        if position.beat > u32::from(self.time_signature_numerator) {
            return Err(invalid_value("position beat is outside the time signature"));
        }
        let beat_index = u128::from(position.bar - 1)
            .checked_mul(u128::from(self.time_signature_numerator))
            .and_then(|value| value.checked_add(u128::from(position.beat - 1)))
            .ok_or_else(|| invalid_value("position is too large"))?;
        let base_ticks = beat_index
            .checked_mul(u128::from(ticks_per_beat))
            .ok_or_else(|| invalid_value("position is too large"))?;
        let offset_ticks = round_fraction(
            u128::from(ticks_per_beat) * u128::from(position.offset.numerator),
            u128::from(position.offset.denominator),
        );
        let ticks = base_ticks
            .checked_add(offset_ticks)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| invalid_value("position is too large"))?;
        Ok(TimelineTick(ticks))
    }

    /// Converts an absolute timeline tick to the nearest exact musical position.
    pub fn tick_to_musical_position(self, tick: TimelineTick) -> MusicalPosition {
        let ticks_per_beat = self.ticks_per_notated_beat().unwrap_or(1);
        let total_beats = tick.0 / ticks_per_beat;
        let beat_offset = tick.0 % ticks_per_beat;
        let beats_per_bar = u64::from(self.time_signature_numerator.max(1));
        let bar = total_beats / beats_per_bar;
        let beat = total_beats % beats_per_bar;
        let bar = u32::try_from(bar.saturating_add(1)).unwrap_or(u32::MAX);
        let beat = u32::try_from(beat.saturating_add(1)).unwrap_or(u32::MAX);
        let numerator = u32::try_from(beat_offset).unwrap_or(u32::MAX);
        let denominator = u32::try_from(ticks_per_beat).unwrap_or(u32::MAX);
        MusicalPosition::new(
            bar,
            beat,
            MusicalFraction::new(numerator, denominator)
                .expect("a valid timebase produces a valid musical fraction"),
        )
        .expect("a valid timebase produces a valid musical position")
    }

    /// Converts a whole-note fraction to the nearest positive timeline duration.
    ///
    /// # Errors
    ///
    /// Returns an error when the timebase or duration is invalid, when the
    /// result rounds to zero, or when it cannot be represented.
    pub fn musical_duration_to_ticks(self, duration: MusicalDuration) -> Result<u64, DomainError> {
        let duration = MusicalDuration::new(duration.numerator, duration.denominator)?;
        let whole_note_ticks = u128::from(self.ppq)
            .checked_mul(4)
            .ok_or_else(|| invalid_value("duration is too large"))?;
        let ticks = round_fraction(
            whole_note_ticks * u128::from(duration.numerator),
            u128::from(duration.denominator),
        );
        if ticks == 0 {
            return Err(invalid_value("duration resolves to zero ticks"));
        }
        u64::try_from(ticks).map_err(|_| invalid_value("duration is too large"))
    }

    /// Converts a non-negative whole-note offset to the nearest timeline tick.
    ///
    /// # Errors
    ///
    /// Returns an error when the timebase or offset is invalid, or when the
    /// result cannot be represented.
    pub fn musical_offset_to_ticks(self, offset: MusicalOffset) -> Result<u64, DomainError> {
        let offset = MusicalOffset::new(offset.numerator, offset.denominator)?;
        let whole_note_ticks = u128::from(self.ppq)
            .checked_mul(4)
            .ok_or_else(|| invalid_value("offset is too large"))?;
        let ticks = round_fraction(
            whole_note_ticks * u128::from(offset.numerator),
            u128::from(offset.denominator),
        );
        u64::try_from(ticks).map_err(|_| invalid_value("offset is too large"))
    }

    fn ticks_per_notated_beat(self) -> Result<u64, DomainError> {
        if self.ppq == 0
            || self.time_signature_numerator == 0
            || !matches!(self.time_signature_denominator, 1 | 2 | 4 | 8 | 16 | 32)
        {
            return Err(invalid_value("project timebase is invalid"));
        }
        let quarter_ticks = u64::from(self.ppq)
            .checked_mul(4)
            .ok_or_else(|| invalid_value("project timebase is too large"))?;
        let denominator = u64::from(self.time_signature_denominator);
        if !quarter_ticks.is_multiple_of(denominator) {
            return Err(invalid_value(
                "time signature denominator cannot be represented by the project PPQ",
            ));
        }
        Ok(quarter_ticks / denominator)
    }
}

fn parse_fraction(value: &str, label: &str) -> Result<MusicalFraction, DomainError> {
    let (numerator, denominator) = value
        .split_once('/')
        .ok_or_else(|| invalid_value(format!("{label} must use numerator/denominator notation")))?;
    MusicalFraction::new(
        parse_u32(numerator, "fraction numerator")?,
        parse_u32(denominator, "fraction denominator")?,
    )
}

fn parse_u32(value: &str, label: &str) -> Result<u32, DomainError> {
    value
        .trim()
        .parse()
        .map_err(|_| invalid_value(format!("{label} must be a non-negative integer")))
}

fn round_fraction(numerator: u128, denominator: u128) -> u128 {
    let whole = numerator / denominator;
    let remainder = numerator % denominator;
    whole + u128::from(remainder * 2 >= denominator)
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn invalid_value(message: impl Into<String>) -> DomainError {
    DomainError::InvalidMusicalValue(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn musical_values_parse_and_normalize() {
        assert_eq!(
            "5:3+2/4".parse::<MusicalPosition>().unwrap().to_string(),
            "5:3+1/2"
        );
        assert_eq!(
            "6/12".parse::<MusicalDuration>().unwrap().to_string(),
            "1/2"
        );
        let fraction: MusicalFraction =
            serde_json::from_str(r#"{"numerator":2,"denominator":4}"#).unwrap();
        assert_eq!(fraction, MusicalFraction::new(1, 2).unwrap());
        assert_eq!(
            serde_json::to_string(&fraction).unwrap(),
            r#"{"numerator":1,"denominator":2}"#
        );
    }

    #[test]
    fn position_conversion_uses_the_project_meter() {
        let timebase = ProjectTimebase::default();
        assert_eq!(
            timebase
                .musical_position_to_tick("5:3+1/2".parse().unwrap())
                .unwrap(),
            TimelineTick(17_760)
        );
        assert_eq!(
            timebase.tick_to_musical_position(TimelineTick(17_760)),
            "5:3+1/2".parse().unwrap()
        );

        let three_four = ProjectTimebase {
            time_signature_numerator: 3,
            time_signature_denominator: 4,
            ..timebase
        };
        assert_eq!(
            three_four
                .musical_position_to_tick("2:1".parse().unwrap())
                .unwrap(),
            TimelineTick(2_880)
        );

        let five_four = ProjectTimebase {
            time_signature_numerator: 5,
            ..timebase
        };
        assert_eq!(
            five_four
                .musical_position_to_tick("2:1".parse().unwrap())
                .unwrap(),
            TimelineTick(4_800)
        );

        let seven_eight = ProjectTimebase {
            time_signature_numerator: 7,
            time_signature_denominator: 8,
            ..timebase
        };
        assert_eq!(
            seven_eight
                .musical_position_to_tick("2:1".parse().unwrap())
                .unwrap(),
            TimelineTick(3_360)
        );
    }

    #[test]
    fn duration_conversion_rounds_only_at_the_tick_boundary() {
        let timebase = ProjectTimebase::default();
        assert_eq!(
            timebase
                .musical_duration_to_ticks("1/12".parse().unwrap())
                .unwrap(),
            320
        );
        assert_eq!(
            timebase
                .musical_duration_to_ticks("1/1921".parse().unwrap())
                .unwrap(),
            2
        );
    }

    #[test]
    fn invalid_musical_values_are_rejected() {
        assert!("0:1".parse::<MusicalPosition>().is_err());
        assert!("1:5".parse::<MusicalPosition>().is_ok());
        assert!("1:1+1/1".parse::<MusicalPosition>().is_err());
        assert!("0/4".parse::<MusicalDuration>().is_err());
        assert!(
            ProjectTimebase::default()
                .musical_position_to_tick(MusicalPosition {
                    bar: 0,
                    beat: 1,
                    offset: MusicalFraction::default(),
                })
                .is_err()
        );
        assert!(
            ProjectTimebase::default()
                .musical_duration_to_ticks(MusicalDuration {
                    numerator: 1,
                    denominator: 0,
                })
                .is_err()
        );
    }
}
