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
                    let base_dir = built_in_instruments.resolve(preset_id).map_or_else(
                        |_| built_in_instruments.root().to_path_buf(),
                        |definition| definition.base_dir.clone(),
                    );
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
                TrackInstrumentSource::Internal {
                    definition_json,
                    resource:
                        InternalInstrumentResource::UserSnapshot {
                            instrument_id,
                            snapshot_id,
                        },
                } => {
                    let base_dir = data_root.join("project-instruments").join(snapshot_id);
                    serde_json::json!({
                        "id": instrument.id,
                        "name": instrument.name,
                        "type": "internal",
                        "bypassed": instrument.bypassed,
                        "resourceType": "userSnapshot",
                        "instrumentId": instrument_id,
                        "snapshotId": snapshot_id,
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
                        "takeVariant": clip.take_variant,
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

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{
        AssetKind, AudioClip, AudioTakeVariant, TimelineTick, Track, TrackInstrument,
    };
    use std::fs;

    #[test]
    fn projects_audio_take_variants_to_the_native_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "riffra-runtime-snapshot-take-variant-{}-{}",
            std::process::id(),
            riffra_control::new_instance_id()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("take.wav");
        fs::write(&source, b"test audio").unwrap();
        let asset_id = crate::asset::register(
            &root,
            AssetKind::Audio,
            "take",
            source.to_str().unwrap(),
            None,
        )
        .unwrap();

        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:audio".into(), "Audio".into()));
        for (id, variant) in [
            ("clip:raw", AudioTakeVariant::Raw),
            ("clip:processed", AudioTakeVariant::Processed),
        ] {
            let mut clip = AudioClip::full_source(
                id.into(),
                id.into(),
                "track:audio".into(),
                asset_id.clone(),
                TimelineTick(0),
                48_000,
                480,
            );
            clip.take_variant = variant;
            session.arrangement.audio_clips.push(clip);
        }

        let snapshot = runtime_timeline_snapshot(
            &root,
            crate::test_support::empty_built_in_catalog(),
            &session,
        );
        let clips = snapshot["tracks"][0]["audioClips"].as_array().unwrap();

        assert_eq!(clips[0]["takeVariant"], serde_json::json!("raw"));
        assert_eq!(clips[1]["takeVariant"], serde_json::json!("processed"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn projects_user_snapshot_with_project_snapshot_base_dir() {
        let root = std::env::temp_dir().join(format!(
            "riffra-runtime-snapshot-user-instrument-{}-{}",
            std::process::id(),
            riffra_control::new_instance_id()
        ));
        let instrument_id = format!("user:{}", riffra_control::new_instance_id());
        let snapshot_id = riffra_control::new_instance_id();
        let mut session = CreativeSession::new(1);
        let mut track = Track::instrument("track:instrument".into(), "Instrument".into());
        track.instrument = Some(
            TrackInstrument::user_snapshot(
                "slot:instrument".into(),
                "User Instrument".into(),
                instrument_id.clone(),
                snapshot_id.clone(),
                r#"{"version":1}"#.into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks.push(track);

        let snapshot = runtime_timeline_snapshot(
            &root,
            crate::test_support::empty_built_in_catalog(),
            &session,
        );
        let instrument = &snapshot["tracks"][0]["instrument"];
        assert_eq!(
            instrument["resourceType"],
            serde_json::json!("userSnapshot")
        );
        assert_eq!(instrument["instrumentId"], serde_json::json!(instrument_id));
        assert_eq!(
            instrument["snapshotId"],
            serde_json::json!(snapshot_id.clone())
        );
        assert_eq!(
            instrument["definitionBaseDir"],
            serde_json::json!(
                root.join("project-instruments")
                    .join(snapshot_id)
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }
}
