//! music command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "music.midi-clip.create"
            | "music.note.insert"
            | "music.harmony.resolve"
            | "music.harmony.list"
            | "music.harmony.insert"
            | "music.harmony.update"
            | "music.harmony.remove"
            | "music.harmony.realize"
            | "music.phrase.insert"
            | "music.region.list"
            | "music.region.add"
            | "music.region.update"
            | "music.region.remove"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    _canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "music.midi-clip.create" => {
            let params: MusicalMidiClipCreateParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .create_musical_midi_clip(
                        &params.track_id,
                        params.start,
                        params.end,
                        params.name,
                    )?,
            )
        }
        "music.note.insert" => {
            let params: MusicalNoteInsertParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .insert_musical_notes(&params.clip_id, params.notes)?,
            )
        }
        "music.harmony.resolve" => {
            let params: MusicalHarmonyResolveParams = decode(request.params)?;
            dispatcher.value(
                "harmonyChord",
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .resolve_harmony_chord(&params.chord)?,
            )
        }
        "music.harmony.list" => dispatcher.value(
            "harmonyEvents",
            dispatcher
                .core
                .application(&dispatcher.storage)
                .list_harmony_events()?,
        ),
        "music.harmony.insert" => {
            let params: MusicalHarmonyInsertParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .insert_harmony_events(params.events)?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "music.harmony.update" => {
            let params: MusicalHarmonyUpdateParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_harmony_event(&params.event_id, params.patch)?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "music.harmony.remove" => {
            let params: MusicalHarmonyRemoveParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .remove_harmony_events(params.event_ids)?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "music.harmony.realize" => {
            let params: MusicalHarmonyRealizeParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .realize_harmony(
                        &params.clip_id,
                        HarmonyRealizeSelection {
                            start: params.start,
                            end: params.end,
                        },
                        ChordVoicingInput {
                            lowest_octave: params.lowest_octave.unwrap_or(3),
                        },
                        params.rhythm,
                        params.velocity,
                        params.channel,
                    )?,
                CanonicalMutationEffect::ProjectArrangement,
            )
        }
        "music.phrase.insert" => {
            let params: MusicalPhraseInsertParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .insert_phrase_pattern(
                        &params.clip_id,
                        params.pattern,
                        params.placements,
                        params.channel,
                    )?,
                CanonicalMutationEffect::ProjectArrangement,
            )
        }
        "music.region.list" => dispatcher.value(
            "regions",
            dispatcher
                .core
                .application(&dispatcher.storage)
                .list_regions()?,
        ),
        "music.region.add" => {
            let params: MusicalRegionAddParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .add_region(params.name, params.start, params.end)?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "music.region.update" => {
            let params: MusicalRegionUpdateParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_region(&params.region_id, params.name, params.start, params.end)?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "music.region.remove" => {
            let params: MusicalRegionIdParams = decode(request.params)?;
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .remove_region(&params.region_id)?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        _ => unreachable!("unsupported music command family"),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalMidiClipCreateParams {
    track_id: String,
    start: riffra_core::MusicalPosition,
    end: riffra_core::MusicalPosition,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalNoteInsertParams {
    clip_id: String,
    notes: Vec<MusicalMidiNoteInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyResolveParams {
    chord: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyInsertParams {
    events: Vec<HarmonyEventInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyUpdateParams {
    event_id: String,
    #[serde(flatten)]
    patch: HarmonyEventPatch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyRemoveParams {
    event_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyRealizeParams {
    clip_id: String,
    start: Option<riffra_core::MusicalPosition>,
    end: Option<riffra_core::MusicalPosition>,
    lowest_octave: Option<i8>,
    rhythm: Option<RhythmPattern>,
    velocity: Option<u8>,
    channel: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalPhraseInsertParams {
    clip_id: String,
    pattern: PhrasePattern,
    placements: Vec<PhrasePlacement>,
    channel: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalRegionAddParams {
    name: String,
    start: riffra_core::MusicalPosition,
    end: riffra_core::MusicalPosition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalRegionUpdateParams {
    region_id: String,
    name: Option<String>,
    start: Option<riffra_core::MusicalPosition>,
    end: Option<riffra_core::MusicalPosition>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalRegionIdParams {
    region_id: String,
}
