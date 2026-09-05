use crate::asset;
use crate::instrument::BuiltInInstrumentCatalog;
use riffra_core::{CreativeSession, InternalInstrumentResource, TrackInstrumentSource};
use std::path::{Path, PathBuf};

/// Builds the device-independent projection consumed by the native graph.
pub fn runtime_timeline_snapshot(
    data_root: &Path,
    built_in_instruments: &BuiltInInstrumentCatalog,
    session: &CreativeSession,
) -> serde_json::Value {
    let arrangement = &session.arrangement;
    let mut unavailable_clip_ids = Vec::new();
    let mut missing_device_ids = Vec::new();
    let tracks = arrangement
        .tracks
        .iter()
        .map(|track| {
            let mut runtime_rack = track.rack.clone();
            for device in runtime_rack
                .devices
                .iter_mut()
                .filter(|device| device.kind == riffra_core::DeviceKind::Plugin)
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
            let runtime_instrument = track.instrument.as_ref().map(|instrument| match &instrument
                .source
            {
                TrackInstrumentSource::Internal {
                    definition_json,
                    resource: InternalInstrumentResource::BuiltInPreset { preset_id },
                } => {
                    let base_dir = built_in_instruments.root().join(preset_id);
                    serde_json::json!({
                        "id": instrument.id,
                        "name": instrument.name,
                        "type": "internal",
                        "bypassed": instrument.bypassed,
                        "resourceType": "builtInPreset",
                        "presetId": preset_id,
                        "definitionJson": definition_json,
                        "definitionBaseDir": base_dir.to_string_lossy().into_owned(),
                    })
                }
                TrackInstrumentSource::Vst3 {
                    path,
                    parameter_values,
                    state_data,
                    disabled_placeholder,
                } => {
                    let runtime_disabled = *disabled_placeholder || !PathBuf::from(path).exists();
                    if !*disabled_placeholder && runtime_disabled {
                        missing_device_ids.push(instrument.id.clone());
                    }
                    serde_json::json!({
                        "id": instrument.id,
                        "name": instrument.name,
                        "type": "vst3",
                        "bypassed": instrument.bypassed,
                        "path": path,
                        "parameterValues": parameter_values,
                        "stateData": state_data,
                        "disabledPlaceholder": runtime_disabled,
                    })
                }
            });
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
