//! Bundle conversion and ownership of resources until Project activation.

use crate::instrument::UserInstrumentStore;
use riffra_core::{
    CreativeSession, InstrumentControlEvent, InstrumentControlEventKind, LoudnessMastering,
    MidiClip, MidiNote, MixdownSettings, PanLaw, ProjectTimebase, TIMELINE_PPQ, TempoChange,
    TimeSignatureChange, TimelineTick, Track, TrackInstrument,
};
use riffra_host::ProjectStore;
use riffra_host::sonalloy_bundle::{self, PatternEvent};
use std::path::{Path, PathBuf};

pub(crate) struct ImportedProject<'a> {
    store: &'a ProjectStore,
    pub(crate) project_id: Option<String>,
    snapshots: Vec<PathBuf>,
}

impl<'a> ImportedProject<'a> {
    pub(crate) fn prepare(
        store: &'a ProjectStore,
        data_root: &Path,
        sonalloy: &Path,
        path: &Path,
    ) -> Result<Self, String> {
        let bundle = sonalloy_bundle::read(path).map_err(|error| error.to_string())?;
        let mut import = Self {
            store,
            project_id: None,
            snapshots: Vec::new(),
        };
        let instruments = UserInstrumentStore::new(data_root, sonalloy);
        let (timebase_part, first) = bundle
            .patterns
            .iter()
            .enumerate()
            .max_by_key(|(_, pattern)| pattern.length_ticks)
            .expect("reader validated nonempty demo");
        let timebase_location = format!(
            "parts[{timebase_part}].pattern (part {})",
            bundle.demo.parts[timebase_part].id
        );
        let mut session = CreativeSession::new(riffra_host::now_ms());
        session.project_name = bundle.demo.name.clone();
        session.arrangement.timebase = ProjectTimebase {
            ppq: TIMELINE_PPQ,
            tempo_changes: first
                .tempo_changes
                .iter()
                .enumerate()
                .map(|(i, point)| {
                    Ok(TempoChange {
                        tick: convert_tick(
                            point.tick,
                            first.ticks_per_beat,
                            &format!("{timebase_location}.tempo_changes[{i}].tick"),
                        )?,
                        bpm: point.bpm,
                    })
                })
                .collect::<Result<_, String>>()?,
            time_signature_changes: first
                .time_signature_changes
                .iter()
                .enumerate()
                .map(|(i, point)| {
                    Ok(TimeSignatureChange {
                        tick: convert_tick(
                            point.tick,
                            first.ticks_per_beat,
                            &format!("{timebase_location}.time_signature_changes[{i}].tick"),
                        )?,
                        numerator: point.numerator,
                        denominator: point.denominator,
                    })
                })
                .collect::<Result<_, String>>()?,
        };
        let track_ids: Vec<_> = bundle
            .demo
            .parts
            .iter()
            .map(|_| riffra_control::new_instance_id())
            .collect();
        let mut musical_end_tick = 0;
        let mut used_channels: std::collections::BTreeSet<_> = bundle
            .demo
            .parts
            .iter()
            .filter_map(|part| part.midi_channel)
            .collect();
        let mut channels = Vec::with_capacity(bundle.demo.parts.len());
        let snapshot_ids: Vec<_> = bundle
            .demo
            .parts
            .iter()
            .map(|_| riffra_control::new_instance_id())
            .collect();
        for part in &bundle.demo.parts {
            let channel = part
                .midi_channel
                .or_else(|| (1..=16).find(|channel| !used_channels.contains(channel)));
            if let Some(channel) = channel {
                used_channels.insert(channel);
            }
            channels.push(channel);
        }
        for (index, (part, pattern)) in bundle.demo.parts.iter().zip(&bundle.patterns).enumerate() {
            let location = format!("parts[{index}] (part {})", part.id);
            if !(-90.0..=24.0).contains(&part.gain_db) {
                return Err(format!(
                    "{location}.gain_db: value cannot be represented by a riffra track"
                ));
            }
            let duration_ticks = convert_tick(
                pattern.length_ticks,
                pattern.ticks_per_beat,
                &format!("{location}.pattern.length_ticks"),
            )?;
            musical_end_tick = musical_end_tick.max(duration_ticks);
            let mut clip = MidiClip {
                id: riffra_control::new_instance_id(),
                name: pattern
                    .name
                    .clone()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| part.id.clone()),
                track_id: track_ids[index].clone(),
                asset_id: None,
                start_tick: TimelineTick(0),
                duration_ticks,
                notes: Vec::new(),
                events: Vec::new(),
                instrument_control_events: Vec::new(),
                muted: false,
                loop_enabled: false,
                recording_take_id: None,
            };
            for (order, event) in pattern.events.iter().enumerate() {
                let field = format!("{location}.pattern.events[{order}]");
                let (tick, kind) = match event {
                    PatternEvent::Note {
                        tick,
                        duration_ticks,
                        note,
                        velocity,
                    } => {
                        clip.notes.push(MidiNote {
                            id: riffra_control::new_instance_id(),
                            note: *note,
                            velocity: *velocity,
                            channel: channels[index].unwrap_or(1),
                            start_tick: TimelineTick(convert_tick(
                                *tick,
                                pattern.ticks_per_beat,
                                &format!("{field}.tick"),
                            )?),
                            duration_ticks: convert_tick(
                                *duration_ticks,
                                pattern.ticks_per_beat,
                                &format!("{field}.duration_ticks"),
                            )?,
                        });
                        continue;
                    }
                    PatternEvent::SustainPedal { tick, down } => (
                        *tick,
                        InstrumentControlEventKind::SustainPedal { down: *down },
                    ),
                    PatternEvent::PitchBend { tick, value } => (
                        *tick,
                        InstrumentControlEventKind::PitchBend { value: *value },
                    ),
                    PatternEvent::ModWheel { tick, value } => (
                        *tick,
                        InstrumentControlEventKind::ModWheel { value: *value },
                    ),
                    PatternEvent::Aftertouch { tick, value } => (
                        *tick,
                        InstrumentControlEventKind::Aftertouch { value: *value },
                    ),
                    PatternEvent::ParameterChange {
                        tick,
                        parameter,
                        native_value,
                    } => (
                        *tick,
                        InstrumentControlEventKind::ParameterChange {
                            parameter: parameter.clone(),
                            native_value: *native_value,
                        },
                    ),
                };
                clip.instrument_control_events.push(InstrumentControlEvent {
                    id: riffra_control::new_instance_id(),
                    tick: TimelineTick(convert_tick(
                        tick,
                        pattern.ticks_per_beat,
                        &format!("{field}.tick"),
                    )?),
                    source_order: order as u32,
                    kind,
                });
            }
            let mut track = Track::instrument(track_ids[index].clone(), part.id.clone());
            track.gain_db = part.gain_db;
            track.pan_law = PanLaw::UnityCenterStereo;
            track.midi_input.channel = part.midi_channel;
            track.instrument = Some(TrackInstrument::user_snapshot(
                riffra_control::new_instance_id(),
                part.id.clone(),
                format!("user:{}", riffra_control::new_instance_id()),
                snapshot_ids[index].clone(),
                bundle.instrument_definitions[index].clone(),
            )?);
            track.external_audio_source_track_id = part.audio_input.as_ref().map(|input| {
                let source = bundle
                    .demo
                    .parts
                    .iter()
                    .position(|source| source.id == input.part)
                    .expect("reader validated source part");
                track_ids[source].clone()
            });
            session.arrangement.tracks.push(track);
            session.arrangement.midi_clips.push(clip);
        }
        session.settings.mixdown = MixdownSettings {
            musical_end_tick,
            tail_seconds: bundle.manifest.render_settings.tail_seconds,
            fade_out_seconds: bundle.demo.mix.fade_out_seconds,
            mastering: bundle.demo.mix.master.map(|master| LoudnessMastering {
                integrated_lufs: master.integrated_lufs,
                true_peak_db: master.true_peak_db,
                loudness_range_lu: master.loudness_range_lu,
            }),
            sample_rate: Some(bundle.manifest.render_settings.sample_rate),
            block_size: Some(bundle.manifest.render_settings.block_size),
        };
        let session = session.validate_and_normalize()?;
        for (index, part) in bundle.demo.parts.iter().enumerate() {
            let snapshot = instruments.create_project_snapshot_from_directory(
                &bundle.root.join(format!("instruments/{}", part.id)),
                &snapshot_ids[index],
            )?;
            import.snapshots.push(snapshot.package_root);
            if snapshot.definition_json != bundle.instrument_definitions[index] {
                return Err(format!(
                    "parts[{index}].instrument (part {}): definition changed during import",
                    part.id
                ));
            }
        }
        import.project_id = Some(
            store
                .create_from_session(&session)
                .map_err(|error| format!("project save failed: {error}"))?
                .project_id,
        );
        Ok(import)
    }

    pub(crate) fn commit(mut self) {
        self.project_id = None;
        self.snapshots.clear();
    }
}

impl Drop for ImportedProject<'_> {
    fn drop(&mut self) {
        if let Some(id) = &self.project_id
            && let Err(error) = self.store.discard_unactivated_project(id)
        {
            tracing::error!(%error, "import project cleanup failed");
            return;
        }
        for snapshot in &self.snapshots {
            if let Err(error) = std::fs::remove_dir_all(snapshot) {
                tracing::error!(%error, "import instrument cleanup failed");
            }
        }
    }
}

fn convert_tick(tick: u64, ppq: u16, location: &str) -> Result<u64, String> {
    let product = tick
        .checked_mul(u64::from(TIMELINE_PPQ))
        .ok_or_else(|| format!("{location}: tick conversion overflow"))?;
    let ppq = u64::from(ppq);
    if ppq == 0 || !product.is_multiple_of(ppq) {
        return Err(format!(
            "{location}: tick cannot be represented exactly at 960 ppq"
        ));
    }
    Ok(product / ppq)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_conversion_rejects_rounding_and_overflow_at_the_source_field() {
        assert_eq!(
            convert_tick(240, 480, "part lead.events[0].tick").unwrap(),
            480
        );
        assert!(
            convert_tick(1, 7, "part lead.events[0].tick")
                .unwrap_err()
                .contains("part lead.events[0].tick")
        );
        assert!(
            convert_tick(u64::MAX, 480, "length_ticks")
                .unwrap_err()
                .contains("overflow")
        );
    }
}
