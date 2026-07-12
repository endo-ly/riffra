use crate::model::ScratchSession;
use serde::Serialize;
use std::{fs, path::Path};

const TICKS_PER_QUARTER: u64 = 480;
const TEMPO_BPM: u64 = 120;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiExportResult {
    pub id: String,
    pub path: String,
    pub note_count: usize,
    pub clip_count: usize,
    pub state: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug)]
struct MidiMessage {
    tick: u64,
    order: u8,
    status: u8,
    note: u8,
    velocity: u8,
}

pub fn export(
    data_root: &Path,
    session: &ScratchSession,
    created_at_ms: u64,
) -> Result<MidiExportResult, String> {
    let mut messages = Vec::new();
    for clip in session.midi_clips.iter().filter(|clip| !clip.muted) {
        for note in &clip.notes {
            let start_ms = clip.start_ms.saturating_add(note.start_ms);
            let end_ms = start_ms.saturating_add(note.duration_ms.max(1));
            let channel = note.channel.saturating_sub(1).min(15);
            let start_tick = ms_to_ticks(start_ms);
            let end_tick = ms_to_ticks(end_ms).max(start_tick.saturating_add(1));
            messages.push(MidiMessage {
                tick: start_tick,
                order: 1,
                status: 0x90 | channel,
                note: note.note.min(127),
                velocity: note.velocity.clamp(1, 127),
            });
            messages.push(MidiMessage {
                tick: end_tick,
                order: 0,
                status: 0x80 | channel,
                note: note.note.min(127),
                velocity: 0,
            });
        }
    }
    if messages.is_empty() {
        return Err("There are no audible MIDI notes to export.".into());
    }
    let note_count = messages.len() / 2;
    messages.sort_by_key(|message| (message.tick, message.order));
    let mut track = Vec::new();
    track.extend_from_slice(&[0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]);
    track.extend_from_slice(&[0x00, 0xff, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]);
    let mut previous_tick = 0_u64;
    for message in messages {
        track.extend_from_slice(&write_var_len(message.tick.saturating_sub(previous_tick)));
        track.extend_from_slice(&[message.status, message.note, message.velocity]);
        previous_tick = message.tick;
    }
    track.extend_from_slice(&[0x00, 0xff, 0x2f, 0x00]);

    let directory = data_root
        .join("exports")
        .join(format!("midi-{created_at_ms}"));
    fs::create_dir_all(&directory)
        .map_err(|error| format!("MIDI export folder could not be created: {error}"))?;
    let path = directory.join("session.mid");
    let partial = directory.join("session.mid.partial");
    let mut file = Vec::with_capacity(22 + track.len());
    file.extend_from_slice(b"MThd");
    file.extend_from_slice(&6_u32.to_be_bytes());
    file.extend_from_slice(&0_u16.to_be_bytes());
    file.extend_from_slice(&1_u16.to_be_bytes());
    file.extend_from_slice(&(TICKS_PER_QUARTER as u16).to_be_bytes());
    file.extend_from_slice(b"MTrk");
    file.extend_from_slice(&(track.len() as u32).to_be_bytes());
    file.extend_from_slice(&track);
    fs::write(&partial, &file)
        .map_err(|error| format!("MIDI export could not be written: {error}"))?;
    fs::rename(&partial, &path)
        .map_err(|error| format!("MIDI export could not be finalized: {error}"))?;
    let result = MidiExportResult {
        id: format!("midi-export:{created_at_ms}"),
        path: path.to_string_lossy().into_owned(),
        note_count,
        clip_count: session.midi_clips.iter().filter(|clip| !clip.muted).count(),
        state: "completed".into(),
        message: "MIDI clips exported as a standard Type 0 file; source session remains unchanged."
            .into(),
    };
    fs::write(
        directory.join("export.json"),
        serde_json::to_vec_pretty(&result).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("MIDI export manifest could not be saved: {error}"))?;
    Ok(result)
}

fn ms_to_ticks(ms: u64) -> u64 {
    ms.saturating_mul(TICKS_PER_QUARTER)
        .saturating_mul(TEMPO_BPM)
        / 60_000
}

fn write_var_len(mut value: u64) -> Vec<u8> {
    let mut bytes = [0_u8; 10];
    let mut index = bytes.len() - 1;
    bytes[index] = (value & 0x7f) as u8;
    while {
        value >>= 7;
        value > 0
    } {
        index -= 1;
        bytes[index] = ((value & 0x7f) as u8) | 0x80;
    }
    bytes[index..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{MidiClip, MidiNote, ScratchSession},
        storage::now_ms,
    };

    #[test]
    fn exports_standard_midi_with_a_note_pair() {
        let root = std::env::temp_dir().join(format!("riffra-midi-{}", now_ms()));
        let mut session = ScratchSession::new(now_ms());
        session.midi_clips.push(MidiClip {
            id: "midi:test".into(),
            name: "Test".into(),
            start_ms: 0,
            duration_ms: 250,
            notes: vec![MidiNote {
                id: "note:1".into(),
                note: 60,
                start_ms: 0,
                duration_ms: 250,
                velocity: 100,
                channel: 1,
            }],
            muted: false,
        });
        let result = export(&root, &session, 1).unwrap();
        let bytes = fs::read(&result.path).unwrap();
        assert_eq!(&bytes[0..4], b"MThd");
        assert_eq!(&bytes[8..10], &0_u16.to_be_bytes());
        assert_eq!(result.note_count, 1);
        let _ = fs::remove_dir_all(root);
    }
}
