//! Standalone Project container commands.
use super::{DispatchError, DispatchResult, HostDispatcher};
use crate::api::output::{ProjectActivationResult, ScoreRebuild};
use crate::api::{ControlOutput, ProjectCommand};
use crate::host::open_project::{Committed, OpenProject, ProjectWriter};
use riffra_core::application::SessionSettingsPatch;
use riffra_host::now_ms;

impl HostDispatcher<'_> {
    pub(super) fn execute_project(
        &self,
        writer: Option<&mut ProjectWriter<'_>>,
        command: ProjectCommand,
        canonical: riffra_core::CanonicalState,
    ) -> Result<DispatchResult, DispatchError> {
        let sequence = canonical.sequence;
        let output = match command {
            ProjectCommand::ProjectList(_) => {
                ControlOutput::ProjectState(self.project_state(&canonical.project_id)?)
            }
            ProjectCommand::ProjectCreate(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let summary = self
                    .project_store
                    .as_ref()
                    .create(params.name)
                    .map_err(|error| error.to_string())?;
                return self.activate_project(writer, &summary.project_id);
            }
            ProjectCommand::ProjectOpen(params) => {
                let writer = writer.expect("project mutation holds the writer");
                return self.activate_project(writer, &params.project_id);
            }
            ProjectCommand::ProjectDelete(params) => {
                self.project_store
                    .as_ref()
                    .delete(&params.project_id)
                    .map_err(|error| error.to_string())?;
                ControlOutput::ProjectState(self.project_state(&canonical.project_id)?)
            }
            ProjectCommand::ProjectRename(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let committed = writer.commit(|mut app| {
                    app.update_session_settings(SessionSettingsPatch {
                        project_name: Some(Some(params.name)),
                        ..Default::default()
                    })
                })?;
                let sequence = committed.snapshot.canonical.sequence;
                self.publish_standalone(committed);
                return Ok(DispatchResult {
                    output: ControlOutput::ProjectState(self.project_state(&canonical.project_id)?),
                    sequence,
                });
            }
            ProjectCommand::ProjectImportScore(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let mut imported = crate::score_yaml::import::import(
                    &params,
                    &self.data_root,
                    &self.built_in_instruments,
                    &self.plugin_scan,
                    &self.sonalloy,
                )
                .map_err(|error| {
                    DispatchError::invalid_request_with_details(error.message, error.details)
                })?;
                if imported.source_changed() {
                    return Err(DispatchError::InvalidRequest {
                        message:
                            "the Score file or a referenced file changed before it could be applied"
                                .into(),
                        details: Some(serde_json::json!({
                            "domain": "score",
                            "kind": "scoreSourceChanged",
                                "path": imported
                                    .session
                                    .score_source
                                    .as_ref()
                                    .and_then(|source| source.path.clone()),
                        })),
                    });
                }
                let summary = self
                    .project_store
                    .as_ref()
                    .create_from_session(&imported.session)
                    .map_err(|error| error.to_string())?;
                let result = self.activate_project(writer, &summary.project_id);
                if result.is_ok() {
                    imported.retain();
                } else if let Err(error) = self.project_store.as_ref().delete(&summary.project_id) {
                    tracing::warn!(%error, "score project cleanup failed");
                }
                return result;
            }
            ProjectCommand::ProjectRebuildScore(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let current = writer.project().core.canonical_state();
                let rebuilt = crate::score_yaml::import::rebuild(
                    &current.session,
                    params.path.as_deref(),
                    &params.bindings,
                    &self.data_root,
                    &self.built_in_instruments,
                    &self.plugin_scan,
                    &self.sonalloy,
                )
                .map_err(|error| {
                    DispatchError::invalid_request_with_details(error.message, error.details)
                })?;
                return self.accept_score_rebuild(writer, current, rebuilt);
            }
            ProjectCommand::ProjectExportScore(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let current = writer.project().core.canonical_state();
                if params.link && already_linked(&current.session) {
                    return Err(already_linked_error());
                }
                let plan = crate::score_yaml::export::plan(
                    &current.session,
                    &params.path,
                    &self.data_root,
                    &self.built_in_instruments,
                    &self.plugin_scan,
                )
                .map_err(export_error)?;
                if !params.link {
                    let path = plan.write().map_err(export_error)?;
                    return Ok(DispatchResult {
                        output: ControlOutput::ScoreExport(crate::api::output::ScoreExport {
                            path: path.to_string_lossy().into_owned(),
                            omissions: plan.omissions,
                            link: None,
                        }),
                        sequence: current.sequence,
                    });
                }
                if !plan.omissions.is_empty() {
                    return Err(omissions_error(&plan.omissions));
                }
                let path = plan.write().map_err(export_error)?;
                let linked = plan.linked_session(&current.session);
                let rebuilt = crate::score_yaml::import::link(
                    &linked,
                    &path,
                    &self.data_root,
                    &self.built_in_instruments,
                    &self.plugin_scan,
                    &self.sonalloy,
                )
                .map_err(|error| {
                    DispatchError::invalid_request_with_details(error.message, error.details)
                })?;
                let mut result = self.accept_score_rebuild(writer, current, rebuilt)?;
                let ControlOutput::ScoreRebuild(rebuild) = result.output else {
                    unreachable!("score rebuild returns a rebuild result")
                };
                result.output = ControlOutput::ScoreExport(crate::api::output::ScoreExport {
                    path: path.to_string_lossy().into_owned(),
                    omissions: Vec::new(),
                    link: Some(rebuild),
                });
                return Ok(result);
            }
            ProjectCommand::ProjectImport(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let session = crate::projects::import(&self.data_root, &params.path)?;
                let summary = self
                    .project_store
                    .as_ref()
                    .create_from_session(&session)
                    .map_err(|error| error.to_string())?;
                return self.activate_project(writer, &summary.project_id);
            }
            ProjectCommand::MusicExportMidi(params) => ControlOutput::MidiExport(
                crate::projects::export_midi(&canonical.session, &params)?,
            ),
            ProjectCommand::ProjectExport(params) => {
                ControlOutput::ProjectExport(crate::projects::export(
                    &self.data_root,
                    &canonical.session,
                    now_ms(),
                    &params.output,
                )?)
            }
        };
        Ok(DispatchResult { output, sequence })
    }

    /// Commits one rebuilt Score candidate and reports the synchronization.
    fn accept_score_rebuild(
        &self,
        writer: &mut ProjectWriter<'_>,
        current: riffra_core::CanonicalState,
        mut rebuilt: crate::score_yaml::import::ScoreOutcome,
    ) -> Result<DispatchResult, DispatchError> {
        let changed = rebuilt.changed;
        let yaml_changed = rebuilt.yaml_changed;
        let link_changed = rebuilt.link_changed;
        let diagnostics = rebuilt.diagnostics.clone();
        let path = rebuilt
            .session
            .score_source
            .as_ref()
            .and_then(|source| source.path.clone());
        let result = |sequence: u64| ScoreRebuild {
            project_id: current.project_id.clone(),
            changed,
            yaml_changed,
            link_changed,
            sequence,
            path: path.clone(),
            diagnostics: diagnostics.clone(),
        };
        let Some(candidate) = rebuilt.candidate(&current.session) else {
            return Ok(DispatchResult {
                output: ControlOutput::ScoreRebuild(result(current.sequence)),
                sequence: current.sequence,
            });
        };
        if rebuilt.source_changed() {
            return Err(DispatchError::InvalidRequest {
                message: "the linked Score file changed before it could be applied".into(),
                details: Some(serde_json::json!({
                    "domain": "score",
                    "kind": "scoreSourceChanged",
                    "path": path,
                })),
            });
        }
        let committed = writer
            .commit_accepted(candidate)
            .map_err(|error| DispatchError::CommandFailed(error.to_string()))?;
        if changed {
            rebuilt.retain();
        }
        let sequence = committed.snapshot.canonical.sequence;
        let output = ControlOutput::ScoreRebuild(result(sequence));
        self.publish_standalone(committed);
        Ok(DispatchResult { output, sequence })
    }

    fn activate_project(
        &self,
        writer: &mut ProjectWriter<'_>,
        project_id: &str,
    ) -> Result<DispatchResult, DispatchError> {
        let prepared = crate::projects::prepare(self.project_store.as_ref(), project_id)
            .map_err(DispatchError::CommandFailed)?;
        let next = OpenProject {
            storage: prepared.storage,
            core: riffra_core::AppCore::new(
                project_id.into(),
                prepared.loaded.session,
                writer.project().core.snapshot().sequence + 1,
            ),
            recovered_from_generation: prepared.loaded.recovered_from_generation,
        };
        self.project_store
            .as_ref()
            .write_workspace(project_id)
            .map_err(|error| error.to_string())?;
        writer.replace(next);
        let snapshot = self.core.cell().read();
        let canonical = snapshot.canonical.clone();
        self.publish_standalone(Committed {
            snapshot,
            value: (),
        });
        let sequence = canonical.sequence;
        Ok(DispatchResult {
            output: ControlOutput::ProjectActivation(ProjectActivationResult {
                project_state: prepared.project_state,
                canonical,
                recovery: prepared.recovery,
            }),
            sequence,
        })
    }
}

fn already_linked(session: &riffra_core::CreativeSession) -> bool {
    session
        .score_source
        .as_ref()
        .is_some_and(|source| source.path.is_some())
}

fn already_linked_error() -> DispatchError {
    DispatchError::InvalidRequest {
        message: "the active Project is already linked to a Score file".into(),
        details: Some(serde_json::json!({
            "domain": "score",
            "kind": "scoreExportLinked",
        })),
    }
}

fn export_error(error: crate::score_yaml::export::ExportError) -> DispatchError {
    DispatchError::invalid_request_with_details(error.message, error.details)
}

fn omissions_error(omissions: &[crate::api::output::ScoreExportOmission]) -> DispatchError {
    let subjects = omissions
        .iter()
        .map(|omission| omission.subject.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    DispatchError::invalid_request_with_details(
        format!("the Score cannot express this Project: {subjects}"),
        serde_json::json!({
            "domain": "score",
            "kind": "scoreExportOmissions",
            "omissions": omissions,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::super::Dispatcher;
    use crate::test_support::{command as request, output_value};
    use riffra_host::ProjectStore;
    use serde_json::{Value, json};

    fn session(dispatcher: &Dispatcher) -> riffra_core::CreativeSession {
        let state = dispatcher
            .dispatch(request("session.get", json!({})))
            .unwrap();
        match state.output {
            crate::api::ControlOutput::Session(session) => session,
            output => panic!("expected the canonical session, got {output:?}"),
        }
    }

    #[test]
    fn export_score_links_an_unlinked_project_and_keeps_track_identity() {
        use crate::test_support::{mutated_session, output_type};

        let root = std::env::temp_dir().join(format!(
            "riffra-score-export-link-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let created = dispatcher
            .dispatch(request("track.add", json!({"name":"Take","kind":"audio"})))
            .unwrap();
        let before = mutated_session(&created);
        let track = before.arrangement.tracks[0].clone();

        let folder = root.join("song");
        let link = dispatcher
            .dispatch(request(
                "project.export-score",
                json!({"path": folder, "link": true}),
            ))
            .unwrap();

        assert_eq!(output_type(&link), "scoreExport");
        let value = output_value(&link);
        assert!(folder.join("score.yaml").is_file());
        assert_eq!(value["link"]["changed"], true);
        assert!(value["link"]["sequence"].as_u64().unwrap() > before.arrangement.revision);
        let linked = session(&dispatcher);
        assert_eq!(linked.arrangement.tracks.len(), 1);
        assert_eq!(linked.arrangement.tracks[0].id, track.id);
        assert_eq!(linked.arrangement.tracks[0].name, track.name);
        assert!(linked.score_source.as_ref().unwrap().path.is_some());

        let again = dispatcher.dispatch(request(
            "project.export-score",
            json!({"path": root.join("second"), "link": true}),
        ));
        assert!(again.is_err(), "a linked Project owns its Score file");
        assert!(!root.join("second").exists());
    }

    #[test]
    fn export_score_without_link_leaves_the_project_unchanged() {
        use crate::test_support::output_type;

        let root = std::env::temp_dir().join(format!(
            "riffra-score-export-plain-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        dispatcher
            .dispatch(request("track.add", json!({"name":"Take","kind":"audio"})))
            .unwrap();
        let folder = root.join("song");
        let exported = dispatcher
            .dispatch(request("project.export-score", json!({"path": folder})))
            .unwrap();
        assert_eq!(output_type(&exported), "scoreExport");
        assert!(folder.join("score.yaml").is_file());
        assert!(session(&dispatcher).score_source.is_none());
    }

    #[test]
    fn export_score_reports_unrepresentable_performance_and_refuses_link() {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-export-keyswitch-{}",
            riffra_control::new_instance_id()
        ));
        let resources = root.join("resources");
        std::fs::create_dir_all(resources.join("01-keys")).unwrap();
        std::fs::write(
            resources.join("01-keys/definition.json"),
            r#"{"metadata":{"name":"Keys"},"layers":[{"id":"main","trigger":{"event":"note_on","key_min":60,"key_max":60,"velocity_min":1,"velocity_max":127}}]}"#,
        )
        .unwrap();
        std::fs::write(
            resources.join("manifest.json"),
            br#"{"sourceRelease":"vtest","presets":[{"id":"01-keys","name":"Keys","author":"Riffra","description":"Keys","category":"Keys","tags":["test"],"recommendedRange":{"minMidi":36,"maxMidi":84},"preview":{"tempoBpm":120,"ticksPerBeat":480,"timeSignature":{"numerator":4,"denominator":4},"lengthTicks":1920,"notes":[{"tick":0,"durationTicks":480,"note":48,"velocity":100}]},"definitionPath":"01-keys/definition.json","resourceBasePath":"01-keys"}]}"#,
        )
        .unwrap();
        let dispatcher = Dispatcher::open(root.clone(), resources).unwrap();
        dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let track_id = session(&dispatcher).arrangement.tracks[0].id.clone();
        dispatcher
            .dispatch(request(
                "instrument.apply",
                json!({"trackId":track_id,"instrumentId":"builtin:01-keys"}),
            ))
            .unwrap();
        dispatcher
            .dispatch(request(
                "clip.midi.create",
                json!({"trackId":track_id,"startTick":0,"durationTicks":1920,"name":"Keys"}),
            ))
            .unwrap();
        let clip_id = session(&dispatcher).arrangement.midi_clips[0].id.clone();
        dispatcher
            .dispatch(request(
                "clip.midi.instrument-event.add",
                json!({
                    "clipId": clip_id,
                    "tick": 480,
                    "kind": {"type":"keySwitch","note":24,"mode":"latch","down":true},
                }),
            ))
            .unwrap();
        dispatcher
            .dispatch(
                request(
                    "clip.midi.note.add",
                    json!({"clipId":clip_id,"pitch":60,"startTick":0,"durationTicks":480,"velocity":100,"channel":10}),
                ))
            .unwrap();
        let before = session(&dispatcher);
        let folder = root.join("song");
        let error = dispatcher
            .dispatch(request(
                "project.export-score",
                json!({"path": folder, "link": true}),
            ))
            .unwrap_err();
        let crate::dispatcher::DispatchError::InvalidRequest {
            details: Some(details),
            ..
        } = error
        else {
            panic!("expected an omission error, got {error:?}");
        };
        assert_eq!(details["kind"], "scoreExportOmissions");
        let omissions = details["omissions"].as_array().expect("omissions");
        assert!(
            omissions.iter().any(|omission| {
                omission["subject"]
                    .as_str()
                    .is_some_and(|subject| subject.contains("key switch 24"))
            }),
            "{details}"
        );
        assert!(
            omissions.iter().any(|omission| {
                omission["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("channel 10"))
            }),
            "{details}"
        );
        assert!(!folder.exists());
        let after = session(&dispatcher);
        assert_eq!(after.arrangement.revision, before.arrangement.revision);
        assert!(after.score_source.is_none());
    }

    fn kit_export_dispatcher(label: &str) -> (Dispatcher, std::path::PathBuf) {
        use crate::api::output::{
            PluginEntry, PluginFormat, PluginRole, PluginScanState, ScanReport,
        };

        let root = std::env::temp_dir().join(format!(
            "riffra-score-export-{label}-{}",
            riffra_control::new_instance_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let plugin = root.join("Drums.vst3");
        std::fs::create_dir_all(&plugin).unwrap();
        crate::plugins::catalog::save(
            &root,
            &ScanReport {
                root: root.to_string_lossy().into_owned(),
                started_at_ms: 0,
                finished_at_ms: 0,
                plugins: vec![PluginEntry {
                    id: "Drums".into(),
                    name: "Drums".into(),
                    vendor: None,
                    version: None,
                    identifier: Some("Drums-class".into()),
                    format: PluginFormat::Vst3,
                    role: Some(PluginRole::Instrument),
                    path: plugin.to_string_lossy().into_owned(),
                    bundle: true,
                    modified_at_ms: None,
                    scan_state: PluginScanState::Validated,
                }],
                issues: Vec::new(),
            },
        )
        .unwrap();
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let score = root.join("kit.yaml");
        std::fs::write(
            &score,
            r#"{
                "meta":{"title":"Kit","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
                "form":[{"section":"verse","bars":1,"tonic":"C"}],
                "sections":{"verse":{"drums":"beat","lines":{"main":{"instrument":"lead","melody":"theme"}}}},
                "melodies":{"theme":["1:16"]},
                "drums":{"beat":{"parts":{"kick":["x..............."]}}},
                "instruments":{"kick":{"kit":"drums","trigger_note_midi":36},"lead":{"source":{"vst3":{"plugin":"Drums"}}}},
                "kits":{"drums":{"source":{"vst3":{"plugin":"Drums"}}}}
            }"#,
        )
        .unwrap();
        dispatcher
            .dispatch(request("project.import-score", json!({"path": score})))
            .unwrap();
        let mut canonical = session(&dispatcher);
        canonical.score_source = None;
        let _ = dispatcher
            .core
            .cell()
            .write()
            .commit_accepted(canonical)
            .unwrap();
        (dispatcher, root)
    }

    fn assert_kit_export_refused(
        dispatcher: &Dispatcher,
        root: &std::path::Path,
        subject: &str,
        message: &str,
    ) {
        let before = session(dispatcher);
        let folder = root.join("song");
        let error = dispatcher
            .dispatch(request(
                "project.export-score",
                json!({"path": folder, "link": true}),
            ))
            .unwrap_err();
        let crate::dispatcher::DispatchError::InvalidRequest {
            details: Some(details),
            ..
        } = error
        else {
            panic!("expected an omission error, got {error:?}");
        };
        assert_eq!(details["kind"], "scoreExportOmissions");
        assert!(
            details["omissions"].as_array().is_some_and(|omissions| {
                omissions.iter().any(|omission| {
                    omission["subject"]
                        .as_str()
                        .is_some_and(|candidate| candidate.contains(subject))
                        && omission["message"]
                            .as_str()
                            .is_some_and(|candidate| candidate.contains(message))
                })
            }),
            "{details}"
        );
        assert!(!folder.exists());
        let after = session(dispatcher);
        assert_eq!(after.arrangement.revision, before.arrangement.revision);
        assert!(after.score_source.is_none());
    }

    #[test]
    fn export_score_reports_a_bypassed_kit_and_refuses_link() {
        let (dispatcher, root) = kit_export_dispatcher("bypassed-kit");
        let mut canonical = session(&dispatcher);
        let kit = canonical
            .arrangement
            .tracks
            .iter_mut()
            .find(|track| {
                track
                    .score_instrument
                    .as_deref()
                    .is_some_and(|id| id.starts_with("kit:"))
            })
            .expect("the Score has a kit track");
        kit.instrument
            .as_mut()
            .expect("the kit has an instrument")
            .bypassed = true;
        let _ = dispatcher
            .core
            .cell()
            .write()
            .commit_accepted(canonical)
            .unwrap();

        assert_kit_export_refused(&dispatcher, &root, "instrument", "bypassed");
    }

    #[test]
    fn export_score_reports_an_unassigned_kit_and_refuses_link() {
        let (dispatcher, root) = kit_export_dispatcher("unassigned-kit");
        let mut canonical = session(&dispatcher);
        let kit = canonical
            .arrangement
            .tracks
            .iter_mut()
            .find(|track| {
                track
                    .score_instrument
                    .as_deref()
                    .is_some_and(|id| id.starts_with("kit:"))
            })
            .expect("the Score has a kit track");
        kit.instrument = None;
        let _ = dispatcher
            .core
            .cell()
            .write()
            .commit_accepted(canonical)
            .unwrap();

        assert_kit_export_refused(&dispatcher, &root, "track", "no instrument");
        let canonical = session(&dispatcher);
        let lead = canonical
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("lead"))
            .expect("the normal track stays in the Project");
        assert!(lead.instrument.is_some());
    }

    #[test]
    fn midi_export_uses_clip_or_track_origin() {
        use crate::test_support::mutated_session;
        let root = std::env::temp_dir().join(format!(
            "riffra-midi-export-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Voice","kind":"instrument"}),
            ))
            .unwrap();
        let track_id = mutated_session(&track).arrangement.tracks[0].id.clone();
        let clip = dispatcher
            .dispatch(request(
                "clip.midi.create",
                json!({"trackId":track_id,"startTick":960,"durationTicks":1920,"name":"Voice"}),
            ))
            .unwrap();
        let clip_id = mutated_session(&clip).arrangement.midi_clips[0].id.clone();
        dispatcher.dispatch(request("clip.midi.note.add", json!({"clipId":clip_id,"pitch":60,"startTick":240,"durationTicks":480,"velocity":100,"channel":1,"lyric":"き"}))).unwrap();
        for (scope, expected) in [
            (json!({"clipId":clip_id}), 240),
            (json!({"trackId":track_id}), 1200),
        ] {
            let path = root.join(format!("{expected}.mid"));
            let mut params = scope;
            params["output"] = json!(path);
            let output = dispatcher
                .dispatch(request("music.export-midi", params))
                .unwrap();
            assert_eq!(output_value(&output)["noteCount"], 1);
            let (_, notes, _) =
                riffra_host::parse_smf(&std::fs::read(&path).unwrap(), None).unwrap();
            assert_eq!(notes[0].start_tick.0, expected);
            assert_eq!(notes[0].lyric.as_deref(), Some("き"));
        }
        let second = dispatcher
            .dispatch(request(
                "clip.midi.create",
                json!({"trackId":track_id,"startTick":1200,"durationTicks":1920,"name":"Second"}),
            ))
            .unwrap();
        let second_id = mutated_session(&second).arrangement.midi_clips[1]
            .id
            .clone();
        dispatcher.dispatch(request("clip.midi.note.add", json!({"clipId":second_id,"pitch":60,"startTick":100,"durationTicks":240,"velocity":100,"channel":1}))).unwrap();
        let path = root.join("1200.mid");
        let existing = std::fs::read(&path).unwrap();
        let result = dispatcher.dispatch(request(
            "music.export-midi",
            json!({"trackId":track_id,"output":path}),
        ));
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), existing);

        dispatcher.dispatch(request("clip.midi.note.add", json!({"clipId":clip_id,"pitch":60,"startTick":300,"durationTicks":240,"velocity":100,"channel":1}))).unwrap();
        let path = root.join("overlap.mid");
        let result = dispatcher.dispatch(request(
            "music.export-midi",
            json!({"clipId":clip_id,"output":path}),
        ));
        assert!(result.is_err());
        assert!(!path.exists());
        drop(dispatcher);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn project_commands_keep_containers_independent_and_switch_the_active_session() {
        let root = std::env::temp_dir().join(format!(
            "riffra-dispatcher-projects-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();

        let initial = dispatcher
            .dispatch(request("project.list", json!({})))
            .unwrap();
        let initial_id = output_value(&initial)["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();

        let created = dispatcher
            .dispatch(request("project.create", json!({"name": "Second"})))
            .unwrap();
        let second_id = output_value(&created)["projectState"]["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(second_id, initial_id);
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})))
                .map(|result| output_value(&result))
                .unwrap()["projectName"],
            "Second"
        );

        dispatcher
            .dispatch(request("project.open", json!({"projectId": initial_id})))
            .unwrap();
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})))
                .map(|result| output_value(&result))
                .unwrap()["projectName"],
            Value::Null
        );
        assert!(
            root.join("projects")
                .join(second_id)
                .join("session.json")
                .is_file()
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn import_creates_a_new_container_and_export_uses_the_requested_path() {
        let root = std::env::temp_dir().join(format!(
            "riffra-dispatcher-import-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let initial_id = dispatcher
            .dispatch(request("project.list", json!({})))
            .map(|result| output_value(&result))
            .unwrap()["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        dispatcher
            .dispatch(request("project.rename", json!({"name": "Source"})))
            .unwrap();
        let export_path = root.join("source.riffra");
        let exported = dispatcher
            .dispatch(request("project.export", json!({"output": export_path})))
            .unwrap();
        let manifest = output_value(&exported)["path"].as_str().unwrap().to_owned();
        let before = dispatcher
            .dispatch(request("project.list", json!({})))
            .map(|result| output_value(&result))
            .unwrap()["projects"]
            .as_array()
            .unwrap()
            .len();

        let imported = dispatcher
            .dispatch(request("project.import", json!({"path": manifest})))
            .unwrap();
        let imported = output_value(&imported);
        let imported_id = imported["projectState"]["activeProjectId"]
            .as_str()
            .unwrap();

        assert_eq!(
            imported["projectState"]["projects"]
                .as_array()
                .unwrap()
                .len(),
            before + 1
        );
        assert_ne!(imported_id, initial_id);
        assert!(
            root.join("projects")
                .join(imported_id)
                .join("session.json")
                .is_file()
        );
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})))
                .map(|result| output_value(&result))
                .unwrap()["projectName"],
            "Source"
        );
        dispatcher
            .dispatch(request("project.open", json!({"projectId": initial_id})))
            .unwrap();
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})))
                .map(|result| output_value(&result))
                .unwrap()["projectName"],
            "Source"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn project_activation_returns_recovery_for_the_target_project_only() {
        let root = std::env::temp_dir().join(format!(
            "riffra-dispatcher-recovery-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let initial_id = dispatcher
            .dispatch(request("project.list", json!({})))
            .map(|result| output_value(&result))
            .unwrap()["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        let created = dispatcher
            .dispatch(request("project.create", json!({"name": "Recovered"})))
            .unwrap();
        let recovered_id = output_value(&created)["projectState"]["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();

        let project_store = ProjectStore::new(&root);
        let storage = project_store.session_store(&recovered_id).unwrap();
        let mut session = storage.load_existing().unwrap().session;
        session.settings.note = "generation source".into();
        storage.save(&session).unwrap();
        std::fs::write(
            root.join("projects")
                .join(&recovered_id)
                .join("session.json"),
            b"not-json",
        )
        .unwrap();

        let recovered = dispatcher
            .dispatch(request("project.open", json!({"projectId": recovered_id})))
            .unwrap();
        assert_eq!(
            output_value(&recovered)["recovery"]["recoveredFromGeneration"],
            true
        );
        assert_eq!(
            output_value(&recovered)["recovery"]["recoveryCandidates"][0]["projectName"],
            "Recovered"
        );

        let normal = dispatcher
            .dispatch(request("project.open", json!({"projectId": initial_id})))
            .unwrap();
        assert_eq!(
            output_value(&normal)["recovery"]["recoveredFromGeneration"],
            false
        );
        assert!(
            output_value(&normal)["recovery"]["recoveryCandidates"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
