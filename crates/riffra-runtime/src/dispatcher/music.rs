//! Music-level command helpers.

use super::{DispatchError, HostDispatcher};
use crate::api::ControlOutput;
use crate::api::output::{PhrasePreview, PhrasePreviewNote};
use crate::api::params::PhrasePreviewParams;
use riffra_core::{MusicalPitch, ProjectTimebase};

impl<A> HostDispatcher<'_, A> {
    pub(super) fn phrase_preview(
        &self,
        timebase: ProjectTimebase,
        params: PhrasePreviewParams,
    ) -> Result<ControlOutput, DispatchError> {
        let resolved = self
            .core
            .application(&self.storage)
            .resolve_phrase_pattern(
                &params.clip_id,
                params.pattern,
                params.placements,
                params.channel,
            )?;
        let invalid =
            |error: riffra_core::DomainError| DispatchError::invalid_request(error.to_string());
        let notes = if params.include_notes {
            Some(
                resolved
                    .notes
                    .iter()
                    .map(|note| {
                        Ok(PhrasePreviewNote {
                            pitch: MusicalPitch::from_midi_pitch(note.pitch).map_err(invalid)?,
                            position: timebase.tick_to_musical_position(note.start_tick),
                            duration: timebase
                                .ticks_to_musical_duration(note.duration_ticks)
                                .map_err(invalid)?,
                            velocity: note.velocity,
                            channel: note.channel,
                        })
                    })
                    .collect::<Result<Vec<_>, DispatchError>>()?,
            )
        } else {
            None
        };
        Ok(ControlOutput::PhrasePreview(PhrasePreview {
            note_count: resolved.notes.len(),
            placement_count: resolved.placement_count,
            start: timebase.tick_to_musical_position(resolved.start_tick),
            end: timebase.tick_to_musical_position(resolved.end_tick),
            notes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use crate::test_support::{command as request, mutated_session, output_type, output_value};
    use riffra_control::ControlRequest;
    use riffra_host::now_ms;
    use serde_json::json;
    use std::fs;

    #[test]
    fn musical_commands_create_canonical_notes_and_regions() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-music-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let track = dispatcher
            .dispatch(
                request("track.add", json!({"name":"Keys","kind":"instrument"})),
                None,
            )
            .unwrap();
        let session = mutated_session(&track);
        let track_id = session.arrangement.tracks[0].id.clone();
        let created = dispatcher
            .dispatch(
                request(
                    "music.midi-clip.create",
                    json!({
                        "trackId": track_id,
                        "start": "5:1",
                        "end": "13:1",
                        "name": "Piano"
                    }),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&created);
        let clip_id = session.arrangement.midi_clips[0].id.clone();
        let invalid = dispatcher
            .dispatch_request(ControlRequest::new(
                "invalid-note",
                "music.note.insert",
                json!({
                    "clipId": clip_id,
                    "notes": [
                        {"pitch":"C4","position":"1:1","duration":"1/8","velocity":null}
                    ]
                }),
                None,
            ))
            .unwrap_err()
            .protocol_error();
        let details = invalid.details.clone().unwrap();
        assert_eq!(invalid.code, riffra_control::ErrorCode::InvalidRequest);
        assert_eq!(details["path"], "/notes/0/velocity");
        assert_eq!(details["index"], 0);
        assert!(details["value"].is_null());
        let region = dispatcher
            .dispatch(
                request(
                    "music.region.add",
                    json!({"name":"A'","start":"5:1","end":"13:1"}),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&region);
        assert_eq!(session.arrangement.regions[0].name, "A'");
        let inserted = dispatcher
            .dispatch(
                request(
                    "music.note.insert",
                    json!({
                        "clipId": clip_id,
                        "notes": [
                            {"pitch":"C4","position":"5:1","duration":"1/8"},
                            {"pitch":"E4","position":"5:1+1/2","duration":"1/8"},
                            {"pitch":"G4","position":"5:2","duration":"1/2","velocity":92},
                            {"pitch":"Bb4","position":"6:3+1/3","duration":"1/12"}
                        ]
                    }),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&inserted);
        let clip = &session.arrangement.midi_clips[0];
        assert_eq!(clip.start_tick, riffra_core::TimelineTick(15_360));
        assert_eq!(clip.duration_ticks, 30_720);
        assert_eq!(clip.notes.len(), 4);
        assert_eq!(clip.notes[0].note, 60);
        assert_eq!(clip.notes[0].start_tick, riffra_core::TimelineTick(0));
        assert_eq!(clip.notes[1].start_tick, riffra_core::TimelineTick(480));
        assert_eq!(clip.notes[2].note, 67);
        assert_eq!(clip.notes[3].note, 70);
        assert_eq!(clip.notes[3].start_tick, riffra_core::TimelineTick(6_080));
        assert_eq!(clip.notes[3].duration_ticks, 320);

        let note_id = clip.notes[0].id.clone();
        let listed = dispatcher
            .dispatch(
                request(
                    "music.note.list",
                    json!({
                        "clipId": clip_id,
                        "start": "5:1",
                        "end": "5:1+1/4"
                    }),
                ),
                None,
            )
            .unwrap();
        assert_eq!(output_type(&listed), "musicNotes");
        assert_eq!(output_value(&listed)["count"], 1);
        assert_eq!(output_value(&listed)["clips"][0]["notes"][0]["pitch"], "C4");
        assert!(
            output_value(&listed)["clips"][0]["notes"][0]
                .get("id")
                .is_none()
        );
        assert!(
            output_value(&listed)["clips"][0]["notes"][0]
                .get("startTick")
                .is_none()
        );
        assert!(
            output_value(&listed)["clips"][0]["notes"][0]
                .get("note")
                .is_none()
        );

        let fetched = dispatcher
            .dispatch(
                request(
                    "music.note.get",
                    json!({"clipId": clip_id, "noteId": note_id}),
                ),
                None,
            )
            .unwrap();
        assert_eq!(output_type(&fetched), "musicNote");
        assert_eq!(output_value(&fetched)["position"], "5:1");

        dispatcher
            .dispatch(
                request(
                    "music.note.update",
                    json!({
                        "clipId": clip_id,
                        "noteId": note_id,
                        "position": "5:2",
                        "duration": "1/4"
                    }),
                ),
                None,
            )
            .unwrap();
        let updated = dispatcher
            .dispatch(
                request(
                    "music.note.get",
                    json!({"clipId": clip_id, "noteId": note_id}),
                ),
                None,
            )
            .unwrap();
        assert_eq!(output_value(&updated)["position"], "5:2");
        assert_eq!(output_value(&updated)["duration"], "1/4");

        let transformed = dispatcher
            .dispatch(
                request(
                    "music.note.transform",
                    json!({
                        "clipId": clip_id,
                        "start": "5:1",
                        "end": "5:2",
                        "pitch": "E4",
                        "velocityOffset": 4
                    }),
                ),
                None,
            )
            .unwrap();
        let transformed = mutated_session(&transformed);
        assert_eq!(transformed.arrangement.midi_clips[0].notes[1].velocity, 104);

        dispatcher
            .dispatch(
                request(
                    "music.midi-clip.resize",
                    json!({"clipId": clip_id, "end": "12:1"}),
                ),
                None,
            )
            .unwrap();
        dispatcher
            .dispatch(
                request(
                    "music.note.remove",
                    json!({"clipId": clip_id, "noteId": note_id}),
                ),
                None,
            )
            .unwrap();
        assert!(
            dispatcher
                .dispatch(
                    request(
                        "music.note.get",
                        json!({"clipId": clip_id, "noteId": note_id}),
                    ),
                    None
                )
                .is_err()
        );

        let listed = dispatcher
            .dispatch(request("music.region.list", json!({})), None)
            .unwrap();
        assert_eq!(output_type(&listed), "regions");
        assert_eq!(output_value(&listed).as_array().unwrap().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn harmony_and_phrase_commands_use_music_level_contracts() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-harmony-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let track = dispatcher
            .dispatch(
                request("track.add", json!({"name":"Keys","kind":"instrument"})),
                None,
            )
            .unwrap();
        let session = mutated_session(&track);
        let clip = dispatcher
            .dispatch(
                request(
                    "music.midi-clip.create",
                    json!({
                        "trackId": session.arrangement.tracks[0].id,
                        "start": "1:1",
                        "end": "3:1"
                    }),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&clip);
        let clip_id = session.arrangement.midi_clips[0].id.clone();

        let resolved = dispatcher
            .dispatch(
                request("music.harmony.resolve", json!({"chord":"G7(b9,#11)/F"})),
                None,
            )
            .unwrap();
        assert_eq!(output_type(&resolved), "harmonyChord");
        assert_eq!(output_value(&resolved)["root"], "G");
        assert_eq!(output_value(&resolved)["bass"], "F");
        assert_eq!(
            output_value(&resolved)["tones"],
            json!(["G", "B", "D", "F", "Ab", "C#"])
        );

        let inserted = dispatcher
            .dispatch(request(
                "music.harmony.insert",
                json!({
                    "events": [
                        {"start":"1:1","end":"2:1","chord":"C/E"},
                        {"start":"2:1","end":"3:1","pitches":["Bb","C","E"],"bass":"F","label":"cluster"}
                    ]
                }),
            ), None)
            .unwrap();
        let session = mutated_session(&inserted);
        let harmony_ids = session
            .arrangement
            .harmony_events
            .iter()
            .map(|event| event.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(harmony_ids.len(), 2);

        let listed = dispatcher
            .dispatch(request("music.harmony.list", json!({})), None)
            .unwrap();
        assert_eq!(output_type(&listed), "harmonyEvents");
        assert_eq!(output_value(&listed)[0]["start"], "1:1");
        assert!(output_value(&listed)[0].get("startTick").is_none());

        dispatcher
            .dispatch(
                request(
                    "music.harmony.realize",
                    json!({"clipId":clip_id,"start":"1:1","end":"3:1"}),
                ),
                None,
            )
            .unwrap();
        let updated = dispatcher
            .dispatch(
                request(
                    "music.harmony.update",
                    json!({"eventId": harmony_ids[0], "chord":"Dm9"}),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&updated);
        assert_eq!(session.arrangement.harmony_events[0].chord.name, "Dm9");

        let phrase = dispatcher
            .dispatch(
                request(
                    "music.phrase.preview",
                    json!({
                        "clipId": clip_id,
                        "pattern": {
                            "length":"1/4",
                            "notes":[
                                {"offset":"0/1","duration":"1/8","semitones":0},
                                {"offset":"1/8","duration":"1/8","semitones":2}
                            ]
                        },
                        "placements":[{"position":"1:1","anchor":"C4","repeats":1}],
                        "includeNotes": true
                    }),
                ),
                None,
            )
            .unwrap();
        assert_eq!(output_type(&phrase), "phrasePreview");
        assert_eq!(output_value(&phrase)["noteCount"], 2);
        assert_eq!(output_value(&phrase)["placementCount"], 1);
        assert_eq!(output_value(&phrase)["notes"].as_array().unwrap().len(), 2);
        assert_eq!(output_value(&phrase)["notes"][0]["pitch"], "C4");

        let phrase = dispatcher
            .dispatch(
                request(
                    "music.phrase.insert",
                    json!({
                        "clipId": clip_id,
                        "pattern": {
                            "length":"1/4",
                            "notes":[
                                {"offset":"0/1","duration":"1/8","semitones":0},
                                {"offset":"1/8","duration":"1/8","semitones":2}
                            ]
                        },
                        "placements":[{"position":"1:1","anchor":"C4","repeats":1}]
                    }),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&phrase);
        assert_eq!(session.arrangement.midi_clips[0].notes.len(), 9);

        dispatcher
            .dispatch(
                request(
                    "music.harmony.remove",
                    json!({"eventIds":[harmony_ids[0], harmony_ids[1]]}),
                ),
                None,
            )
            .unwrap();
        let listed = dispatcher
            .dispatch(request("music.harmony.list", json!({})), None)
            .unwrap();
        assert!(output_value(&listed).as_array().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
