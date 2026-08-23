//! MIDI file placement adapter.

use super::*;

pub fn add_midi_clip(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
) -> Result<crate::model::ArrangementMutationResult, String> {
    let source_asset = asset::load(context.data_root, &asset_id)
        .ok_or_else(|| format!("MIDI Asset is not registered: {asset_id}"))?;
    if source_asset.kind != AssetKind::Midi {
        return Err(format!("Asset {asset_id} is not a MIDI Asset."));
    }
    let bytes = fs::read(&source_asset.content_location)
        .map_err(|error| format!("MIDI Asset could not be read: {error}"))?;
    let (duration_ticks, notes, events) = riffra_host::parse_smf(&bytes)?;
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .add_midi_asset_clip(MidiAssetClipPlacement {
                asset_id,
                name,
                start_tick,
                track_id,
                duration_ticks,
                notes,
                events,
            })
    })?;
    Ok(crate::session::commit::arrangement_mutation_result(
        context, committed,
    ))
}
