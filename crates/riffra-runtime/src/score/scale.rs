//! The scale spellings Score knows without `scale_definitions`.

use super::syntax;

/// Degree spellings of every built-in scale.
const KNOWN: &[(&str, &[&str])] = &[
    ("ionian", &["1", "2", "3", "4", "5", "6", "7"]),
    ("dorian", &["1", "2", "b3", "4", "5", "6", "b7"]),
    ("phrygian", &["1", "b2", "b3", "4", "5", "b6", "b7"]),
    ("lydian", &["1", "2", "3", "#4", "5", "6", "7"]),
    ("mixolydian", &["1", "2", "3", "4", "5", "6", "b7"]),
    ("aeolian", &["1", "2", "b3", "4", "5", "b6", "b7"]),
    ("locrian", &["1", "b2", "b3", "4", "b5", "b6", "b7"]),
    ("harmonic_minor", &["1", "2", "b3", "4", "5", "b6", "7"]),
    ("melodic_minor", &["1", "2", "b3", "4", "5", "6", "7"]),
    ("major_pentatonic", &["1", "2", "3", "5", "6"]),
    ("minor_pentatonic", &["1", "b3", "4", "5", "b7"]),
    ("whole_tone", &["1", "2", "3", "#4", "#5", "#6"]),
];

/// Returns the degree spellings of a built-in scale.
pub(crate) fn degrees(name: &str) -> Option<&'static [&'static str]> {
    KNOWN
        .iter()
        .find(|(scale, _)| *scale == name)
        .map(|(_, degrees)| *degrees)
}

/// Resolves scale degrees to their semitone offsets from the tonic, skipping
/// unparseable entries.
pub(crate) fn offsets<'a>(degrees: impl IntoIterator<Item = &'a str>) -> Vec<i32> {
    degrees
        .into_iter()
        .filter_map(|degree| syntax::degree(degree).ok())
        .collect()
}
