use riffra_core::{CreativeSession, Track};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn reaches_mix(
    session: &CreativeSession,
    track: &Track,
    selected: Option<&str>,
) -> bool {
    if track.muted || selected.is_some_and(|id| id != track.id) {
        return false;
    }
    let group = track.output_bus_id.as_ref().and_then(|id| {
        session
            .arrangement
            .mixer
            .buses
            .iter()
            .find(|bus| &bus.id == id)
    });
    let has_solo = selected.is_some()
        || session.arrangement.tracks.iter().any(|track| track.solo)
        || session.arrangement.mixer.buses.iter().any(|bus| bus.solo);
    let solo = selected.is_some() || track.solo;
    let main = group.is_none_or(|bus| !bus.muted)
        && (!has_solo || solo || group.is_some_and(|bus| bus.solo && selected.is_none()));
    main || session.arrangement.mixer.sends.iter().any(|send| {
        send.track_id == track.id
            && send.enabled
            && session.arrangement.mixer.buses.iter().any(|bus| {
                bus.id == send.return_bus_id
                    && !bus.muted
                    && (!has_solo || solo || (bus.solo && selected.is_none()))
            })
    })
}

pub(super) fn silent_singing_warnings(
    session: &CreativeSession,
    track: &Track,
    path: &Path,
    start_tick: u64,
    end_tick: u64,
    cancelled: Option<&AtomicBool>,
) -> Result<Vec<String>, String> {
    let ranges = singing_ranges(session, track, start_tick, end_tick);
    if ranges.is_empty() {
        return Ok(Vec::new());
    }
    let wav = riffra_host::read_wav_structure(path)?;
    let width = usize::from(wav.bits_per_sample / 8);
    let frame_bytes = usize::from(wav.channels) * width;
    if width == 0 || frame_bytes == 0 {
        return Err("vocal WAV has an invalid format".into());
    }
    let timebase = &session.arrangement.timebase;
    let base = timebase.ticks_to_seconds(start_tick);
    let mut file =
        File::open(path).map_err(|error| format!("vocal WAV could not be opened: {error}"))?;
    let mut warnings = Vec::new();
    let mut buffer = vec![0; frame_bytes * 1024];
    for (start, end) in ranges {
        let first = (((timebase.ticks_to_seconds(start) - base) * f64::from(wav.sample_rate))
            .floor() as u64)
            .min(wav.frame_count);
        let last = (((timebase.ticks_to_seconds(end) - base) * f64::from(wav.sample_rate)).ceil()
            as u64)
            .min(wav.frame_count);
        if last <= first {
            continue;
        }
        file.seek(SeekFrom::Start(
            wav.data_offset as u64 + first * frame_bytes as u64,
        ))
        .map_err(|error| format!("vocal WAV could not be read: {error}"))?;
        let mut remaining = last - first;
        let mut audible = false;
        while remaining > 0 && !audible {
            if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                return Err("Timeline render was cancelled.".into());
            }
            let frames = remaining.min(1024) as usize;
            let bytes = frames * frame_bytes;
            file.read_exact(&mut buffer[..bytes])
                .map_err(|error| format!("vocal WAV could not be read: {error}"))?;
            for sample in buffer[..bytes].chunks_exact(width) {
                if crate::analysis::decode_sample(sample, wav.format, wav.bits_per_sample)?.abs()
                    > 1.0e-7
                {
                    audible = true;
                    break;
                }
            }
            remaining -= frames as u64;
        }
        if !audible {
            warnings.push(format!("VoiSona track {} ({}) has singing notes but no detectable audio at ticks [{start}, {end}).", track.name, track.id));
        }
    }
    Ok(warnings)
}

pub(super) fn singing_ranges(
    session: &CreativeSession,
    track: &Track,
    start_tick: u64,
    end_tick: u64,
) -> Vec<(u64, u64)> {
    if track.muted {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    for clip in session
        .arrangement
        .midi_clips
        .iter()
        .filter(|clip| clip.track_id == track.id && !clip.muted)
    {
        for note in clip.notes.iter().filter(|note| {
            note.lyric
                .as_deref()
                .is_some_and(|lyric| !lyric.trim().is_empty())
        }) {
            let start = clip.start_tick.0.saturating_add(note.start_tick.0);
            let end = start.saturating_add(note.duration_ticks);
            let start = start.max(start_tick);
            let end = end.min(end_tick);
            if start < end {
                ranges.push((start, end));
            }
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{MidiNote, TimelineTick};

    #[test]
    fn reports_only_silent_singing_in_the_rendered_range() {
        let mut session = super::super::tests::session_with_clips();
        let clip = &mut session.arrangement.midi_clips[0];
        clip.start_tick = TimelineTick(0);
        let note = |id: &str, start, lyric: Option<&str>| MidiNote {
            id: id.into(),
            note: 60,
            start_tick: TimelineTick(start),
            duration_ticks: 480,
            velocity: 100,
            channel: 1,
            lyric: lyric.map(str::to_owned),
        };
        clip.notes = vec![
            note("silent", 0, Some("あ")),
            note("audible", 480, Some("い")),
            note("instrumental", 960, None),
        ];
        let path = std::env::temp_dir().join(format!(
            "riffra-vocal-{}.wav",
            riffra_control::new_instance_id()
        ));
        let mut wav = super::super::tests::wav(1000, 1000);
        for sample in wav[44 + 250 * 4..44 + 500 * 4].as_chunks_mut::<2>().0 {
            sample.copy_from_slice(&1000_i16.to_le_bytes());
        }
        std::fs::write(&path, wav).unwrap();
        let track = &session.arrangement.tracks[0];

        let warnings = silent_singing_warnings(&session, track, &path, 0, 1440, None).unwrap();

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("[0, 480)"));
        assert!(
            silent_singing_warnings(&session, track, &path, 960, 1440, None)
                .unwrap()
                .is_empty()
        );
        let mut muted = track.clone();
        muted.muted = true;
        assert!(
            silent_singing_warnings(&session, &muted, &path, 0, 1440, None)
                .unwrap()
                .is_empty()
        );
        session
            .arrangement
            .tracks
            .push(riffra_core::Track::audio("other".into(), "Other".into()));
        session.arrangement.tracks[1].solo = true;
        assert!(!reaches_mix(&session, &session.arrangement.tracks[0], None));
        assert!(reaches_mix(
            &session,
            &session.arrangement.tracks[0],
            Some("instrument")
        ));
        std::fs::remove_file(path).unwrap();
    }
}
