use crate::{
    analysis::{decode_sample, parse_wav},
    model::ScratchSession,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

const MAX_RENDER_MINUTES: u64 = 30;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderOptions {
    #[serde(default)]
    pub range_start_ms: u64,
    #[serde(default)]
    pub range_end_ms: Option<u64>,
    #[serde(default)]
    pub normalize: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub id: String,
    pub path: String,
    pub sample_rate: u32,
    pub frames: u64,
    pub duration_ms: u64,
    pub clip_count: usize,
    pub range_start_ms: u64,
    pub range_end_ms: u64,
    pub normalized: bool,
    pub state: String,
    pub message: String,
}

pub fn render_timeline_with_options(
    data_root: &Path,
    session: &ScratchSession,
    created_at_ms: u64,
    options: RenderOptions,
) -> Result<RenderResult, String> {
    let has_solo = session.tracks.iter().any(|track| track.solo);
    let clips = session
        .timeline
        .iter()
        .filter(|clip| !clip.muted)
        .filter(|clip| {
            let track = session
                .tracks
                .iter()
                .find(|track| track.id == clip.track_id);
            track
                .map(|track| !track.muted && (!has_solo || track.solo))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    if clips.is_empty() {
        return Err("Timeline has no audible clips to render.".into());
    }
    let mut sources = Vec::with_capacity(clips.len());
    let mut sample_rate = None;
    let mut total_frames = 0_u64;
    for clip in &clips {
        let source = PathBuf::from(&clip.asset_path);
        let bytes = fs::read(&source).map_err(|error| {
            format!("Timeline source '{}' could not be read: {error}", clip.name)
        })?;
        let wav = parse_wav(&bytes)?;
        if wav.channels == 0
            || !matches!(wav.format, 1 | 3)
            || !matches!(wav.bits_per_sample, 8 | 16 | 24 | 32)
        {
            return Err(format!(
                "Timeline source '{}' is not a supported PCM WAV.",
                clip.name
            ));
        }
        if wav.format == 3 && wav.bits_per_sample != 32 {
            return Err(format!(
                "Timeline source '{}' uses an unsupported float width.",
                clip.name
            ));
        }
        if let Some(expected) = sample_rate {
            if expected != wav.sample_rate {
                return Err(
                    "Timeline sources must share one sample rate before offline render.".into(),
                );
            }
        } else {
            sample_rate = Some(wav.sample_rate);
        }
        let bytes_per_sample = usize::from(wav.bits_per_sample / 8);
        let frame_bytes = bytes_per_sample * usize::from(wav.channels);
        let source_frames = (wav.data_len / frame_bytes) as u64;
        let start_frame = clip.start_ms.saturating_mul(u64::from(wav.sample_rate)) / 1_000;
        let duration_frames = clip.duration_ms.saturating_mul(u64::from(wav.sample_rate)) / 1_000;
        let source_in_frame = clip.source_in_ms.saturating_mul(u64::from(wav.sample_rate)) / 1_000;
        let source_out_frame = if clip.source_out_ms == 0 {
            source_frames
        } else {
            clip.source_out_ms
                .saturating_mul(u64::from(wav.sample_rate))
                / 1_000
        }
        .min(source_frames);
        let available_frames =
            source_out_frame.saturating_sub(source_in_frame.min(source_out_frame));
        let audible_frames = if clip.loop_enabled {
            duration_frames
        } else {
            duration_frames.min(available_frames)
        };
        total_frames = total_frames.max(start_frame.saturating_add(audible_frames));
        sources.push((clip, bytes, wav));
    }
    let sample_rate =
        sample_rate.ok_or_else(|| "Timeline has no renderable sources.".to_string())?;
    let max_frames = u64::from(sample_rate).saturating_mul(60 * MAX_RENDER_MINUTES);
    if total_frames == 0 || total_frames > max_frames {
        return Err(format!(
            "Timeline render is limited to {MAX_RENDER_MINUTES} minutes."
        ));
    }
    let render_start_frame = options
        .range_start_ms
        .saturating_mul(u64::from(sample_rate))
        / 1_000;
    let requested_end_frame = options
        .range_end_ms
        .filter(|end| *end > options.range_start_ms)
        .map(|end| end.saturating_mul(u64::from(sample_rate)) / 1_000)
        .unwrap_or(total_frames);
    let render_end_frame = requested_end_frame.min(total_frames);
    if render_start_frame >= render_end_frame {
        return Err("Render range does not overlap the audible timeline.".into());
    }
    let output_frames = render_end_frame.saturating_sub(render_start_frame);
    let frames =
        usize::try_from(output_frames).map_err(|_| "Timeline render is too large.".to_string())?;
    let render_start_frame_usize = usize::try_from(render_start_frame)
        .map_err(|_| "Timeline render range is too large.".to_string())?;
    let render_end_frame_usize = usize::try_from(render_end_frame)
        .map_err(|_| "Timeline render range is too large.".to_string())?;
    let mut output = vec![
        0.0_f32;
        frames
            .checked_mul(2)
            .ok_or("Timeline render is too large.")?
    ];
    for (clip, bytes, wav) in sources {
        let bytes_per_sample = usize::from(wav.bits_per_sample / 8);
        let frame_bytes = bytes_per_sample * usize::from(wav.channels);
        let source_frames = wav.data_len / frame_bytes;
        let data = bytes
            .get(wav.data_offset..wav.data_offset + source_frames * frame_bytes)
            .ok_or_else(|| {
                format!(
                    "Timeline source '{}' has an invalid data boundary.",
                    clip.name
                )
            })?;
        let start_frame = (clip.start_ms.saturating_mul(u64::from(sample_rate)) / 1_000) as usize;
        let requested = (clip.duration_ms.saturating_mul(u64::from(sample_rate)) / 1_000) as usize;
        let source_start_frame =
            (clip.source_in_ms.saturating_mul(u64::from(sample_rate)) / 1_000) as usize;
        let source_end_frame = if clip.source_out_ms == 0 {
            source_frames
        } else {
            (clip.source_out_ms.saturating_mul(u64::from(sample_rate)) / 1_000) as usize
        }
        .min(source_frames);
        let source_range =
            source_end_frame.saturating_sub(source_start_frame.min(source_end_frame));
        if source_range == 0 {
            continue;
        }
        let track = session
            .tracks
            .iter()
            .find(|track| track.id == clip.track_id);
        let track_gain_db = track.map(|track| track.gain_db).unwrap_or_default();
        let track_pan = track.map(|track| track.pan).unwrap_or_default();
        let gain = 10.0_f32.powf(((clip.gain_db + track_gain_db) as f32) / 20.0);
        let pan = (clip.pan + track_pan).clamp(-1.0, 1.0) as f32;
        let render_frames = if clip.loop_enabled {
            requested
        } else {
            requested.min(source_range)
        };
        for frame in 0..render_frames {
            let absolute_frame = start_frame.saturating_add(frame);
            if absolute_frame < render_start_frame_usize || absolute_frame >= render_end_frame_usize
            {
                continue;
            }
            let output_frame = absolute_frame.saturating_sub(render_start_frame_usize);
            if output_frame >= frames {
                continue;
            }
            let source_frame = if clip.loop_enabled {
                source_start_frame + frame % source_range
            } else {
                source_start_frame + frame
            };
            let source_start = source_frame * frame_bytes;
            let left = decode_sample(
                &data[source_start..source_start + bytes_per_sample],
                wav.format,
                wav.bits_per_sample,
            )? as f32;
            let right = if wav.channels > 1 {
                decode_sample(
                    &data[source_start + bytes_per_sample..source_start + 2 * bytes_per_sample],
                    wav.format,
                    wav.bits_per_sample,
                )? as f32
            } else {
                left
            };
            let fade_in = if clip.fade_in_ms == 0 {
                1.0
            } else {
                ((frame as f64 * 1_000.0 / f64::from(sample_rate)) / clip.fade_in_ms as f64)
                    .clamp(0.0, 1.0) as f32
            };
            let fade_out = if clip.fade_out_ms == 0 {
                1.0
            } else {
                let remaining_ms = (render_frames.saturating_sub(frame + 1) as f64 * 1_000.0)
                    / f64::from(sample_rate);
                (remaining_ms / clip.fade_out_ms as f64).clamp(0.0, 1.0) as f32
            };
            let envelope = fade_in.min(fade_out);
            let left_pan = (1.0 - pan).clamp(0.0, 1.0);
            let right_pan = (1.0 + pan).clamp(0.0, 1.0);
            let left = left * gain * envelope * left_pan;
            let right = right * gain * envelope * right_pan;
            let output_start = output_frame * 2;
            output[output_start] = (output[output_start] + left).clamp(-1.0, 1.0);
            output[output_start + 1] = (output[output_start + 1] + right).clamp(-1.0, 1.0);
        }
    }

    apply_master_gain(&mut output, session.master_db);
    if options.normalize {
        normalize_peak(&mut output);
    }

    let directory = data_root
        .join("exports")
        .join(format!("render-{created_at_ms}"));
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Render output folder could not be created: {error}"))?;
    let path = directory.join("timeline.wav");
    let partial = directory.join("timeline.wav.partial");
    write_float_wav(&partial, sample_rate, &output)
        .map_err(|error| format!("Timeline render could not be written: {error}"))?;
    fs::rename(&partial, &path)
        .map_err(|error| format!("Timeline render could not be finalized: {error}"))?;
    let result = RenderResult {
        id: format!("render:{created_at_ms}"),
        path: path.to_string_lossy().into_owned(),
        sample_rate,
        frames: output_frames,
        duration_ms: output_frames * 1_000 / u64::from(sample_rate),
        clip_count: clips.len(),
        range_start_ms: render_start_frame * 1_000 / u64::from(sample_rate),
        range_end_ms: render_end_frame * 1_000 / u64::from(sample_rate),
        normalized: options.normalize,
        state: "completed".into(),
        message:
            if options.normalize {
                "Timeline rendered to a normalized stereo WAV with master gain; source clips remain unchanged."
            } else {
                "Timeline rendered to a new stereo WAV with master gain; source clips remain unchanged."
            }
            .into(),
    };
    let manifest = directory.join("render.json");
    fs::write(
        &manifest,
        serde_json::to_vec_pretty(&result).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Render manifest could not be saved: {error}"))?;
    Ok(result)
}

fn apply_master_gain(samples: &mut [f32], gain_db: f64) {
    let gain = 10.0_f32.powf((gain_db as f32) / 20.0);
    for sample in samples {
        if !sample.is_finite() {
            *sample = 0.0;
        } else {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
}

fn normalize_peak(samples: &mut [f32]) {
    let peak = samples
        .iter()
        .filter_map(|sample| sample.is_finite().then_some(sample.abs()))
        .fold(0.0_f32, f32::max);
    if peak <= 0.0 {
        return;
    }
    let gain = (0.98 / peak).min(1.0);
    for sample in samples {
        *sample = if sample.is_finite() {
            (*sample * gain).clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }
}

fn write_float_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> io::Result<()> {
    let data_len = u32::try_from(
        samples
            .len()
            .checked_mul(4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WAV is too large"))?,
    )
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "WAV is too large"))?;
    let mut writer = BufWriter::new(File::create(path)?);
    writer.write_all(b"RIFF")?;
    writer.write_all(&(36_u32 + data_len).to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&16_u32.to_le_bytes())?;
    writer.write_all(&3_u16.to_le_bytes())?;
    writer.write_all(&2_u16.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&(sample_rate.saturating_mul(8)).to_le_bytes())?;
    writer.write_all(&8_u16.to_le_bytes())?;
    writer.write_all(&32_u16.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        writer.write_all(&sample.to_le_bytes())?;
    }
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analysis::analyze, model::ScratchSession, storage::now_ms};

    #[test]
    fn renders_non_destructive_timeline_to_stereo_float_wav() {
        let root = std::env::temp_dir().join(format!("riffra-render-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.wav");
        write_mono_test_wav(&source);
        let mut session = ScratchSession::new(now_ms());
        session.timeline.push(crate::model::TimelineClip {
            id: "clip:test".into(),
            asset_path: source.to_string_lossy().into_owned(),
            name: "source".into(),
            track_id: "main".into(),
            start_ms: 100,
            duration_ms: 200,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: -6.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let result =
            render_timeline_with_options(&root, &session, 42, RenderOptions::default()).unwrap();
        assert_eq!(result.clip_count, 1);
        let analysis = analyze(Path::new(&result.path)).unwrap();
        assert_eq!(analysis.channels, 2);
        assert!(analysis.samples >= 8_800);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn applies_bounded_master_gain_after_clip_mix() {
        let mut samples = vec![0.5_f32, -0.5, f32::NAN, 2.0];
        apply_master_gain(&mut samples, -6.0);
        assert!((samples[0] - 0.25059).abs() < 0.001);
        assert!((samples[1] + 0.25059).abs() < 0.001);
        assert_eq!(samples[2], 0.0);
        assert!(samples[3] <= 1.0);
    }

    #[test]
    fn loops_a_non_destructive_source_range_to_clip_length() {
        let root = std::env::temp_dir().join(format!("riffra-render-loop-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.wav");
        write_mono_test_wav(&source);
        let mut session = ScratchSession::new(now_ms());
        session.timeline.push(crate::model::TimelineClip {
            id: "clip:loop".into(),
            asset_path: source.to_string_lossy().into_owned(),
            name: "loop".into(),
            track_id: "main".into(),
            start_ms: 0,
            duration_ms: 200,
            source_in_ms: 0,
            source_out_ms: 50,
            loop_enabled: true,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let result =
            render_timeline_with_options(&root, &session, 43, RenderOptions::default()).unwrap();
        assert_eq!(result.frames, 8_820);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_only_the_requested_timeline_range() {
        let root = std::env::temp_dir().join(format!("riffra-render-range-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.wav");
        write_mono_test_wav(&source);
        let mut session = ScratchSession::new(now_ms());
        session.timeline.push(crate::model::TimelineClip {
            id: "clip:range".into(),
            asset_path: source.to_string_lossy().into_owned(),
            name: "range".into(),
            track_id: "main".into(),
            start_ms: 0,
            duration_ms: 200,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let result = render_timeline_with_options(
            &root,
            &session,
            44,
            RenderOptions {
                range_start_ms: 25,
                range_end_ms: Some(75),
                normalize: true,
            },
        )
        .unwrap();
        assert_eq!(result.frames, 2_205);
        assert_eq!(result.duration_ms, 50);
        assert!(result.normalized);
        let _ = fs::remove_dir_all(root);
    }

    fn write_mono_test_wav(path: &Path) {
        let data = vec![0_u8; 4_410 * 2];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36_u32 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&44_100_u32.to_le_bytes());
        wav.extend_from_slice(&(44_100_u32 * 2).to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        fs::write(path, wav).unwrap();
    }
}
