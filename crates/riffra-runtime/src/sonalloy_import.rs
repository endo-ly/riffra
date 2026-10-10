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
        let output = crate::process::sidecar_command(sonalloy)
            .args(["demo", "validate"])
            .arg(bundle.root.join("demo.json"))
            .args(["--json"])
            .output()
            .map_err(|error| format!("sonalloy demo validation could not be started: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "sonalloy demo validation failed: {}",
                String::from_utf8_lossy(&output.stdout)
            ));
        }
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
    use riffra_core::TrackInstrumentSource;
    use std::fs;

    #[test]
    #[ignore = "requires installed Sonalloy sidecar"]
    fn preserves_the_session_and_owns_new_resources_until_activation() {
        let root = std::env::temp_dir().join(format!(
            "riffra-bundle-import-{}",
            riffra_control::new_instance_id()
        ));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let sonalloy = workspace
            .join("target/debug")
            .join(format!("sonalloy{}", std::env::consts::EXE_SUFFIX));
        let fixture = workspace.join("contracts/sonalloy-bundle/basic");
        let store = ProjectStore::new(&root);
        let initial = store.initialize().unwrap();
        let previous_workspace = fs::read(root.join("workspace.json")).unwrap();

        let import = ImportedProject::prepare(&store, &root, &sonalloy, &fixture).unwrap();
        let session = store
            .load(import.project_id.as_deref().unwrap())
            .unwrap()
            .session;

        assert_eq!(
            session
                .arrangement
                .tracks
                .iter()
                .map(|track| track.name.as_str())
                .collect::<Vec<_>>(),
            ["lead", "duck"]
        );
        assert_eq!(
            session.arrangement.tracks[0].pan_law,
            PanLaw::UnityCenterStereo
        );
        assert_eq!(
            session.arrangement.tracks[1]
                .external_audio_source_track_id
                .as_deref(),
            Some(session.arrangement.tracks[0].id.as_str())
        );
        assert_eq!(
            session.arrangement.midi_clips[0].start_tick,
            TimelineTick(0)
        );
        assert_eq!(session.arrangement.midi_clips[0].duration_ticks, 3840);
        assert_eq!(
            session.arrangement.midi_clips[0].notes[0].start_tick,
            TimelineTick(480)
        );
        assert_eq!(
            session.arrangement.midi_clips[0].notes[1].start_tick,
            TimelineTick(960)
        );
        assert_eq!(
            session.arrangement.midi_clips[0]
                .instrument_control_events
                .last()
                .unwrap()
                .tick,
            TimelineTick(3840)
        );
        assert_eq!(session.settings.mixdown.tail_seconds, 0.5);
        assert_eq!(session.arrangement.timebase.tempo_changes[1].tick, 1920);
        assert!(matches!(
            session.arrangement.tracks[0]
                .instrument
                .as_ref()
                .unwrap()
                .source,
            TrackInstrumentSource::Internal { .. }
        ));
        let snapshot = import.snapshots[0].clone();
        assert!(
            snapshot
                .join("assets/13baa325a60e8ecc3a6282e4b8ff7daf7c2f97ffd1fa4dbfde8e7a1db596eb96.wav")
                .is_file()
        );
        assert!(
            UserInstrumentStore::new(&root, &sonalloy)
                .list()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fs::read(root.join("workspace.json")).unwrap(),
            previous_workspace
        );

        drop(import);

        assert!(!snapshot.exists());
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(store.initialize().unwrap().project_id, initial.project_id);
        assert_eq!(
            fs::read(root.join("workspace.json")).unwrap(),
            previous_workspace
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires installed Sonalloy, native render sidecars, and ffmpeg"]
    fn bundle_and_project_package_render_the_reference_performance_without_the_source_bundle() {
        // Arrange: the reference is produced by the bundled, released Sonalloy CLI.
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let binaries = workspace.join("target/debug");
        let sonalloy = binaries.join(format!("sonalloy{}", std::env::consts::EXE_SUFFIX));
        let renderer = crate::render::RenderWorker::new(
            binaries.join(format!("riffra-render{}", std::env::consts::EXE_SUFFIX)),
        );
        let root = std::env::temp_dir().join(format!(
            "riffra-bundle-audio-{}",
            riffra_control::new_instance_id()
        ));
        let bundle_path = root.join("bundle");
        let fixture = workspace.join("contracts/sonalloy-bundle/basic");
        let fixture_bundle = sonalloy_bundle::read(&fixture).unwrap();
        fs::create_dir_all(&bundle_path).unwrap();
        for file in &fixture_bundle.manifest.files {
            let destination = bundle_path.join(&file.path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(fixture.join(&file.path), destination).unwrap();
        }
        fs::copy(fixture.join("bundle.json"), bundle_path.join("bundle.json")).unwrap();
        let reference = root.join("reference.wav");
        let output = crate::process::sidecar_command(&sonalloy)
            .args(["render", "demo"])
            .arg(bundle_path.join("demo.json"))
            .arg("--output")
            .arg(&reference)
            .args([
                "--sample-rate",
                "48000",
                "--block-size",
                "256",
                "--tail",
                "0.5",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let data_root = root.join("data");
        let store = ProjectStore::new(&data_root);
        store.initialize().unwrap();
        let import = ImportedProject::prepare(&store, &data_root, &sonalloy, &bundle_path).unwrap();
        let project_id = import.project_id.clone().unwrap();
        store.write_workspace(&project_id).unwrap();
        import.commit();

        // Act: remove the complete source Bundle, reopen, then export and import the Project.
        fs::remove_dir_all(&bundle_path).unwrap();
        let session = store.load(&project_id).unwrap().session;
        let package = root.join("project.riffra");
        riffra_host::export_project(&data_root, &session, 1, &package).unwrap();
        let package_root = root.join("package-data");
        let imported = riffra_host::import_project(&package_root, &package).unwrap();
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let render = |data_root: &Path, session: &CreativeSession| {
            crate::render::render_timeline_with_cancellation(
                &renderer,
                data_root,
                crate::test_support::empty_built_in_catalog(),
                session,
                1,
                crate::api::params::RenderOptions {
                    range: Default::default(),
                    normalize: false,
                    track_id: None,
                },
                &cancelled,
            )
            .unwrap()
        };
        let original_result = render(&data_root, &session);
        let packaged_result = render(&package_root, &imported);
        let samples = |path: &Path| {
            let output = crate::process::sidecar_command("ffmpeg")
                .args(["-hide_banner", "-nostdin", "-i"])
                .arg(path)
                .args(["-f", "f32le", "-acodec", "pcm_f32le", "-"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let (samples, remainder) = output.stdout.as_chunks::<4>();
            assert!(
                remainder.is_empty(),
                "ffmpeg output must contain complete f32 samples"
            );
            samples
                .iter()
                .map(|bytes| f32::from_le_bytes(*bytes))
                .collect::<Vec<_>>()
        };
        let reference_samples = samples(&reference);

        // Assert: event timing, routing, gain, tail, and fade share the reference waveform.
        assert_eq!(reference_samples.len(), 136_001 * 2);
        for result in [&original_result, &packaged_result] {
            let actual = samples(Path::new(&result.path));
            assert_eq!(actual.len(), reference_samples.len());
            let maximum_error = actual
                .iter()
                .zip(&reference_samples)
                .map(|(actual, expected)| (actual - expected).abs())
                .fold(0.0f32, f32::max);
            let signal_peak = reference_samples
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0f32, f32::max);
            assert!(
                signal_peak > 0.01,
                "reference must contain an audible performance"
            );
            assert!(
                maximum_error < 1.0e-5,
                "maximum waveform difference {maximum_error}, reference peak {signal_peak}"
            );
            assert_eq!(&actual[actual.len() - 2..], &[0.0, 0.0]);
            assert_eq!(result.frames, 136_001);
        }
        let target = riffra_core::LoudnessMastering {
            integrated_lufs: -16.0,
            true_peak_db: -1.0,
            loudness_range_lu: 11.0,
        };
        let mut mastered_demo: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.join("demo.json")).unwrap()).unwrap();
        mastered_demo["mix"]["master"] = serde_json::json!({
            "integrated_lufs": target.integrated_lufs,
            "true_peak_db": target.true_peak_db,
            "loudness_range_lu": target.loudness_range_lu,
        });
        for part in mastered_demo["parts"].as_array_mut().unwrap() {
            for field in ["instrument", "pattern"] {
                part[field] = serde_json::Value::from(
                    fixture
                        .join(part[field].as_str().unwrap())
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        let master_demo_path = root.join("master-demo.json");
        fs::write(
            &master_demo_path,
            serde_json::to_vec(&mastered_demo).unwrap(),
        )
        .unwrap();
        let master_reference = root.join("master-reference.wav");
        let output = crate::process::sidecar_command(&sonalloy)
            .args(["render", "demo"])
            .arg(master_demo_path)
            .arg("--output")
            .arg(&master_reference)
            .args([
                "--sample-rate",
                "48000",
                "--block-size",
                "256",
                "--tail",
                "0.5",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let mut mastered_session = imported;
        mastered_session.settings.mixdown.mastering = Some(target.clone());
        let mastered = render(&package_root, &mastered_session);
        let report = mastered.mastering.as_ref().unwrap();
        assert!(report.output.true_peak_db <= target.true_peak_db);
        assert!(report.deviation.integrated_lufs.abs() < 0.1);
        let expected = samples(&master_reference);
        let actual = samples(Path::new(&mastered.path));
        assert_eq!(actual.len(), expected.len());
        let maximum_error = actual
            .iter()
            .zip(expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            maximum_error < 1.0e-5,
            "mastered waveform difference {maximum_error}"
        );
        fs::remove_dir_all(root).unwrap();
    }

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
