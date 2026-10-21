//! Shared streaming PCM and FFmpeg measurements of existing WAVs.
use super::ffmpeg::run;
use crate::api::output::{AnalysisSectionMeasurement, BandLevels, RangeLoudness, RangePeak};
use crate::api::{
    output::{AnalysisMeasuredSource, LoudnessMeasurement, LoudnessReadout, MeasurementState},
    params::MeasurementRange,
};
use serde_json::Value;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

fn empty_section(
    name: Option<String>,
    start_seconds: f64,
    end_seconds: f64,
) -> AnalysisSectionMeasurement {
    AnalysisSectionMeasurement {
        name,
        start_seconds,
        end_seconds,
        loudness: None,
        bands: BandLevels {
            low_db: None,
            mid_db: None,
            high_db: None,
        },
    }
}

struct LoudnessFrame {
    end: f64,
    momentary: f64,
    peak: f64,
}

fn parse_frames(log: &str) -> Result<Vec<LoudnessFrame>, String> {
    fn number(text: &str, field: &str, line: &str) -> Result<f64, String> {
        let raw = text.split_whitespace().next().unwrap_or("");
        raw.parse::<f64>()
            .ok()
            .filter(|v| {
                v.is_finite() || (field != "t" && *v == f64::NEG_INFINITY)
                // ebur128 can emit M: nan after a signal decays into silence.
                // It supplies no usable loudness block; the gate excludes it.
                || (field == "M" && v.is_nan())
            })
            .ok_or_else(|| format!("ffmpeg section log invalid {field} value {raw:?}: {line}"))
    }
    let mut frames = Vec::new();
    for line in log
        .lines()
        .filter(|line| line.contains("t:") && (line.contains("M:") || line.contains("FTPK:")))
    {
        let (_, time) = line
            .split_once("t:")
            .ok_or_else(|| format!("ffmpeg section log is missing t: {line}"))?;
        let (_, momentary) = time
            .split_once("M:")
            .ok_or_else(|| format!("ffmpeg section log is missing M: {line}"))?;
        let (_, peaks) = momentary
            .split_once("FTPK:")
            .ok_or_else(|| format!("ffmpeg section log is missing FTPK: {line}"))?;
        let (peaks, _) = peaks
            .split_once("dBFS")
            .ok_or_else(|| format!("ffmpeg section log is missing FTPK units: {line}"))?;
        let peaks = peaks
            .split_whitespace()
            .map(|text| number(text, "FTPK", line))
            .collect::<Result<Vec<_>, _>>()?;
        if peaks.is_empty() {
            return Err(format!(
                "ffmpeg section log contains no FTPK channel values: {line}"
            ));
        }
        let end = number(time, "t", line)?;
        if !end.is_finite()
            || end < 0.0
            || frames.last().is_some_and(|f: &LoudnessFrame| f.end >= end)
        {
            return Err(format!(
                "ffmpeg section log contains invalid frame time {end}: {line}"
            ));
        }
        frames.push(LoudnessFrame {
            end,
            momentary: number(momentary, "M", line)?,
            peak: peaks.into_iter().fold(f64::NEG_INFINITY, f64::max),
        });
    }
    if frames.is_empty() {
        return Err("ffmpeg did not emit section loudness frames".into());
    }
    Ok(frames)
}

fn measure_loudness(
    sections: &mut [AnalysisSectionMeasurement],
    log: &str,
    sample_rate: u32,
    audio_duration: f64,
) -> Result<(), String> {
    let frames = parse_frames(log)?;
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
        let mut peak: Option<RangePeak> = None;
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
                peak = Some(RangePeak {
                    db: frame.peak,
                    seconds,
                });
            }
        }
        section.loudness = Some(RangeLoudness {
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

struct BandMeter {
    filters: Vec<[Biquad; 4]>,
    bounds: Vec<(usize, usize)>,
    boundaries: Vec<(u64, [f64; 3])>,
    next_boundary: usize,
    totals: [f64; 3],
    sample: u64,
}

impl BandMeter {
    fn new(sample_rate: u32, channels: u16, sections: &[AnalysisSectionMeasurement]) -> Self {
        let sample =
            |seconds: f64| (seconds * f64::from(sample_rate)).round() as u64 * u64::from(channels);
        let mut positions: Vec<_> = sections
            .iter()
            .flat_map(|section| [sample(section.start_seconds), sample(section.end_seconds)])
            .collect();
        positions.sort_unstable();
        positions.dedup();
        let bounds = sections
            .iter()
            .map(|section| {
                (
                    positions
                        .binary_search(&sample(section.start_seconds))
                        .expect("start boundary was collected"),
                    positions
                        .binary_search(&sample(section.end_seconds))
                        .expect("end boundary was collected"),
                )
            })
            .collect();
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
            bounds,
            boundaries: positions
                .into_iter()
                .map(|sample| (sample, [0.0; 3]))
                .collect(),
            next_boundary: 0,
            totals: [0.0; 3],
            sample: 0,
        }
    }

    fn push(&mut self, value: f64) {
        if self.bounds.is_empty() {
            return;
        }
        let channel = self.sample as usize % self.filters.len();
        self.capture_boundary();
        self.sample += 1;
        let [low, mid_high, mid_low, high] = &mut self.filters[channel];
        let values = [
            low.process(value),
            mid_low.process(mid_high.process(value)),
            high.process(value),
        ];
        for (sum, value) in self.totals.iter_mut().zip(values) {
            *sum += value * value;
        }
    }

    fn capture_boundary(&mut self) {
        if let Some((sample, sums)) = self.boundaries.get_mut(self.next_boundary)
            && *sample == self.sample
        {
            *sums = self.totals;
            self.next_boundary += 1;
        }
    }

    fn finish(mut self, sections: &mut [AnalysisSectionMeasurement]) {
        self.capture_boundary();
        for ((start, end), section) in self.bounds.into_iter().zip(sections) {
            let count = self.boundaries[end].0 - self.boundaries[start].0;
            let sums = std::array::from_fn::<_, 3, _>(|i| {
                self.boundaries[end].1[i] - self.boundaries[start].1[i]
            });
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

fn duration(wav: &riffra_host::WavMetadata) -> f64 {
    wav.data_len as f64
        / (f64::from(wav.channels)
            * f64::from(wav.bits_per_sample / 8)
            * f64::from(wav.sample_rate))
}

pub(crate) fn validate_wav(
    input: &Path,
    ranges: &[MeasurementRange],
) -> Result<riffra_host::WavMetadata, String> {
    let wav = riffra_host::read_wav_structure(input)?;
    if !matches!(wav.format, 1 | 3)
        || !matches!(wav.bits_per_sample, 8 | 16 | 24 | 32)
        || (wav.format == 3 && wav.bits_per_sample != 32)
    {
        return Err("completed wav sample format is unsupported".into());
    }
    if wav.channels == 0
        || wav.sample_rate == 0
        || wav.data_len == 0
        || !wav
            .data_len
            .is_multiple_of(usize::from(wav.channels) * usize::from(wav.bits_per_sample / 8))
    {
        return Err("wav must contain complete audio frames".into());
    }
    let duration = duration(&wav);
    for range in ranges {
        if !range.start_seconds.is_finite()
            || !range.end_seconds.is_finite()
            || range.start_seconds < 0.0
            || range.end_seconds <= range.start_seconds
            || range.end_seconds > duration
        {
            return Err(format!(
                "invalid measurement range {:?}: [{}, {}) for wav duration {duration}",
                range.name, range.start_seconds, range.end_seconds
            ));
        }
    }
    Ok(wav)
}

pub(crate) fn measure(
    input: &Path,
    ranges: &[MeasurementRange],
    cancelled: Option<&AtomicBool>,
) -> Result<AnalysisMeasuredSource, String> {
    measure_with_runner(
        input,
        ranges,
        cancelled,
        |filter, sample_rate, directory, log_level| {
            run(
                input,
                None,
                filter,
                sample_rate,
                directory,
                cancelled,
                log_level,
            )
        },
    )
}

fn measure_with_runner(
    input: &Path,
    ranges: &[MeasurementRange],
    cancelled: Option<&AtomicBool>,
    mut run_filter: impl FnMut(&str, u32, &Path, &str) -> Result<String, String>,
) -> Result<AnalysisMeasuredSource, String> {
    let wav = validate_wav(input, ranges)?;
    let sample_rate = wav.sample_rate;
    let duration_seconds = duration(&wav);
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(input)
        .map_err(|error| format!("completed wav could not be opened: {error}"))?;
    file.seek(SeekFrom::Start(wav.data_offset as u64))
        .map_err(|error| format!("completed wav samples could not be located: {error}"))?;
    let width = usize::from(wav.bits_per_sample / 8);
    let mut buffer = [0_u8; 24_576];
    let mut remaining = wav.data_len;
    let mut peak = 0.0_f64;
    let mut sections = ranges
        .iter()
        .map(|r| empty_section(r.name.clone(), r.start_seconds, r.end_seconds))
        .collect::<Vec<_>>();
    let mut bands = BandMeter::new(wav.sample_rate, wav.channels, &sections);
    while remaining > 0 {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err("audio measurement was cancelled".into());
        }
        let count = remaining.min(buffer.len());
        file.read_exact(&mut buffer[..count])
            .map_err(|error| format!("completed wav samples could not be read: {error}"))?;
        for sample in buffer[..count].chunks_exact(width) {
            let value = crate::analysis::decode_sample(sample, wav.format, wav.bits_per_sample)?;
            peak = peak.max(value.abs());
            bands.push(value);
        }
        remaining -= count;
    }
    bands.finish(&mut sections);
    let sample_peak_dbfs = (peak > 0.0).then(|| 20.0 * peak.log10());
    let directory =
        std::env::temp_dir().join(format!("measure-{}", riffra_control::new_instance_id()));
    let temporary = match fs::create_dir(&directory) {
        Ok(()) => Ok(TemporaryDirectory(directory)),
        Err(error) => Err(format!(
            "measurement directory could not be created: {error}"
        )),
    };
    let mut run_optional = |filter, log_level| match &temporary {
        Ok(directory) => run_filter(filter, sample_rate, &directory.0, log_level),
        Err(reason) => Err(reason.clone()),
    };
    let loudness = match run_optional("loudnorm=print_format=json", "info")
        .and_then(|text| parse_loudnorm(&text))
        .and_then(|report| measurement(&report))
    {
        Ok(measurement) => LoudnessReadout::Measured { measurement },
        Err(reason) => LoudnessReadout::Unavailable { reason },
    };
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err("audio measurement was cancelled".into());
    }
    let section_loudness_state = if sections.is_empty() {
        MeasurementState::Measured
    } else {
        match run_optional(
            "apad=pad_dur=0.1,ebur128=peak=true:framelog=verbose",
            "verbose",
        )
        .and_then(|log| measure_loudness(&mut sections, &log, sample_rate, duration_seconds))
        {
            Ok(()) => MeasurementState::Measured,
            Err(reason) => MeasurementState::Unavailable { reason },
        }
    };
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err("audio measurement was cancelled".into());
    }
    Ok(AnalysisMeasuredSource {
        path: input.to_string_lossy().into_owned(),
        track_id: None,
        track_name: None,
        sample_rate,
        duration_seconds,
        sample_peak_dbfs,
        loudness,
        section_loudness_state,
        sections,
    })
}
pub(crate) struct TemporaryDirectory(pub(crate) std::path::PathBuf);

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            tracing::error!(%error, "audio processing temporary files could not be removed");
        }
    }
}

pub(crate) fn loudnorm(
    input: &Path,
    output: Option<&Path>,
    filter: &str,
    sample_rate: u32,
    directory: &Path,
    cancelled: Option<&AtomicBool>,
) -> Result<Value, String> {
    let text = run(
        input,
        output,
        filter,
        sample_rate,
        directory,
        cancelled,
        "info",
    )?;
    parse_loudnorm(&text)
}

fn parse_loudnorm(text: &str) -> Result<Value, String> {
    for (index, character) in text.char_indices() {
        if character != '{' {
            continue;
        }
        let mut stream = serde_json::Deserializer::from_str(&text[index..]).into_iter::<Value>();
        if let Some(Ok(report)) = stream.next()
            && report.get("input_i").is_some()
        {
            return Ok(report);
        }
    }
    Err("ffmpeg loudnorm did not emit a json report".into())
}

fn number(report: &Value, field: &str) -> Result<f64, String> {
    report
        .get(field)
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("ffmpeg loudnorm report is missing finite {field}"))
}

pub(crate) fn measurement(report: &Value) -> Result<LoudnessMeasurement, String> {
    Ok(LoudnessMeasurement {
        integrated_lufs: number(report, "input_i")?,
        true_peak_db: number(report, "input_tp")?,
        loudness_range_lu: number(report, "input_lra")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_section_failure_preserves_pcm_and_whole_file_measurements() {
        let path = std::env::temp_dir().join(format!(
            "riffra-measure-failure-{}.wav",
            riffra_control::new_instance_id()
        ));
        let samples = vec![1000_i16; 48_000];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36_u32 + samples.len() as u32 * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&48000_u32.to_le_bytes());
        bytes.extend_from_slice(&96000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        fs::write(&path, &bytes).unwrap();
        let ranges = [MeasurementRange {
            name: None,
            start_seconds: 0.0,
            end_seconds: 1.0,
        }];

        let measured = measure_with_runner(&path, &ranges, None, |filter, _, _, _| {
            if filter.starts_with("loudnorm") {
                Ok(r#"{"input_i":"-20","input_tp":"-10","input_lra":"0"}"#.into())
            } else {
                Ok("t: 0.5 M: -20 FTPK: corrupt dBFS".into())
            }
        })
        .unwrap();

        assert!(measured.sample_peak_dbfs.is_some());
        assert!(matches!(
            measured.loudness,
            LoudnessReadout::Measured { .. }
        ));
        let MeasurementState::Unavailable { reason } = measured.section_loudness_state else {
            panic!("invalid frame must be unavailable");
        };
        assert!(reason.contains("FTPK") && reason.contains("corrupt"));
        assert!(measured.sections[0].loudness.is_none());
        assert!(measured.sections[0].bands.low_db.is_some());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        for (start, end) in [
            (0.0, 1.01),
            (-0.1, 0.5),
            (0.5, 0.5),
            (f64::NAN, 0.5),
            (0.0, f64::INFINITY),
        ] {
            let range = MeasurementRange {
                name: None,
                start_seconds: start,
                end_seconds: end,
            };
            assert!(validate_wav(&path, &[range]).is_err());
        }
        let cancelled = AtomicBool::new(true);
        assert!(
            measure(&path, &ranges, Some(&cancelled))
                .unwrap_err()
                .contains("cancelled")
        );
        fs::remove_file(path).unwrap();
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

        let mut sections = vec![
            empty_section(Some("First".into()), 0.0, 1.0),
            empty_section(Some("Second".into()), 1.0, 2.0),
            empty_section(Some("Silent".into()), 2.0, 3.0),
            empty_section(Some("Short".into()), 3.0, 3.2),
        ];
        let mut log = String::new();
        for i in 1..=32 {
            let loudness = if i >= 31 {
                -10.0
            } else if i >= 21 {
                if i % 2 == 0 {
                    f64::NAN
                } else {
                    f64::NEG_INFINITY
                }
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

        measure_loudness(&mut sections, &log, 48_000, 3.2).unwrap();

        let first = sections[0].loudness.as_ref().unwrap();
        let second = sections[1].loudness.as_ref().unwrap();
        assert!((first.integrated_lufs.unwrap() + 20.0).abs() < 0.001);
        assert!((second.integrated_lufs.unwrap() + 40.0).abs() < 0.001);
        let peak = second.true_peak.as_ref().unwrap();
        assert_eq!(peak.db, -1.0);
        assert!((peak.seconds - 1.0).abs() < 1.0e-9);
        let silent = sections[2].loudness.as_ref().unwrap();
        assert_eq!(silent.integrated_lufs, None);
        assert!(silent.true_peak.is_none());
        let short = sections[3].loudness.as_ref().unwrap();
        assert_eq!(short.integrated_lufs, None);
        assert_eq!(short.true_peak.as_ref().unwrap().db, -2.0);
        assert!(parse_frames("t: 0.5 M: invalid FTPK: -2 dBFS").is_err());
        assert!(parse_frames("no frames").is_err());
        let nan = parse_frames(
            "t: 1.499979 TARGET:-23 LUFS M: nan S:-120.7 I:-23.6 LUFS FTPK: -inf dBFS",
        )
        .unwrap();
        assert!(nan[0].momentary.is_nan());
        for (field, log) in [
            ("t", "t: nan M: -20 FTPK: -2 dBFS"),
            ("M", "t: 0.5 M: +inf FTPK: -2 dBFS"),
            ("FTPK", "t: 0.5 M: -20 FTPK: nan dBFS"),
        ] {
            let error = parse_frames(log).err().unwrap();
            assert!(error.contains(field) && error.contains(log), "{error}");
        }
    }

    #[test]
    fn bands_separate_low_and_high_tones_and_preserve_silence() {
        for (frequency, dominant) in [(100.0, 0), (8000.0, 2), (0.0, 0)] {
            let mut sections = vec![
                empty_section(Some("Tone".into()), 0.0, 2.0),
                empty_section(None, 1.0, 2.0),
                empty_section(None, 0.5, 1.5),
                empty_section(None, 0.0, 2.0),
            ];
            let mut meter = BandMeter::new(48000, 2, &sections);

            for i in 0..96000 {
                let value =
                    (f64::from(i) * frequency * std::f64::consts::TAU / 48000.0).sin() * 0.5;
                meter.push(value);
                meter.push(0.0);
            }
            meter.finish(&mut sections);

            assert_eq!(
                serde_json::to_value(&sections[0].bands).unwrap(),
                serde_json::to_value(&sections[3].bands).unwrap()
            );
            for section in &sections[1..3] {
                if frequency != 0.0 {
                    let values = [
                        section.bands.low_db,
                        section.bands.mid_db,
                        section.bands.high_db,
                    ];
                    let full = &sections[0].bands;
                    let full = [full.low_db, full.mid_db, full.high_db];
                    assert!((values[dominant].unwrap() - full[dominant].unwrap()).abs() < 0.1);
                } else {
                    assert!(section.bands.low_db.is_none());
                }
            }
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
