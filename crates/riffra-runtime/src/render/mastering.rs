//! Cancellable integrated-loudness mastering of the native float WAV.
//!
//! Mastering combines fixed input gain with oversampled peak limiting, then
//! verifies integrated loudness and true peak before replacing the output.

use super::sections::SectionPlan;
use crate::analysis::ffmpeg::run;
use crate::analysis::measurement::{TemporaryDirectory, loudnorm, measurement};
use crate::api::output::{AudioMeasurements, LoudnessReadout, MasteringDeviation, MasteringReport};
use riffra_core::LoudnessMastering;
use std::{fs, path::Path, sync::atomic::AtomicBool};

pub(super) fn master(
    input: &Path,
    target: &LoudnessMastering,
    sample_rate: u32,
    cancelled: Option<&AtomicBool>,
) -> Result<MasteringReport, String> {
    let directory = input
        .parent()
        .ok_or("mastering input has no parent directory")?
        .join(format!("mastering-{}", riffra_control::new_instance_id()));
    fs::create_dir(&directory)
        .map_err(|error| format!("mastering temporary directory could not be created: {error}"))?;
    let temporary = TemporaryDirectory(directory);
    let normalized = temporary.0.join("normalized.wav");
    let corrected = temporary.0.join("corrected.wav");
    let input_measurement = measurement(&loudnorm(
        input,
        None,
        "loudnorm=print_format=json",
        sample_rate,
        &temporary.0,
        cancelled,
    )?)?;
    let oversampled_rate = sample_rate
        .checked_mul(4)
        .ok_or("oversampled sample rate overflows")?;
    let limit = 10.0_f64.powf(target.true_peak_db / 20.0);
    let mut gain_db = target.integrated_lufs - input_measurement.integrated_lufs;
    for _ in 0..8 {
        let filter = format!(
            "volume={gain_db}dB:precision=double,aresample={oversampled_rate},alimiter=limit={limit}:attack=5:release=50:level=0:latency=1,aresample={sample_rate}"
        );
        run(
            input,
            Some(&normalized),
            &filter,
            sample_rate,
            &temporary.0,
            cancelled,
            "info",
        )?;
        let mut output = measurement(&loudnorm(
            &normalized,
            None,
            "loudnorm=print_format=json",
            sample_rate,
            &temporary.0,
            cancelled,
        )?)?;
        let mut final_path = normalized.as_path();
        let mut correction_db = 0.0;
        if output.true_peak_db >= target.true_peak_db {
            // loudnorm reports hundredths of a decibel; reserve one quantum.
            correction_db = output.true_peak_db - target.true_peak_db + 0.01;
            run(
                &normalized,
                Some(&corrected),
                &volume_filter(-correction_db),
                sample_rate,
                &temporary.0,
                cancelled,
                "info",
            )?;
            output = measurement(&loudnorm(
                &corrected,
                None,
                "loudnorm=print_format=json",
                sample_rate,
                &temporary.0,
                cancelled,
            )?)?;
            final_path = &corrected;
        }
        let deviation = output.integrated_lufs - target.integrated_lufs;
        if deviation.abs() <= 0.5 && output.true_peak_db <= target.true_peak_db {
            fs::copy(final_path, input)
                .map_err(|error| format!("mastered wav could not be saved: {error}"))?;
            return Ok(MasteringReport {
                target: target.clone(),
                input: input_measurement,
                deviation: MasteringDeviation {
                    integrated_lufs: deviation,
                    true_peak_db: output.true_peak_db - target.true_peak_db,
                    loudness_range_lu: target
                        .loudness_range_lu
                        .map(|target| output.loudness_range_lu - target),
                },
                normalization_type: "limited".into(),
                true_peak_correction_db: correction_db,
            });
        }
        if output.true_peak_db > target.true_peak_db {
            break;
        }
        gain_db -= deviation;
    }
    Err(format!(
        "master target not reached: target {} lufs / {} dbtp",
        target.integrated_lufs, target.true_peak_db
    ))
}

pub(super) fn measure_output(
    input: &Path,
    sample_rate: u32,
    mastering: Option<&MasteringReport>,
    target: Option<&LoudnessMastering>,
    section_plan: &SectionPlan<'_>,
    cancelled: Option<&AtomicBool>,
) -> Result<(AudioMeasurements, Vec<String>), String> {
    let ranges = section_plan
        .ranges
        .iter()
        .map(|s| crate::api::params::MeasurementRange {
            name: s.name.clone(),
            start_seconds: s.start_seconds,
            end_seconds: s.end_seconds,
        })
        .collect::<Vec<_>>();
    let measured = crate::analysis::measurement::measure(input, &ranges, cancelled)?;
    if measured.sample_rate != sample_rate {
        return Err("completed wav sample rate differs from render plan".into());
    }
    let crate::api::output::AnalysisMeasuredSource {
        sample_peak_dbfs,
        loudness,
        section_loudness_state,
        sections: measured_sections,
        ..
    } = measured;
    if target.is_some()
        && let LoudnessReadout::Unavailable { reason } = &loudness
    {
        return Err(format!(
            "mastered wav final loudness measurement failed: {reason}"
        ));
    }
    let origin = section_plan
        .timebase
        .ticks_to_seconds(section_plan.start_tick);
    let mut sections = section_plan.ranges.clone();
    for (section, measured) in sections.iter_mut().zip(measured_sections) {
        section.bands = measured.bands;
        section.loudness = measured
            .loudness
            .map(|l| crate::api::output::SectionLoudness {
                integrated_lufs: l.integrated_lufs,
                true_peak: l.true_peak.map(|p| crate::api::output::SectionPeak {
                    db: p.db,
                    seconds: p.seconds,
                    position: section_plan.timebase.tick_to_musical_position(
                        section_plan.timebase.seconds_to_ticks(origin + p.seconds),
                    ),
                }),
            });
    }
    let mut warnings = Vec::new();
    if sample_peak_dbfs.is_some_and(|peak| peak > 0.0) {
        warnings.push("sample peak exceeds 0 dbfs".into());
    }
    if let LoudnessReadout::Measured { measurement } = &loudness {
        if mastering.is_some_and(|report| report.true_peak_correction_db > 0.0) {
            warnings.push("true peak measurement required output gain correction".into());
        }
        if let Some(target) = target {
            if measurement.true_peak_db > target.true_peak_db {
                warnings.push("true peak exceeds the configured ceiling".into());
            }
            if (measurement.integrated_lufs - target.integrated_lufs).abs() > 0.5 {
                warnings.push(
                    "integrated loudness differs from the target by more than 0.5 lufs".into(),
                );
            }
            if target
                .loudness_range_lu
                .is_some_and(|target| measurement.loudness_range_lu > target + 0.5)
            {
                warnings.push("loudness range exceeds the target by more than 0.5 lu".into());
            }
        }
    }
    Ok((
        AudioMeasurements {
            sample_peak_dbfs,
            section_loudness_state,
            loudness,
            sections,
        },
        warnings,
    ))
}

fn volume_filter(gain_db: f64) -> String {
    format!("volume={gain_db}dB:precision=double")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_float_wav(path: &Path, sample_rate: u32, samples: &[f32]) {
        let frames = samples.len() as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + frames * 4).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&3_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(frames * 4).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        fs::write(path, bytes).unwrap();
    }

    fn segment_mean_square(
        path: &Path,
        sample_rate: u32,
        start_second: usize,
        seconds: usize,
    ) -> f64 {
        let wav = riffra_host::read_wav_structure(path).unwrap();
        use std::io::{Read, Seek, SeekFrom};
        let width = usize::from(wav.bits_per_sample / 8);
        let mut file = fs::File::open(path).unwrap();
        file.seek(SeekFrom::Start(
            wav.data_offset as u64 + u64::from(sample_rate) * start_second as u64 * width as u64,
        ))
        .unwrap();
        let count = sample_rate as usize * seconds * width;
        let mut chunk = vec![0_u8; count];
        file.read_exact(&mut chunk).unwrap();
        let sum: f64 = chunk
            .chunks_exact(width)
            .map(|raw| {
                let sample: f64 =
                    crate::analysis::decode_sample(raw, wav.format, wav.bits_per_sample).unwrap();
                sample * sample
            })
            .sum();
        sum / (chunk.len() / width) as f64
    }

    #[test]
    fn measures_float_samples_above_full_scale_and_reports_loudness_availability() {
        let root = std::env::temp_dir().join(format!(
            "riffra-output-measurement-{}",
            riffra_control::new_instance_id()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("output.wav");
        let sample_rate = 48_000_u32;
        let timebase = riffra_core::ProjectTimebase::default();
        let sections = SectionPlan::new(&[], &timebase, 0, 7680, 4.0);
        let frames = sample_rate * 4;
        let mut samples = Vec::new();
        for frame in 0..frames {
            let sample = if frame == sample_rate {
                1.25_f32
            } else {
                (f64::from(frame) * 440.0 * std::f64::consts::TAU / f64::from(sample_rate)).sin()
                    as f32
                    * 0.25
            };
            samples.push(sample);
        }
        write_float_wav(&path, sample_rate, &samples);

        let (measurements, warnings) =
            measure_output(&path, sample_rate, None, None, &sections, None).unwrap();

        assert!((measurements.sample_peak_dbfs.unwrap() - 20.0 * 1.25_f64.log10()).abs() < 0.0001);
        assert_eq!(warnings, ["sample peak exceeds 0 dbfs"]);
        match &measurements.loudness {
            LoudnessReadout::Measured { measurement } => {
                assert!(measurement.integrated_lufs < -5.0 && measurement.integrated_lufs > -30.0);
                assert!(measurement.true_peak_db > 0.0);
                assert!(measurement.loudness_range_lu.is_finite());
            }
            LoudnessReadout::Unavailable { reason } => assert!(reason.contains("ffmpeg")),
        }
        let encoded = serde_json::to_vec(&measurements).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&encoded).unwrap(),
            serde_json::to_value(&measurements).unwrap()
        );
        if matches!(measurements.loudness, LoudnessReadout::Measured { .. }) {
            let target = LoudnessMastering {
                integrated_lufs: -14.0,
                true_peak_db: -1.0,
                loudness_range_lu: Some(11.0),
            };
            let report = master(&path, &target, sample_rate, None).unwrap();
            assert!(report.deviation.true_peak_db <= 0.0);
            assert!(report.deviation.integrated_lufs.abs() <= 0.5);
            assert_eq!(report.normalization_type, "limited");
            let (final_measurements, warnings) = measure_output(
                &path,
                sample_rate,
                Some(&report),
                Some(&target),
                &sections,
                None,
            )
            .unwrap();
            assert!(final_measurements.sample_peak_dbfs.unwrap() <= 0.0);
            let measurement = match &final_measurements.loudness {
                LoudnessReadout::Measured { measurement } => measurement,
                LoudnessReadout::Unavailable { reason } => panic!("reason: {reason}"),
            };
            assert!(
                (measurement.integrated_lufs
                    - target.integrated_lufs
                    - report.deviation.integrated_lufs)
                    .abs()
                    < 1.0e-9
            );
            assert!(warnings.is_empty());
        }
        write_float_wav(&path, sample_rate, &vec![0.0; frames as usize]);
        let target = LoudnessMastering {
            integrated_lufs: -14.0,
            true_peak_db: -1.0,
            loudness_range_lu: Some(11.0),
        };
        let error =
            measure_output(&path, sample_rate, None, Some(&target), &sections, None).unwrap_err();
        assert!(error.contains("final loudness measurement failed"));
        assert!(master(&path, &target, sample_rate, None).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn measures_relative_segment_levels_and_overlapping_analysis_ranges() {
        let root = std::env::temp_dir().join(format!(
            "riffra-mastering-ratio-{}",
            riffra_control::new_instance_id()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("output.wav");
        let sample_rate = 48_000_u32;
        let timebase = riffra_core::ProjectTimebase::default();
        let markers = [
            riffra_core::Marker {
                id: "first".into(),
                name: "First".into(),
                tick: 0,
            },
            riffra_core::Marker {
                id: "second".into(),
                name: "Second".into(),
                tick: 3840,
            },
        ];
        let sections = SectionPlan::new(&markers, &timebase, 0, 7680, 4.0);
        let mut samples = Vec::new();
        for amplitude in [0.25_f32, 0.05_f32] {
            for frame in 0..sample_rate as usize * 2 {
                samples.push(if frame >= sample_rate as usize * 2 - 480 {
                    0.0
                } else {
                    (f64::from(frame as u32) * 440.0 * std::f64::consts::TAU
                        / f64::from(sample_rate))
                    .sin() as f32
                        * amplitude
                });
            }
        }
        write_float_wav(&path, sample_rate, &samples);

        let pre = segment_mean_square(&path, sample_rate, 0, 2);
        let pre_ratio = pre / segment_mean_square(&path, sample_rate, 2, 2);
        assert!(
            (pre_ratio - (0.25_f64 / 0.05_f64).powi(2)).abs() < 0.05,
            "pre {pre} ratio {pre_ratio}"
        );
        let (measurements, _) =
            measure_output(&path, sample_rate, None, None, &sections, None).unwrap();
        assert_eq!(measurements.sections.len(), 2);
        let ranges = vec![
            crate::api::params::MeasurementRange {
                name: Some("First".into()),
                start_seconds: 0.0,
                end_seconds: 2.0,
            },
            crate::api::params::MeasurementRange {
                name: Some("Second".into()),
                start_seconds: 2.0,
                end_seconds: 4.0,
            },
            crate::api::params::MeasurementRange {
                name: Some("Overlap".into()),
                start_seconds: 1.0,
                end_seconds: 3.0,
            },
        ];
        let copy = root.join("copy.wav");
        fs::copy(&path, &copy).unwrap();
        let params = crate::api::params::AnalysisMeasureParams {
            render_id: None,
            paths: Some(vec![
                copy.to_string_lossy().into_owned(),
                path.to_string_lossy().into_owned(),
            ]),
            ranges,
        };
        let sources = crate::analysis::measurement_sources(&root, &params).unwrap();
        let mut progress = Vec::new();
        let analyzed =
            crate::analysis::measure_sources(sources, params, &AtomicBool::new(false), |p, _| {
                progress.push(p)
            })
            .unwrap();
        assert_eq!(analyzed.sources.len(), 2);
        assert_eq!(Path::new(&analyzed.sources[0].path), copy);
        assert_eq!(Path::new(&analyzed.sources[1].path), path);
        assert_eq!(progress, [0.0, 0.5, 0.5, 1.0]);
        for source in &analyzed.sources {
            for (actual, expected) in source.sections.iter().zip(&measurements.sections) {
                assert_eq!(
                    serde_json::to_value(&actual.bands).unwrap(),
                    serde_json::to_value(&expected.bands).unwrap()
                );
                assert_eq!(
                    actual.loudness.as_ref().map(|l| l.integrated_lufs),
                    expected.loudness.as_ref().map(|l| l.integrated_lufs)
                );
                assert_eq!(
                    actual
                        .loudness
                        .as_ref()
                        .and_then(|l| l.true_peak.as_ref())
                        .map(|p| (p.db, p.seconds)),
                    expected
                        .loudness
                        .as_ref()
                        .and_then(|l| l.true_peak.as_ref())
                        .map(|p| (p.db, p.seconds))
                );
            }
            assert_eq!(source.sections[2].name.as_deref(), Some("Overlap"));
        }

        if matches!(
            measurements.section_loudness_state,
            crate::api::output::MeasurementState::Unavailable { .. }
        ) {
            assert!(
                measurements
                    .sections
                    .iter()
                    .all(|section| section.loudness.is_none())
            );
        }
        if matches!(
            measurements.section_loudness_state,
            crate::api::output::MeasurementState::Measured
        ) {
            let first = measurements.sections[0].loudness.as_ref().unwrap();
            let second = measurements.sections[1].loudness.as_ref().unwrap();
            let expected_difference = 20.0 * (0.25_f64 / 0.05).log10();
            assert!(
                (first.integrated_lufs.unwrap()
                    - second.integrated_lufs.unwrap()
                    - expected_difference)
                    .abs()
                    < 0.2
            );
            let peak = first.true_peak.as_ref().unwrap();
            assert!(peak.seconds < 0.1);
            assert!(
                peak.db > second.true_peak.as_ref().unwrap().db + 13.0,
                "first {} second {}",
                peak.db,
                second.true_peak.as_ref().unwrap().db
            );
            assert_eq!(
                peak.position,
                timebase.tick_to_musical_position(timebase.seconds_to_ticks(peak.seconds))
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
