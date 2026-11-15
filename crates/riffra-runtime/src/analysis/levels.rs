use crate::api::output::{AnalysisLevelWindow, AnalysisLevelsResult, AnalysisLevelsSource};
use crate::api::params::{AnalysisLevelsParams, LevelSource};
use riffra_core::{ProjectTimebase, TimelineTick};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) struct LevelsPlan {
    render_id: String,
    grid: riffra_core::MusicalDuration,
    timebase: ProjectTimebase,
    sample_rate: u32,
    frames: u64,
    boundaries: Vec<TimelineTick>,
    offsets: Vec<u64>,
    sources: Vec<(LevelSource, PathBuf)>,
}

pub(crate) fn prepare(
    data_root: &std::path::Path,
    params: AnalysisLevelsParams,
) -> Result<LevelsPlan, String> {
    let render = crate::render::resolve_levels_render(data_root, &params.render_id)?;
    let matches_source = |candidate: &LevelSource, selected: &LevelSource| {
        matches!(
            (candidate, selected),
            (
                LevelSource::Mixdown { .. },
                LevelSource::Mixdown {
                    rendered_track_id: None
                }
            )
        ) || candidate == selected
    };
    let sources = render
        .sources
        .into_iter()
        .filter(|(source, _)| {
            params
                .source
                .as_ref()
                .is_none_or(|selected| matches_source(source, selected))
        })
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Err("requested source is not present in the render".into());
    }
    let start = params
        .start
        .map(|position| render.timebase.musical_position_to_tick(position))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(TimelineTick(render.start_tick));
    let end = params
        .end
        .map(|position| render.timebase.musical_position_to_tick(position))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(TimelineTick(render.end_tick));
    if start.0 < render.start_tick || end.0 > render.end_tick || start >= end {
        return Err(
            "analysis range must be a nonempty subset of the rendered musical range".into(),
        );
    }
    let max_cells = 4096 / sources.len();
    if max_cells == 0 {
        return Err("too many sources; select one source".into());
    }
    let boundaries = render
        .timebase
        .musical_grid_boundaries(start, end, params.grid, max_cells)
        .map_err(|error| error.to_string())?;
    let origin = render
        .timebase
        .ticks_to_frames(render.start_tick, render.sample_rate);
    let musical_frames = render
        .timebase
        .ticks_to_frames(render.end_tick, render.sample_rate)
        .checked_sub(origin)
        .ok_or("render frame positions are invalid")?;
    if musical_frames == 0 || musical_frames > render.frames {
        return Err("render musical range does not fit its WAV frame count".into());
    }
    let offsets = boundaries
        .iter()
        .map(|tick| {
            render
                .timebase
                .ticks_to_frames(tick.0, render.sample_rate)
                .checked_sub(origin)
                .ok_or_else(|| "render frame positions are invalid".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if offsets.windows(2).any(|pair| pair[0] >= pair[1])
        || offsets.last().is_none_or(|end| *end > render.frames)
    {
        return Err("grid has an empty sample window or extends beyond the rendered WAV".into());
    }
    for (_, path) in &sources {
        let wav = super::measurement::validate_wav(path, &[])?;
        if wav.sample_rate != render.sample_rate || wav.frame_count != render.frames {
            return Err(format!(
                "render WAV metadata does not match its manifest: {}",
                path.display()
            ));
        }
    }
    Ok(LevelsPlan {
        render_id: params.render_id,
        grid: params.grid,
        timebase: render.timebase,
        sample_rate: render.sample_rate,
        frames: render.frames,
        boundaries,
        offsets,
        sources,
    })
}

pub(crate) fn measure(
    plan: LevelsPlan,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(f32, String),
) -> Result<AnalysisLevelsResult, String> {
    let mut sources = Vec::with_capacity(plan.sources.len());
    for (index, (source, path)) in plan.sources.iter().enumerate() {
        progress(
            index as f32 / plan.sources.len() as f32,
            format!("measuring {}", path.display()),
        );
        let wav = super::measurement::validate_wav(path, &[])?;
        if wav.sample_rate != plan.sample_rate || wav.frame_count != plan.frames {
            return Err("render WAV metadata changed during analysis".into());
        }
        let width = usize::from(wav.bits_per_sample / 8);
        let frame_bytes = width * usize::from(wav.channels);
        let mut file =
            File::open(path).map_err(|error| format!("wav could not be opened: {error}"))?;
        let byte_offset = plan.offsets[0]
            .checked_mul(frame_bytes as u64)
            .and_then(|offset| offset.checked_add(wav.data_offset as u64))
            .ok_or("wav offset is too large")?;
        file.seek(SeekFrom::Start(byte_offset))
            .map_err(|error| format!("wav could not be positioned: {error}"))?;
        let chunk_frames = (24_576 / frame_bytes).max(1);
        let mut bytes = vec![0_u8; chunk_frames * frame_bytes];
        let mut windows = Vec::with_capacity(plan.offsets.len() - 1);
        for (index, offsets) in plan.offsets.windows(2).enumerate() {
            let frames = offsets[1] - offsets[0];
            let mut remaining = frames;
            let mut sum = 0.0_f64;
            let mut peak = 0.0_f64;
            while remaining > 0 {
                if cancelled.load(Ordering::Acquire) {
                    return Err("levels analysis was cancelled".into());
                }
                let count = remaining.min(chunk_frames as u64) as usize;
                let chunk = &mut bytes[..count * frame_bytes];
                file.read_exact(chunk)
                    .map_err(|error| format!("wav samples could not be read: {error}"))?;
                for sample in chunk.chunks_exact(width) {
                    let value = super::decode_sample(sample, wav.format, wav.bits_per_sample)?;
                    sum += value * value;
                    peak = peak.max(value.abs());
                }
                remaining -= count as u64;
            }
            let rms = (sum / (frames as f64 * f64::from(wav.channels))).sqrt();
            let db = |value: f64| (value > 0.0).then(|| 20.0 * value.log10());
            let start = plan.boundaries[index];
            let end = plan.boundaries[index + 1];
            windows.push(AnalysisLevelWindow {
                start: plan.timebase.tick_to_musical_position(start),
                end: plan.timebase.tick_to_musical_position(end),
                start_tick: start.0,
                end_tick: end.0,
                frames,
                rms_dbfs: db(rms),
                peak_dbfs: db(peak),
            });
        }
        sources.push(AnalysisLevelsSource {
            source: source.clone(),
            windows,
        });
    }
    let start = plan.boundaries[0];
    let end = *plan.boundaries.last().expect("grid contains endpoints");
    Ok(AnalysisLevelsResult {
        render_id: plan.render_id,
        grid: plan.grid,
        sample_rate: plan.sample_rate,
        start: plan.timebase.tick_to_musical_position(start),
        end: plan.timebase.tick_to_musical_position(end),
        start_tick: start.0,
        end_tick: end.0,
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn saved_timebase_controls_grid_and_stereo_levels_without_tail() {
        let root = std::env::temp_dir().join(format!(
            "riffra-levels-{}",
            riffra_control::new_instance_id()
        ));
        let directory = root.join("renders/render-test");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("master.wav");
        let frames = 300_u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + frames * 4).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&80_u32.to_le_bytes());
        wav.extend_from_slice(&320_u32.to_le_bytes());
        wav.extend_from_slice(&4_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(frames * 4).to_le_bytes());
        for frame in 0..frames {
            let left: i16 = if !(40..280).contains(&frame) {
                16_384
            } else {
                0
            };
            wav.extend_from_slice(&left.to_le_bytes());
            wav.extend_from_slice(&0_i16.to_le_bytes());
        }
        let bus_path = directory.join("bus.wav");
        fs::write(&path, &wav).unwrap();
        fs::write(&bus_path, &wav).unwrap();
        let timebase = ProjectTimebase {
            ppq: 10,
            tempo_changes: vec![
                riffra_core::TempoChange {
                    tick: 0,
                    bpm: 120.0,
                },
                riffra_core::TempoChange {
                    tick: 10,
                    bpm: 60.0,
                },
            ],
            time_signature_changes: vec![
                riffra_core::TimeSignatureChange {
                    tick: 0,
                    numerator: 4,
                    denominator: 4,
                },
                riffra_core::TimeSignatureChange {
                    tick: 20,
                    numerator: 3,
                    denominator: 8,
                },
            ],
        };
        let manifest = json!({"projectId":"project","createdAtMs":1,"sampleRate":80,"frames":frames,"captures":[{"target":{"kind":"master"},"name":"Master","assetId":"asset:00000000-0000-7000-8000-000000000001","path":path},{"target":{"kind":"bus","id":"bus:1"},"name":"Bus","assetId":"asset:00000000-0000-7000-8000-000000000002","path":bus_path}],"musicalRange":{"timebase":timebase,"startTick":0,"endTick":40}});
        fs::write(
            directory.join("render.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let params = AnalysisLevelsParams {
            render_id: "render-test".into(),
            grid: "1/4".parse().unwrap(),
            start: None,
            end: None,
            source: None,
        };
        let plan = prepare(&root, params.clone()).unwrap();
        let result = measure(plan, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(result.sources.len(), 2);
        let windows = &result.sources[0].windows;
        assert_eq!(
            windows
                .iter()
                .map(|window| window.frames)
                .collect::<Vec<_>>(),
            [40, 80, 80, 40, 40]
        );
        assert!((windows[0].rms_dbfs.unwrap() - -9.03089987).abs() < 1.0e-6);
        assert!((windows[0].peak_dbfs.unwrap() - -6.02059991).abs() < 1.0e-6);
        assert!(
            windows[1..]
                .iter()
                .all(|window| window.rms_dbfs.is_none() && window.peak_dbfs.is_none())
        );
        assert_eq!(
            result.sources[1]
                .windows
                .iter()
                .map(|window| window.frames)
                .collect::<Vec<_>>(),
            [40, 80, 80, 40, 40]
        );
        assert_eq!(
            windows
                .iter()
                .map(|window| (window.start_tick, window.end_tick))
                .collect::<Vec<_>>(),
            [(0, 10), (10, 20), (20, 30), (30, 35), (35, 40)]
        );
        let cropped = AnalysisLevelsParams {
            start: Some("1:1+1/2".parse().unwrap()),
            end: Some("1:2+1/2".parse().unwrap()),
            source: Some(LevelSource::Bus { id: "bus:1".into() }),
            ..params.clone()
        };
        let cropped = measure(
            prepare(&root, cropped).unwrap(),
            &AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(cropped.sources.len(), 1);
        assert_eq!(
            cropped.sources[0]
                .windows
                .iter()
                .map(|window| window.frames)
                .collect::<Vec<_>>(),
            [20, 40]
        );
        let cancelled = AtomicBool::new(true);
        assert!(
            measure(
                prepare(&root, params.clone()).unwrap(),
                &cancelled,
                |_, _| {}
            )
            .is_err()
        );
        let mut duplicate = manifest.clone();
        duplicate["captures"][1]["target"] = json!({"kind":"master"});
        fs::write(
            directory.join("render.json"),
            serde_json::to_vec(&duplicate).unwrap(),
        )
        .unwrap();
        assert!(
            prepare(&root, params.clone())
                .err()
                .unwrap()
                .contains("duplicate source")
        );
        let mut wrong_frames = manifest.clone();
        wrong_frames["frames"] = json!(299);
        fs::write(
            directory.join("render.json"),
            serde_json::to_vec(&wrong_frames).unwrap(),
        )
        .unwrap();
        assert!(
            prepare(&root, params.clone())
                .err()
                .unwrap()
                .contains("does not match")
        );
        let mut dense = manifest.clone();
        dense["musicalRange"]["timebase"]["ppq"] = json!(960);
        dense["musicalRange"]["timebase"]["tempoChanges"] = json!([{"tick":0,"bpm":120.0}]);
        dense["musicalRange"]["timebase"]["timeSignatureChanges"] =
            json!([{"tick":0,"numerator":4,"denominator":4}]);
        dense["musicalRange"]["endTick"] = json!(3840);
        fs::write(
            directory.join("render.json"),
            serde_json::to_vec(&dense).unwrap(),
        )
        .unwrap();
        let dense_params = AnalysisLevelsParams {
            grid: "1/3840".parse().unwrap(),
            ..params.clone()
        };
        assert!(
            prepare(&root, dense_params)
                .err()
                .unwrap()
                .contains("too many cells")
        );
        let mut legacy = manifest.clone();
        legacy.as_object_mut().unwrap().remove("musicalRange");
        fs::write(
            directory.join("render.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        assert!(
            prepare(&root, params)
                .err()
                .unwrap()
                .contains("no saved timebase")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
