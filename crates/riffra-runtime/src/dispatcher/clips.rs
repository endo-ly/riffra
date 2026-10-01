//! Audio and MIDI Clip command helpers.

use super::{DispatchError, HostDispatcher, parse_asset_id};
use crate::api::ControlOutput;
use crate::api::params::ClipAddAssetParams;
use riffra_core::application::{AudioAssetClipPlacement, MidiAssetClipPlacement};
use riffra_core::{AssetKind, TimelineTick};

impl HostDispatcher<'_> {
    pub(super) fn add_audio_clip<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        params: ClipAddAssetParams,
    ) -> Result<ControlOutput, DispatchError> {
        let asset_id = parse_asset_id(&params.asset_id)?;
        let asset = riffra_host::load(&self.data_root, &asset_id)
            .ok_or_else(|| format!("Audio Asset is not registered: {asset_id}"))?;
        if asset.kind != AssetKind::Audio {
            return Err(format!("Asset {asset_id} is not an audio Asset.").into());
        }
        let bytes = std::fs::read(&asset.content_location)
            .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
        let metadata = riffra_host::parse_wav(&bytes)?;
        if metadata.sample_rate == 0 || metadata.frame_count == 0 {
            return Err("Audio Asset has no usable frames.".into());
        }
        self.created(
            application.add_audio_asset_clip_with_created_ids(
                AudioAssetClipPlacement {
                    asset_id,
                    name: params.name,
                    start_tick: params.start_tick.map(TimelineTick),
                    track_id: params.track_id,
                    sample_rate: metadata.sample_rate,
                    source_frames: metadata.frame_count,
                },
                |id| riffra_host::load(&self.data_root, id).is_some(),
            )?,
            application,
        )
    }

    pub(super) fn add_midi_clip<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        params: ClipAddAssetParams,
    ) -> Result<ControlOutput, DispatchError> {
        let asset_id = parse_asset_id(&params.asset_id)?;
        let asset = riffra_host::load(&self.data_root, &asset_id)
            .ok_or_else(|| format!("MIDI Asset is not registered: {asset_id}"))?;
        if asset.kind != AssetKind::Midi {
            return Err(format!("Asset {asset_id} is not a MIDI Asset.").into());
        }
        let bytes = std::fs::read(&asset.content_location)
            .map_err(|error| format!("MIDI Asset could not be read: {error}"))?;
        let (duration_ticks, notes, events) = riffra_host::parse_smf(&bytes)?;
        self.created(
            application.add_midi_asset_clip_with_created_ids(MidiAssetClipPlacement {
                asset_id,
                name: params.name,
                start_tick: params.start_tick.map(TimelineTick),
                track_id: params.track_id,
                duration_ticks,
                notes,
                events,
            })?,
            application,
        )
    }

    pub(super) fn audio_source_frames<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        clip_id: &str,
    ) -> Result<u64, DispatchError> {
        let session = application.get_session()?;
        let clip = session
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| format!("Audio clip '{clip_id}' not found."))?;
        let asset = riffra_host::load(&self.data_root, &clip.asset_id)
            .ok_or_else(|| format!("Audio Asset is not registered: {}", clip.asset_id))?;
        let bytes = std::fs::read(&asset.content_location)
            .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
        Ok(riffra_host::parse_wav(&bytes)?.frame_count)
    }
}

#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use crate::test_support::{command, mutated_session};
    use riffra_host::now_ms;
    use serde_json::json;
    use std::fs;

    #[test]
    fn clearing_midi_notes_preserves_clip_and_is_undoable() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-clear-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let track = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Keys","kind":"instrument"})),
                None,
            )
            .unwrap();
        let track_id = mutated_session(&track).arrangement.tracks[0].id.clone();
        let created = dispatcher
            .dispatch(
                command(
                    "midi-clip.create",
                    json!({
                        "trackId": track_id,
                        "startTick": 480,
                        "durationTicks": 1920,
                        "name": "Lead"
                    }),
                ),
                None,
            )
            .unwrap();
        let original_clip = mutated_session(&created).arrangement.midi_clips[0].clone();
        let clip_id = original_clip.id.clone();
        dispatcher
            .dispatch(
                command(
                    "midi-note.insert",
                    json!({
                        "clipId": clip_id,
                        "notes": [{
                            "pitch": 60,
                            "startTick": 0,
                            "durationTicks": 480,
                            "velocity": 100,
                            "channel": 1
                        }]
                    }),
                ),
                None,
            )
            .unwrap();

        let cleared = dispatcher
            .dispatch(command("midi-note.clear", json!({"clipId": clip_id})), None)
            .unwrap();
        let session = mutated_session(&cleared);
        let cleared_clip = &session.arrangement.midi_clips[0];
        assert!(cleared_clip.notes.is_empty());
        assert_eq!(cleared_clip.id, original_clip.id);
        assert_eq!(cleared_clip.name, original_clip.name);
        assert_eq!(cleared_clip.start_tick, original_clip.start_tick);
        assert_eq!(cleared_clip.duration_ticks, original_clip.duration_ticks);

        let undone = dispatcher
            .dispatch(command("undo", json!({})), None)
            .unwrap();
        assert_eq!(
            mutated_session(&undone).arrangement.midi_clips[0]
                .notes
                .len(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }
}
