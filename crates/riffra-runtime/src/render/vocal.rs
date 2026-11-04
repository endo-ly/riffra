use crate::execution::{VocalDiagnostic, VocalRange};
use riffra_core::{CreativeSession, Track};

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

pub(super) fn ranges(
    session: &CreativeSession,
    track: &Track,
    start_tick: u64,
    end_tick: u64,
) -> Result<Vec<VocalRange>, String> {
    Ok(
        crate::voisona::singing_ranges(session, track, start_tick, end_tick)?
            .into_iter()
            .map(|(start_tick, end_tick)| VocalRange {
                start_tick,
                end_tick,
            })
            .collect(),
    )
}

pub(super) fn warnings_for_track(track: &Track, diagnostics: &[VocalDiagnostic]) -> Vec<String> {
    diagnostics.iter().filter(|diagnostic| diagnostic.track_id == track.id)
        .flat_map(|diagnostic| diagnostic.ranges.iter())
        .map(|range| format!("VoiSona track {} ({}) has singing notes but no detectable audio at ticks [{}, {}).", track.name, track.id, range.start_tick, range.end_tick))
        .collect()
}

pub(super) fn warnings(
    session: &CreativeSession,
    diagnostics: &[VocalDiagnostic],
) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    for diagnostic in diagnostics {
        let track = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.id == diagnostic.track_id)
            .ok_or("render returned a vocal diagnostic for an unknown track")?;
        result.extend(warnings_for_track(track, std::slice::from_ref(diagnostic)));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_diagnostics_follow_mute_and_solo_routing() {
        let mut session = super::super::tests::session_with_clips();
        session
            .arrangement
            .tracks
            .push(Track::audio("other".into(), "Other".into()));
        session.arrangement.tracks[1].solo = true;
        let track = &session.arrangement.tracks[0];
        assert!(!reaches_mix(&session, track, None));
        assert!(reaches_mix(&session, track, Some("instrument")));
        let mut muted = track.clone();
        muted.muted = true;
        assert!(!reaches_mix(&session, &muted, Some("instrument")));
    }
}
