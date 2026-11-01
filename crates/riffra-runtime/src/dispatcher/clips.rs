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
        let asset = riffra_host::load(&self.data_root, &asset_id)?
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
                |id| id == &asset.id,
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
        let asset = riffra_host::load(&self.data_root, &asset_id)?
            .ok_or_else(|| format!("MIDI Asset is not registered: {asset_id}"))?;
        if asset.kind != AssetKind::Midi {
            return Err(format!("Asset {asset_id} is not a MIDI Asset.").into());
        }
        let bytes = std::fs::read(&asset.content_location)
            .map_err(|error| format!("MIDI Asset could not be read: {error}"))?;
        let ppq = application.get_session()?.arrangement.timebase.ppq;
        let (duration_ticks, notes, events) = riffra_host::parse_smf(&bytes, Some(ppq))?;
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
        let asset = riffra_host::load(&self.data_root, &clip.asset_id)?
            .ok_or_else(|| format!("Audio Asset is not registered: {}", clip.asset_id))?;
        let bytes = std::fs::read(&asset.content_location)
            .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
        Ok(riffra_host::parse_wav(&bytes)?.frame_count)
    }
}

#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use crate::test_support::{command, mutated_session, output_value};
    use riffra_host::now_ms;
    use serde_json::json;
    use std::fs;

    #[test]
    fn imports_midi_with_invalid_lyrics_using_project_ppq() {
        let root = std::env::temp_dir().join(format!(
            "riffra-midi-ppq-{}",
            riffra_control::new_instance_id()
        ));
        let store = riffra_host::ProjectStore::new(&root);
        let mut initialized = store.initialize().unwrap();
        initialized.loaded.session.arrangement.timebase.ppq = 3360;
        initialized
            .storage
            .save(&initialized.loaded.session)
            .unwrap();
        let track = [
            0, 0xff, 5, 1, 0xff, 0, 0x90, 60, 100, 0x83, 0x60, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
        ];
        let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk".to_vec();
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&track);
        let path = root.join("voice.mid");
        fs::write(&path, bytes).unwrap();
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let imported = dispatcher
            .dispatch(command("asset.import-midi", json!({"path":path})), None)
            .unwrap();
        let asset_id = output_value(&imported);
        let placed = dispatcher
            .dispatch(
                command(
                    "midi-clip.add-asset",
                    json!({"assetId":asset_id,"name":"Voice"}),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&placed);
        let clip = &session.arrangement.midi_clips[0];
        assert_eq!(clip.duration_ticks, 3360);
        assert_eq!(
            (
                clip.notes[0].start_tick.0,
                clip.notes[0].duration_ticks,
                clip.notes[0].lyric.as_deref()
            ),
            (0, 3360, None)
        );
        drop(dispatcher);
        let _ = fs::remove_dir_all(root);
    }

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
        let pasted = dispatcher
            .dispatch(
                command(
                    "clip.paste",
                    json!({"midiClipIds":[clip_id], "startTick":3840}),
                ),
                None,
            )
            .unwrap();
        assert_eq!(mutated_session(&pasted).arrangement.midi_clips.len(), 2);
        let removed = dispatcher
            .dispatch(
                command("clip.remove", json!({"midiClipIds":[clip_id]})),
                None,
            )
            .unwrap();
        assert_eq!(mutated_session(&removed).arrangement.midi_clips.len(), 1);
        assert!(
            dispatcher
                .dispatch(command("clip.remove", json!({})), None)
                .is_err()
        );
        assert!(
            dispatcher
                .dispatch(command("clip.paste", json!({"startTick":0})), None)
                .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }
}
