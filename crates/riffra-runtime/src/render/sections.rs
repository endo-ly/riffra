//! Marker ranges and measurements of the completed WAV.

use crate::api::output::{BandLevels, ScoreSectionMeasurement, SectionMeasurement};
use riffra_core::{Marker, ProjectTimebase, TimelineRegion};
use std::collections::BTreeMap;

pub(super) struct SectionPlan<'a> {
    pub(super) ranges: Vec<SectionMeasurement>,
    pub(super) score_sections: Vec<ScoreSectionMeasurement>,
    pub(super) timebase: &'a ProjectTimebase,
    pub(super) start_tick: u64,
}

impl<'a> SectionPlan<'a> {
    pub(super) fn new(
        markers: &[Marker],
        score_regions: Option<&[TimelineRegion]>,
        timebase: &'a ProjectTimebase,
        start_tick: u64,
        end_tick: u64,
        duration: f64,
    ) -> Self {
        Self {
            ranges: ranges(markers, timebase, start_tick, end_tick, duration),
            score_sections: score_regions.map_or_else(Vec::new, |regions| {
                score_ranges(regions, timebase, start_tick, end_tick, duration)
            }),
            timebase,
            start_tick,
        }
    }
}

fn score_ranges(
    regions: &[TimelineRegion],
    timebase: &ProjectTimebase,
    start_tick: u64,
    end_tick: u64,
    duration: f64,
) -> Vec<ScoreSectionMeasurement> {
    let origin = timebase.ticks_to_seconds(start_tick);
    let mut occurrences = BTreeMap::new();
    let mut measured = Vec::new();
    for region in regions {
        let occurrence = occurrences.entry(region.name.clone()).or_insert(0_u32);
        *occurrence += 1;
        let start = region.start_tick.0.max(start_tick);
        let end = region.end_tick.0.min(end_tick);
        if end <= start {
            continue;
        }
        let start_seconds = timebase.ticks_to_seconds(start) - origin;
        let end_seconds = (timebase.ticks_to_seconds(end) - origin).min(duration);
        if end_seconds <= start_seconds {
            continue;
        }
        measured.push(ScoreSectionMeasurement {
            name: region.name.clone(),
            occurrence: *occurrence,
            start_tick: region.start_tick.0,
            end_tick: region.end_tick.0,
            start_seconds,
            end_seconds,
            complete: region.start_tick.0 >= start_tick && region.end_tick.0 <= end_tick,
            loudness: None,
            bands: BandLevels {
                low_db: None,
                mid_db: None,
                high_db: None,
            },
        });
    }
    measured
}

fn ranges(
    markers: &[Marker],
    timebase: &ProjectTimebase,
    start_tick: u64,
    end_tick: u64,
    duration: f64,
) -> Vec<SectionMeasurement> {
    if markers.is_empty() {
        return Vec::new();
    }
    let markers: BTreeMap<_, _> = markers.iter().map(|m| (m.tick, m.name.clone())).collect();
    let initial = markers
        .range(..=start_tick)
        .next_back()
        .map(|(_, name)| name.clone());
    let mut starts = vec![(start_tick, initial)];
    starts.extend(
        markers
            .range((
                std::ops::Bound::Excluded(start_tick),
                std::ops::Bound::Excluded(end_tick),
            ))
            .map(|(tick, name)| (*tick, Some(name.clone()))),
    );
    let origin = timebase.ticks_to_seconds(start_tick);
    starts
        .iter()
        .enumerate()
        .map(|(index, (tick, name))| {
            let start_seconds = timebase.ticks_to_seconds(*tick) - origin;
            let end_seconds = starts.get(index + 1).map_or(duration, |(next, _)| {
                timebase.ticks_to_seconds(*next) - origin
            });
            SectionMeasurement {
                name: name.clone(),
                start: timebase.tick_to_musical_position(riffra_core::TimelineTick(*tick)),
                end: timebase
                    .tick_to_musical_position(timebase.seconds_to_ticks(origin + end_seconds)),
                start_seconds,
                end_seconds,
                loudness: None,
                bands: BandLevels {
                    low_db: None,
                    mid_db: None,
                    high_db: None,
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(tick: u64, name: &str) -> Marker {
        Marker {
            id: name.into(),
            name: name.into(),
            tick,
        }
    }

    fn region(name: &str, start: u64, end: u64) -> TimelineRegion {
        TimelineRegion {
            id: format!("{name}:{start}"),
            name: name.into(),
            start_tick: riffra_core::TimelineTick(start),
            end_tick: riffra_core::TimelineTick(end),
        }
    }

    #[test]
    fn score_ranges_keep_occurrences_and_exclude_the_render_tail() {
        let timebase = ProjectTimebase::default();
        let regions = [
            region("walk", 0, 3840),
            region("walk", 3840, 7680),
            region("verse", 7680, 11520),
        ];

        let sections = score_ranges(&regions, &timebase, 1920, 9600, 4.0);

        assert_eq!(
            sections
                .iter()
                .map(|section| (section.name.as_str(), section.occurrence))
                .collect::<Vec<_>>(),
            [("walk", 1), ("walk", 2), ("verse", 1)]
        );
        assert_eq!(
            sections
                .iter()
                .map(|section| (section.start_seconds, section.end_seconds, section.complete))
                .collect::<Vec<_>>(),
            [(0.0, 1.0, false), (1.0, 3.0, true), (3.0, 4.0, false)]
        );
    }

    #[test]
    fn ranges_follow_canonical_markers_and_extend_through_tail() {
        let timebase = ProjectTimebase::default();
        let markers = [
            marker(3840, "Verse"),
            marker(7680, "Chorus"),
            marker(3840, "Intro"),
            marker(11520, "Outside"),
        ];

        let sections = ranges(&markers, &timebase, 0, 11520, 7.0);
        let cropped = ranges(&markers, &timebase, 5760, 11520, 4.0);

        assert_eq!(
            sections
                .iter()
                .map(|s| s.name.as_deref())
                .collect::<Vec<_>>(),
            [None, Some("Intro"), Some("Chorus")]
        );
        assert_eq!(
            sections
                .iter()
                .map(|s| (s.start_seconds, s.end_seconds))
                .collect::<Vec<_>>(),
            [(0.0, 2.0), (2.0, 4.0), (4.0, 7.0)]
        );
        assert_eq!(sections[2].end.to_string(), "4:3");
        assert_eq!(cropped[0].name.as_deref(), Some("Intro"));
        assert_eq!(cropped[1].start_seconds, 1.0);
        assert_eq!(cropped[1].end_seconds, 4.0);
        assert!(ranges(&[], &timebase, 0, 3840, 2.0).is_empty());
    }
}
