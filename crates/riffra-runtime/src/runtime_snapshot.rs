use crate::asset;
use riffra_core::{CreativeSession, DeviceKind};
use std::path::{Path, PathBuf};

/// Builds the device-independent projection consumed by the native graph.
pub fn runtime_timeline_snapshot(data_root: &Path, session: &CreativeSession) -> serde_json::Value {
    let arrangement = &session.arrangement;
    let mut unavailable_clip_ids = Vec::new();
    let mut missing_device_ids = Vec::new();
    let tracks = arrangement
        .tracks
        .iter()
        .map(|track| {
            let mut runtime_instrument = track.instrument.clone();
            let mut runtime_rack = track.rack.clone();
            for device in runtime_instrument
                .iter_mut()
                .chain(runtime_rack.devices.iter_mut())
                .filter(|device| device.kind == DeviceKind::Plugin)
            {
                if !device.disabled_placeholder
                    && device
                        .path
                        .as_deref()
                        .is_none_or(|path| !PathBuf::from(path).exists())
                {
                    missing_device_ids.push(device.id.clone());
                    device.disabled_placeholder = true;
                }
            }
            let audio_clips = arrangement
                .audio_clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
                .filter_map(|clip| {
                    let path = asset::resolve_content_location(data_root, &clip.asset_id)?;
                    Some(serde_json::json!({
                        "clipId": clip.id,
                        "path": path,
                        "sourceSampleRate": clip.source_sample_rate,
                        "sourceStartFrame": clip.source_range.start,
                        "sourceEndFrame": clip.source_range.end,
                        "durationFrames": clip.timeline_duration.frames,
                        "durationSampleRate": clip.timeline_duration.sample_rate,
                        "startTick": clip.start_tick.0,
                        "fadeInFrames": clip.fade_in.frames,
                        "fadeOutFrames": clip.fade_out.frames,
                        "fadeShape": clip.fade_shape.as_code(),
                        "gainDb": clip.gain_db,
                        "pan": clip.pan,
                        "loopEnabled": clip.loop_enabled,
                        "muted": clip.muted,
                    }))
                })
                .collect::<Vec<_>>();
            for clip in arrangement
                .audio_clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
            {
                if asset::resolve_content_location(data_root, &clip.asset_id).is_none() {
                    unavailable_clip_ids.push(clip.id.clone());
                }
            }
            let midi_clips = arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
                .collect::<Vec<_>>();
            let automation = arrangement
                .automation_lanes
                .iter()
                .filter(|lane| lane.track_id == track.id)
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": track.id,
                "name": track.name,
                "kind": track.kind,
                "gainDb": track.gain_db,
                "pan": track.pan,
                "muted": track.muted,
                "solo": track.solo,
                "armed": track.armed,
                "monitoring": track.monitoring,
                "audioInput": track.audio_input,
                "midiInput": track.midi_input,
                "instrument": runtime_instrument,
                "rack": runtime_rack,
                "audioClips": audio_clips,
                "midiClips": midi_clips,
                "automation": automation,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "revision": arrangement.revision,
        "timebase": arrangement.timebase,
        "loopRange": arrangement.loop_range,
        "punchRange": arrangement.punch_range,
        "metronomeEnabled": session.settings.metronome_enabled,
        "tracks": tracks,
        "unavailableClipIds": unavailable_clip_ids,
        "missingDeviceIds": missing_device_ids,
    })
}
