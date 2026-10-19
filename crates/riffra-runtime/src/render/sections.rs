//! Marker ranges and measurements of the completed WAV.

use crate::api::output::{BandLevels, SectionLoudness, SectionMeasurement, SectionPeak};
use riffra_core::{Marker, ProjectTimebase};
use std::collections::BTreeMap;

pub(super) struct SectionPlan<'a> {
    pub(super) ranges: Vec<SectionMeasurement>,
    pub(super) timebase: &'a ProjectTimebase,
    pub(super) start_tick: u64,
}

impl<'a> SectionPlan<'a> {
    pub(super) fn new(
        markers: &[Marker],
        timebase: &'a ProjectTimebase,
        start_tick: u64,
        end_tick: u64,
        duration: f64,
    ) -> Self {
        Self {
            ranges: ranges(markers, timebase, start_tick, end_tick, duration),
            timebase,
            start_tick,
        }
    }
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

struct LoudnessFrame {
    end: f64,
    momentary: f64,
    peak: f64,
}

fn parse_frames(log: &str) -> Result<Vec<LoudnessFrame>, String> {
    fn number(text: &str) -> Result<f64, String> {
        text.split_whitespace()
            .next()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() || *v == f64::NEG_INFINITY)
            .ok_or_else(|| "ffmpeg section log contains an invalid number".into())
    }
    let mut frames = Vec::new();
    for line in log
        .lines()
        .filter(|line| line.contains("t:") && line.contains("FTPK:"))
    {
        let (_, time) = line
            .split_once("t:")
            .ok_or("ffmpeg section log is missing time")?;
        let (_, momentary) = time
            .split_once("M:")
            .ok_or("ffmpeg section log is missing momentary loudness")?;
        let (_, peaks) = momentary
            .split_once("FTPK:")
            .ok_or("ffmpeg section log is missing frame peaks")?;
        let (peaks, _) = peaks
            .split_once("dBFS")
            .ok_or("ffmpeg section log is missing peak units")?;
        let peaks = peaks
            .split_whitespace()
            .map(number)
            .collect::<Result<Vec<_>, _>>()?;
        if peaks.is_empty() {
            return Err("ffmpeg section log contains no channel peaks".into());
        }
        let end = number(time)?;
        if !end.is_finite()
            || end < 0.0
            || frames.last().is_some_and(|f: &LoudnessFrame| f.end >= end)
        {
            return Err("ffmpeg section log contains invalid frame times".into());
        }
        frames.push(LoudnessFrame {
            end,
            momentary: number(momentary)?,
            peak: peaks.into_iter().fold(f64::NEG_INFINITY, f64::max),
        });
    }
    if frames.is_empty() {
        return Err("ffmpeg did not emit section loudness frames".into());
    }
    Ok(frames)
}

pub(super) fn measure_loudness(
    sections: &mut [SectionMeasurement],
    log: &str,
    timebase: &ProjectTimebase,
    start_tick: u64,
    sample_rate: u32,
    audio_duration: f64,
) -> Result<(), String> {
    let frames = parse_frames(log)?;
    let origin = timebase.ticks_to_seconds(start_tick);
    let samples_per_second = f64::from(sample_rate);
    for section in sections {
        let blocks: Vec<_> = frames
            .iter()
            .filter(|frame| {
                frame.end < audio_duration
                    && frame.end - 0.4 >= section.start_seconds - 0.001
                    && frame.end <= section.end_seconds + 0.001
                    && frame.momentary > -70.0
            })
            .map(|frame| frame.momentary)
            .collect();
        let relative_gate = energy_mean(&blocks).map(|mean| mean - 10.0);
        let gated: Vec<_> = blocks
            .into_iter()
            .filter(|value| relative_gate.is_some_and(|gate| *value > gate))
            .collect();
        let mut peak: Option<SectionPeak> = None;
        for frame in &frames {
            // FFmpeg reports the last sample time to microsecond precision.
            let seconds =
                ((frame.end + 1.0 / samples_per_second - 0.1) * 1_000_000.0).round() / 1_000_000.0;
            let seconds = seconds.max(0.0);
            if seconds >= section.start_seconds
                && seconds < section.end_seconds
                && frame.peak.is_finite()
                && peak.as_ref().is_none_or(|peak| frame.peak > peak.db)
            {
                peak = Some(SectionPeak {
                    db: frame.peak,
                    seconds,
                    position: timebase
                        .tick_to_musical_position(timebase.seconds_to_ticks(origin + seconds)),
                });
            }
        }
        section.loudness = Some(SectionLoudness {
            integrated_lufs: energy_mean(&gated),
            true_peak: peak,
        });
    }
    Ok(())
}

fn energy_mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| {
        10.0 * (values
            .iter()
            .map(|value| 10.0_f64.powf(value / 10.0))
            .sum::<f64>()
            / values.len() as f64)
            .log10()
    })
}

#[derive(Clone)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn new(sample_rate: u32, frequency: f64, high_pass: bool) -> Self {
        let omega = std::f64::consts::TAU * frequency / f64::from(sample_rate);
        let cosine = omega.cos();
        let alpha = omega.sin() / std::f64::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        let b = if high_pass {
            [(1.0 + cosine) / 2.0, -(1.0 + cosine), (1.0 + cosine) / 2.0]
        } else {
            [(1.0 - cosine) / 2.0, 1.0 - cosine, (1.0 - cosine) / 2.0]
        };
        Self {
            b: b.map(|v| v / a0),
            a: [-2.0 * cosine / a0, (1.0 - alpha) / a0],
            z: [0.0; 2],
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let output = self.b[0] * input + self.z[0];
        self.z[0] = self.b[1] * input - self.a[0] * output + self.z[1];
        self.z[1] = self.b[2] * input - self.a[1] * output;
        output
    }
}

pub(super) struct BandMeter {
    filters: Vec<[Biquad; 4]>,
    bounds: Vec<(u64, u64)>,
    sums: Vec<[f64; 3]>,
    counts: Vec<u64>,
    sample: u64,
    section: usize,
}

impl BandMeter {
    pub(super) fn new(sample_rate: u32, channels: u16, sections: &[SectionMeasurement]) -> Self {
        Self {
            filters: vec![
                [
                    Biquad::new(sample_rate, 250.0, false),
                    Biquad::new(sample_rate, 250.0, true),
                    Biquad::new(sample_rate, 4000.0, false),
                    Biquad::new(sample_rate, 4000.0, true)
                ];
                usize::from(channels)
            ],
            bounds: sections
                .iter()
                .map(|s| {
                    (
                        (s.start_seconds * f64::from(sample_rate)).round() as u64,
                        (s.end_seconds * f64::from(sample_rate)).round() as u64,
                    )
                })
                .collect(),
            sums: vec![[0.0; 3]; sections.len()],
            counts: vec![0; sections.len()],
            sample: 0,
            section: 0,
        }
    }

    pub(super) fn push(&mut self, value: f64) {
        if self.bounds.is_empty() {
            return;
        }
        let channel = self.sample as usize % self.filters.len();
        let frame = self.sample / self.filters.len() as u64;
        self.sample += 1;
        let [low, mid_high, mid_low, high] = &mut self.filters[channel];
        let values = [
            low.process(value),
            mid_low.process(mid_high.process(value)),
            high.process(value),
        ];
        while self.section < self.bounds.len() && frame >= self.bounds[self.section].1 {
            self.section += 1;
        }
        if self.section < self.bounds.len() && frame >= self.bounds[self.section].0 {
            for (sum, value) in self.sums[self.section].iter_mut().zip(values) {
                *sum += value * value;
            }
            self.counts[self.section] += 1;
        }
    }

    pub(super) fn finish(self, sections: &mut [SectionMeasurement]) {
        for ((sums, count), section) in self.sums.into_iter().zip(self.counts).zip(sections) {
            let [low_db, mid_db, high_db] = sums
                .map(|sum| (sum > 0.0 && count > 0).then(|| 10.0 * (sum / count as f64).log10()));
            section.bands = BandLevels {
                low_db,
                mid_db,
                high_db,
            };
        }
    }
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

    #[test]
    fn parses_ffmpeg_frames_and_applies_gates_and_earliest_peak() {
        let example = "[Parsed_ebur128_0 @ 0000025a6f97b080] t: 0.599979   TARGET:-23 LUFS    M: -15.9 S:-120.7     I: -18.5 LUFS       LRA:   0.0 LU  FTPK:  -0.3 -20.0 dBFS  TPK:  -0.3 -20.0 dBFS";
        let frame = parse_frames(example).unwrap().remove(0);
        assert_eq!(frame.end, 0.599979);
        assert_eq!(frame.momentary, -15.9);
        assert_eq!(frame.peak, -0.3);
        let silent = parse_frames("t: 0.099979 M: -inf FTPK: -inf -inf dBFS")
            .unwrap()
            .remove(0);
        assert_eq!(silent.momentary, f64::NEG_INFINITY);
        assert_eq!(silent.peak, f64::NEG_INFINITY);

        let timebase = ProjectTimebase::default();
        let mut sections = ranges(
            &[
                marker(0, "First"),
                marker(1920, "Second"),
                marker(3840, "Silent"),
                marker(5760, "Short"),
            ],
            &timebase,
            0,
            6144,
            3.2,
        );
        let mut log = String::new();
        for i in 1..=32 {
            let loudness = if i >= 31 {
                -10.0
            } else if i >= 21 {
                f64::NEG_INFINITY
            } else if i == 5 {
                -80.0
            } else if i == 6 {
                -65.0
            } else if i <= 10 {
                -20.0
            } else {
                -40.0
            };
            let peak = if i >= 31 {
                -2.0
            } else if i >= 21 {
                f64::NEG_INFINITY
            } else if i == 11 || i == 15 || i == 16 {
                -1.0
            } else {
                -10.0
            };
            log.push_str(&format!(
                "t: {:.6} M: {loudness} FTPK: {peak} {peak} dBFS\n",
                f64::from(i) / 10.0 - 1.0 / 48_000.0
            ));
        }

        measure_loudness(&mut sections, &log, &timebase, 0, 48_000, 3.2).unwrap();

        let first = sections[0].loudness.as_ref().unwrap();
        let second = sections[1].loudness.as_ref().unwrap();
        assert!((first.integrated_lufs.unwrap() + 20.0).abs() < 0.001);
        assert!((second.integrated_lufs.unwrap() + 40.0).abs() < 0.001);
        let peak = second.true_peak.as_ref().unwrap();
        assert_eq!(peak.db, -1.0);
        assert!((peak.seconds - 1.0).abs() < 1.0e-9);
        assert_eq!(
            peak.position,
            timebase.tick_to_musical_position(timebase.seconds_to_ticks(peak.seconds))
        );
        let silent = sections[2].loudness.as_ref().unwrap();
        assert_eq!(silent.integrated_lufs, None);
        assert!(silent.true_peak.is_none());
        let short = sections[3].loudness.as_ref().unwrap();
        assert_eq!(short.integrated_lufs, None);
        assert_eq!(short.true_peak.as_ref().unwrap().db, -2.0);
        assert!(parse_frames("t: 0.5 M: invalid FTPK: -2 dBFS").is_err());
        assert!(parse_frames("no frames").is_err());
    }

    #[test]
    fn bands_separate_low_and_high_tones_and_preserve_silence() {
        let timebase = ProjectTimebase::default();
        for (frequency, dominant) in [(100.0, 0), (8000.0, 2), (0.0, 0)] {
            let mut sections = ranges(&[marker(0, "Tone")], &timebase, 0, 3840, 2.0);
            let mut meter = BandMeter::new(48000, 2, &sections);

            for i in 0..96000 {
                let value =
                    (f64::from(i) * frequency * std::f64::consts::TAU / 48000.0).sin() * 0.5;
                meter.push(value);
                meter.push(0.0);
            }
            meter.finish(&mut sections);

            let bands = &sections[0].bands;
            let levels = [bands.low_db, bands.mid_db, bands.high_db];
            if frequency == 0.0 {
                assert_eq!(levels, [None; 3]);
            } else {
                assert!(
                    levels
                        .iter()
                        .enumerate()
                        .all(|(i, level)| i == dominant
                            || level.unwrap() < levels[dominant].unwrap())
                );
                // A sine on one of two channels has mean-square amplitude A²/4.
                assert!(
                    (levels[dominant].unwrap() - 10.0 * (0.5_f64.powi(2) / 4.0).log10()).abs()
                        < 0.7
                );
            }
        }
    }
}
