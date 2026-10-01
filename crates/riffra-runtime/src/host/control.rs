use super::lifecycle::default_plugin_root;
use super::project;
use super::*;
use crate::api::output::{
    ArrangementMutationResult, DeviceCapabilities, DeviceInspection, HostInfo, HostStatus,
    InstrumentPreviewDefinition, PluginPresetInfo, PluginStateSnapshot,
};
use crate::api::{
    CanonicalAccess, CanonicalCommand, CommandScope, ControlCommand, ControlOutput, RuntimeCommand,
};
use crate::execution::GraphPluginState;
use crate::instrument::{BuiltInInstrumentCatalog, UserInstrumentStore};

impl HostState {
    fn failure(request_id: String, error: ProtocolError) -> ControlResponse {
        ControlResponse::failure(request_id, None, error)
    }

    pub(super) fn session_context<'a>(
        &'a self,
        writer: Option<super::open_project::ProjectWriter<'a>>,
        snapshot: CanonicalState,
        publish: &'a dyn Fn(
            &super::open_project::ProjectWriter<'_>,
            super::open_project::Committed<()>,
        ) -> ArrangementMutationResult,
    ) -> Result<SessionContext<'a>, ProtocolError> {
        let storage = match writer.as_ref() {
            Some(writer) => writer.project().storage.clone(),
            None => self
                .project_store
                .session_store(&snapshot.project_id)
                .map_err(|error| command_error(error.to_string()))?,
        };
        Ok(SessionContext {
            project: &self.project,
            snapshot,
            writer,
            audio: &self.audio,
            runtime: self.runtime.as_ref(),
            storage,
            data_root: &self.data_root,
            built_in_instruments: self.built_in_instruments.as_ref(),
            safe_mode: self.safe_mode,
            publish,
        })
    }

    pub(crate) fn dispatch_request(self: &Arc<Self>, request: ControlRequest) -> ControlResponse {
        let _lifecycle = match self.lifecycle.enter() {
            Ok(guard) => guard,
            Err(error) => return Self::failure(request.request_id, error),
        };
        self.dispatch_request_inner(request)
    }

    fn dispatch_request_inner(self: &Arc<Self>, request: ControlRequest) -> ControlResponse {
        if let Err(error) = request.validate() {
            return Self::failure(request.request_id, error);
        }
        let ControlRequest {
            request_id,
            command,
            expected_sequence,
            expected_project_id,
            params,
        } = request;
        let command = match ControlCommand::decode(&command, params) {
            Ok(command) => command,
            Err(error) => return Self::failure(request_id, error.into()),
        };
        let scope = command.policy().scope;
        let writer = (scope
            == CommandScope::Project {
                long_running: false,
            })
        .then(|| self.project.write());
        let current = writer.as_ref().map_or_else(
            || self.project.read().canonical.clone(),
            |writer| writer.project().core.canonical_state(),
        );
        if let Some(expected_sequence) = expected_sequence
            && expected_sequence != current.sequence
        {
            return Self::failure(
                request_id,
                ProtocolError::conflict(expected_sequence, current.sequence),
            );
        }
        if let Err(error) = crate::dispatcher::validate_project_precondition(
            scope,
            expected_project_id.as_deref(),
            &current.project_id,
        ) {
            return Self::failure(request_id, error);
        }
        let result = match command {
            ControlCommand::Canonical(command) => {
                let access = command.access();
                self.dispatch_canonical(command, access, current, writer)
            }
            ControlCommand::Project(command) => project::dispatch(self, writer, command, current),
            ControlCommand::Runtime(command) => self.dispatch_runtime(command, current, writer),
        };
        match result {
            Ok((output, sequence)) => ControlResponse::success(request_id, sequence, output.into()),
            Err(error) => Self::failure(request_id, error),
        }
    }

    /// Executes a canonical command, using the live implementations that
    /// prepare runtime resources before committing.
    fn dispatch_canonical(
        &self,
        command: CanonicalCommand,
        access: CanonicalAccess,
        current: CanonicalState,
        writer: Option<super::open_project::ProjectWriter<'_>>,
    ) -> Result<(ControlOutput, u64), ProtocolError> {
        let publish = |writer: &super::open_project::ProjectWriter<'_>, committed| {
            self.publish_commit(writer, committed).0
        };
        let mut context = self.session_context(writer, current.clone(), &publish)?;
        let sequence = Some(current.sequence);
        let mutation = match command {
            CanonicalCommand::TrackAudioInputSet(params) => session_adapter::set_track_audio_input(
                &mut context,
                &params.track_id,
                Some(params.channel_index),
            ),
            CanonicalCommand::TrackAudioInputClear(params) => {
                session_adapter::set_track_audio_input(&mut context, &params.track_id, None)
            }
            CanonicalCommand::TrackMidiInputSet(params) => session_adapter::set_track_midi_input(
                &mut context,
                &params.track_id,
                riffra_core::MidiInputRoute {
                    device_id: params.device_id,
                    channel: params.channel,
                },
            ),
            CanonicalCommand::TrackMidiInputClear(params) => session_adapter::set_track_midi_input(
                &mut context,
                &params.track_id,
                riffra_core::MidiInputRoute::default(),
            ),
            CanonicalCommand::InstrumentVst3Set(params) => {
                session_adapter::set_track_vst3_instrument_with_expected_sequence(
                    &mut context,
                    &params.track_id,
                    &params.plugin_path,
                    sequence,
                )
            }
            CanonicalCommand::InstrumentClear(params) => {
                session_adapter::clear_track_instrument(&mut context, &params.track_id)
            }
            CanonicalCommand::EffectAdd(params) => {
                session_adapter::add_track_effect_with_expected_sequence(
                    &mut context,
                    &params.track_id,
                    &params.plugin_path,
                    sequence,
                )
            }
            CanonicalCommand::EffectRemove(params) => session_adapter::remove_track_effect(
                &mut context,
                &params.track_id,
                &params.device_id,
            ),
            CanonicalCommand::EffectReorder(params) => session_adapter::reorder_track_effects(
                &mut context,
                &params.track_id,
                &params.device_ids,
            ),
            CanonicalCommand::DeviceBypass(params) => session_adapter::set_track_device_bypassed(
                &mut context,
                &params.track_id,
                &params.device_id,
                params.bypassed,
            ),
            CanonicalCommand::DeviceParameterSet(params) => {
                session_adapter::set_track_device_parameter(
                    &mut context,
                    &params.track_id,
                    &params.device_id,
                    params.parameter_index,
                    params.value,
                )
            }
            CanonicalCommand::MissingRelink(params) => {
                let asset_id =
                    riffra_core::AssetId::from_normalized(&params.asset_id).map_err(|error| {
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
                    })?;
                session_adapter::relink_missing_dependency(&mut context, asset_id, &params.new_path)
            }
            CanonicalCommand::MissingDisablePlugin(params) => {
                session_adapter::disable_missing_plugin(&mut context, &params.device_id)
            }
            CanonicalCommand::MissingReplacePlugin(params) => {
                session_adapter::replace_missing_track_plugin_with_expected_sequence(
                    &mut context,
                    &params.device_id,
                    &params.new_path,
                    sequence,
                )
            }
            CanonicalCommand::Undo(_) => session_adapter::undo(&mut context),
            CanonicalCommand::Redo(_) => session_adapter::redo(&mut context),
            command @ (CanonicalCommand::SessionGet(_)
            | CanonicalCommand::SessionInspect(_)
            | CanonicalCommand::SessionApply(_)
            | CanonicalCommand::SessionSettingsUpdate(_)
            | CanonicalCommand::HistoryGet(_)
            | CanonicalCommand::MasterGainSet(_)
            | CanonicalCommand::TrackList(_)
            | CanonicalCommand::TrackAdd(_)
            | CanonicalCommand::TrackUpdate(_)
            | CanonicalCommand::TrackRemove(_)
            | CanonicalCommand::TrackDuplicate(_)
            | CanonicalCommand::TrackReorder(_)
            | CanonicalCommand::MarkerAdd(_)
            | CanonicalCommand::MarkerUpdate(_)
            | CanonicalCommand::MarkerRemove(_)
            | CanonicalCommand::TimebaseUpdate(_)
            | CanonicalCommand::LoopRangeSet(_)
            | CanonicalCommand::PunchRangeSet(_)
            | CanonicalCommand::AutomationSet(_)
            | CanonicalCommand::AutomationClear(_)
            | CanonicalCommand::AudioClipList(_)
            | CanonicalCommand::AudioClipAddAsset(_)
            | CanonicalCommand::AudioClipUpdate(_)
            | CanonicalCommand::AudioClipMove(_)
            | CanonicalCommand::AudioClipTrim(_)
            | CanonicalCommand::AudioClipSplit(_)
            | CanonicalCommand::AudioClipDuplicate(_)
            | CanonicalCommand::AudioClipCrossfade(_)
            | CanonicalCommand::MidiClipList(_)
            | CanonicalCommand::MidiClipCreate(_)
            | CanonicalCommand::MidiClipAddAsset(_)
            | CanonicalCommand::MidiClipUpdate(_)
            | CanonicalCommand::MidiClipMove(_)
            | CanonicalCommand::MidiClipTrim(_)
            | CanonicalCommand::MidiClipSplit(_)
            | CanonicalCommand::MidiClipDuplicate(_)
            | CanonicalCommand::MidiNoteAdd(_)
            | CanonicalCommand::MidiNoteInsert(_)
            | CanonicalCommand::MidiNoteUpdate(_)
            | CanonicalCommand::MidiNoteUpdateMany(_)
            | CanonicalCommand::MidiNoteRemove(_)
            | CanonicalCommand::MidiNoteRemoveMany(_)
            | CanonicalCommand::MidiNoteClear(_)
            | CanonicalCommand::MidiNoteQuantize(_)
            | CanonicalCommand::MidiNoteTransform(_)
            | CanonicalCommand::MidiNoteDuplicate(_)
            | CanonicalCommand::ClipRemove(_)
            | CanonicalCommand::ClipPaste(_)
            | CanonicalCommand::MusicMidiClipCreate(_)
            | CanonicalCommand::MusicMidiClipResize(_)
            | CanonicalCommand::MusicNoteInsert(_)
            | CanonicalCommand::MusicNoteList(_)
            | CanonicalCommand::MusicNoteGet(_)
            | CanonicalCommand::MusicNoteUpdate(_)
            | CanonicalCommand::MusicNoteRemove(_)
            | CanonicalCommand::MusicNoteTransform(_)
            | CanonicalCommand::MusicHarmonyResolve(_)
            | CanonicalCommand::MusicHarmonyList(_)
            | CanonicalCommand::MusicHarmonyInsert(_)
            | CanonicalCommand::MusicHarmonyUpdate(_)
            | CanonicalCommand::MusicHarmonyRemove(_)
            | CanonicalCommand::MusicHarmonyRealize(_)
            | CanonicalCommand::MusicPhraseInsert(_)
            | CanonicalCommand::MusicPhrasePreview(_)
            | CanonicalCommand::MusicRegionList(_)
            | CanonicalCommand::MusicRegionAdd(_)
            | CanonicalCommand::MusicRegionUpdate(_)
            | CanonicalCommand::MusicRegionRemove(_)
            | CanonicalCommand::AssetImportMidi(_)
            | CanonicalCommand::InstrumentList(_)
            | CanonicalCommand::InstrumentSave(_)
            | CanonicalCommand::InstrumentExport(_)
            | CanonicalCommand::InstrumentApply(_)) => {
                return self.dispatch_shared_canonical(command, access, current, &mut context);
            }
        }
        .map_err(|error| error.protocol_error())?;
        Ok(mutation_output(mutation))
    }

    /// Executes a canonical command through the shared dispatcher and
    /// projects the committed state.
    fn dispatch_shared_canonical(
        &self,
        command: CanonicalCommand,
        access: CanonicalAccess,
        current: CanonicalState,
        context: &mut SessionContext<'_>,
    ) -> Result<(ControlOutput, u64), ProtocolError> {
        let dispatcher = HostDispatcher::borrowed(
            &self.project,
            &self.project_store,
            &self.data_root,
            &self.binaries.sonalloy,
            &self.built_in_instruments,
        );
        if access == CanonicalAccess::Read {
            let result = dispatcher
                .execute_read_canonical(command, current)
                .map_err(|error| error.protocol_error())?;
            return Ok((result.output, result.sequence));
        }
        let writer = context
            .writer
            .as_mut()
            .expect("shared canonical mutations hold the project writer");
        let current_sequence = current.sequence;
        let (result, committed) = dispatcher
            .execute_canonical(writer, command, access, current)
            .map_err(|error| error.protocol_error())?;
        if result.sequence <= current_sequence {
            return Ok((result.output, result.sequence));
        }
        let (mut mutation, ()) = self.publish_commit(writer, committed);
        match result.output {
            ControlOutput::BatchMutation(mut batch) => {
                batch.projection = Some(mutation.projection);
                Ok((
                    ControlOutput::BatchMutation(batch),
                    mutation.canonical.sequence,
                ))
            }
            ControlOutput::ArrangementMutation(committed) => {
                mutation.created_entity_ids = committed.created_entity_ids;
                Ok(mutation_output(mutation))
            }
            output => Ok((output, mutation.canonical.sequence)),
        }
    }

    fn dispatch_runtime(
        self: &Arc<Self>,
        command: RuntimeCommand,
        current: CanonicalState,
        mut writer: Option<super::open_project::ProjectWriter<'_>>,
    ) -> Result<(ControlOutput, u64), ProtocolError> {
        let sequence = current.sequence;
        let publish = |writer: &super::open_project::ProjectWriter<'_>, committed| {
            self.publish_commit(writer, committed).0
        };
        let output = match command {
            RuntimeCommand::HostStatus(_) => ControlOutput::HostStatus(HostStatus {
                instance_id: self.identity().instance_id.clone(),
                pid: self.identity().pid,
                safe_mode: self.safe_mode,
                data_root: self.data_root.to_string_lossy().into_owned(),
                runtime_generation: self.audio.runtime_generation(),
            }),
            RuntimeCommand::HostInfo(_) => ControlOutput::HostInfo(HostInfo {
                instance_id: self.identity().instance_id.clone(),
                pid: self.identity().pid,
                data_root: self.data_root.to_string_lossy().into_owned(),
                project_name: current.session.project_name,
                safe_mode: self.safe_mode,
                runtime_state: self.audio.status().map_err(audio_error)?.state,
            }),
            RuntimeCommand::HostBootstrap(_) => ControlOutput::HostBootstrap(Box::new(
                self.bootstrap()
                    .map_err(|error| command_error(error.to_string()))?,
            )),
            RuntimeCommand::HostShutdown(_) => {
                self.shutdown_requested.store(true, Ordering::Release);
                self.lifecycle.request_shutdown();
                ControlOutput::Ok(())
            }

            RuntimeCommand::RuntimeProjectionGet(_) => {
                ControlOutput::RuntimeProjection(self.runtime.status())
            }
            RuntimeCommand::RuntimeProjectionRetry(_) => {
                let target = self
                    .canonical()
                    .map_err(|error| command_error(error.to_string()))?;
                let project_id = self.project.read().canonical.project_id.clone();
                if self.safe_mode {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps runtime projection offline",
                    ));
                }
                self.run_audio_transition(|state| {
                    project::apply_project_runtime_transition(state, &target, &project_id)
                })?;
                return Ok((
                    ControlOutput::RuntimeProjection(self.runtime.status()),
                    target.sequence,
                ));
            }
            RuntimeCommand::TransportPlay(_) => {
                self.ensure_transport_online()?;
                let outcome = self
                    .runtime
                    .request_play_when_ready(riffra_core::ProjectionKey {
                        sequence: current.sequence,
                        session_revision: current.session.arrangement.revision,
                    })
                    .map_err(runtime_error)?;
                if outcome == crate::runtime::PlayStart::Stalled {
                    // Nothing in flight can produce the requested key anymore;
                    // only a canonical resubmission returns the runtime to a
                    // playable graph.
                    commit::project_committed(
                        current.clone(),
                        self.runtime.as_ref(),
                        &self.data_root,
                        self.built_in_instruments.as_ref(),
                        self.safe_mode,
                    );
                }
                ControlOutput::Ok(())
            }
            RuntimeCommand::TransportStop(_) => {
                self.ensure_transport_online()?;
                self.runtime.stop().map_err(runtime_error)?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::TransportGoToStart(_) => {
                self.ensure_transport_online()?;
                self.runtime
                    .stop_and_seek_to_start(|| {
                        self.audio.seek_timeline(0).map_err(RuntimeError::from)
                    })
                    .map_err(runtime_error)?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::TransportSeek(params) => {
                self.ensure_transport_online()?;
                self.audio.seek_timeline(params.tick).map_err(audio_error)?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::MasterGainPreview(params) => {
                if !params.gain_db.is_finite() {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidRequest,
                        "master gain must be finite",
                    ));
                }
                self.audio
                    .preview_master_gain_db(params.gain_db)
                    .map_err(audio_error)?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::TrackMixPreview(params) => {
                if params.track_id.trim().is_empty() {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidRequest,
                        "track id is required",
                    ));
                }
                if params.gain_db.is_none() && params.pan.is_none() {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidRequest,
                        "at least one of gainDb or pan is required",
                    ));
                }
                if params.gain_db.is_some_and(|value| !value.is_finite())
                    || params.pan.is_some_and(|value| !value.is_finite())
                {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidRequest,
                        "track mix values must be finite",
                    ));
                }
                self.audio
                    .preview_track_mix(&params.track_id, params.gain_db, params.pan)
                    .map_err(audio_error)?;
                ControlOutput::Ok(())
            }

            RuntimeCommand::AudioStatus(_) => audio_status(self.audio.status())?,
            RuntimeCommand::AudioDiagnostics(params) => {
                ControlOutput::AudioDiagnostics(self.audio_diagnostics(params.debug)?)
            }
            RuntimeCommand::AudioProbe(_) => {
                if self.safe_mode {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps audio device probing offline",
                    ));
                }
                ControlOutput::AudioProbe(
                    self.audio
                        .probe_devices(std::time::Duration::from_secs(10))
                        .map_err(command_error)?,
                )
            }
            RuntimeCommand::AudioChannelsProbe(params) => {
                if self.safe_mode {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps audio channel probing offline",
                    ));
                }
                ControlOutput::DeviceChannels(
                    self.audio
                        .probe_device_channels(
                            &params.driver,
                            &params.input_device,
                            &params.output_device,
                            std::time::Duration::from_secs(10),
                        )
                        .map_err(command_error)?,
                )
            }
            RuntimeCommand::AudioRecover(_) => {
                self.ensure_external_devices_online()?;
                ControlOutput::AudioStatus(Box::new(
                    self.recover_audio_device()
                        .map_err(|error| command_error(error.to_string()))?,
                ))
            }
            RuntimeCommand::AudioStartupRetry(_) => {
                self.ensure_external_devices_online()?;
                ControlOutput::AudioStatus(Box::new(
                    self.retry_runtime_startup()
                        .map_err(|error| command_error(error.to_string()))?,
                ))
            }
            RuntimeCommand::AudioDriverGet(_) => ControlOutput::AudioDriver(
                self.audio_preferences
                    .lock()
                    .map_err(|_| command_error("audio preferences lock was poisoned"))?
                    .as_driver_config(),
            ),
            RuntimeCommand::AudioDriverSet(config) => {
                self.ensure_external_devices_online()?;
                ControlOutput::AudioStatus(Box::new(self.set_audio_driver(config)?))
            }
            RuntimeCommand::EmergencyMute(params) => {
                audio_status(self.audio.set_emergency_mute_from_user(params.muted))?
            }
            RuntimeCommand::FeedbackProtectionReset(_) => {
                audio_status(self.audio.reset_feedback_protection())?
            }
            RuntimeCommand::AssetPreview(params) => {
                if self.safe_mode {
                    return Err(runtime_unavailable("Safe Mode blocks live sample preview"));
                }
                let asset_id =
                    riffra_core::AssetId::from_normalized(&params.asset_id).map_err(|error| {
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
                    })?;
                ControlOutput::AudioStatus(Box::new(
                    crate::asset::application::preview_asset(
                        &AssetPreviewContext {
                            audio: &self.audio,
                            data_root: &self.data_root,
                            safe_mode: false,
                        },
                        asset_id,
                        AssetPreviewOptions {
                            start_ms: params.start_ms,
                            end_ms: params.end_ms,
                            looped: params.looped,
                            gain: params.gain,
                        },
                    )
                    .map_err(command_error)?,
                ))
            }
            RuntimeCommand::AssetPreviewStop(_) => audio_status(self.audio.stop_preview())?,
            RuntimeCommand::InstrumentPreview(params) => {
                if self.safe_mode {
                    return Err(runtime_unavailable("Safe Mode blocks instrument preview"));
                }
                let preview = resolve_instrument_preview(
                    &self.data_root,
                    &self.binaries.sonalloy,
                    self.built_in_instruments.as_ref(),
                    &params.instrument_id,
                )?;
                audio_status(self.audio.preview_instrument(
                    &preview.definition_json,
                    &preview.definition_base_dir,
                    &preview.preview,
                ))?
            }
            RuntimeCommand::InstrumentPreviewStop(_) => {
                audio_status(self.audio.stop_instrument_preview())?
            }

            RuntimeCommand::MidiListeningEnable(_) => {
                if self.safe_mode {
                    return Err(runtime_unavailable(
                        "Safe Mode blocks MIDI input; offline MIDI remains available",
                    ));
                }
                audio_status(self.audio.enable_midi_listening())?
            }
            RuntimeCommand::MidiListeningDisable(_) => {
                audio_status(self.audio.disable_midi_listening())?
            }
            RuntimeCommand::MidiSend(params) => {
                self.ensure_midi_output_online()?;
                self.audio
                    .send_track_midi(&params.track_id, &params.bytes)
                    .map_err(audio_error)?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::MidiTargetSet(params) => {
                self.audio
                    .set_live_midi_target(params.track_id.as_deref())
                    .map_err(audio_error)?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::MidiPanic(params) => {
                self.ensure_midi_output_online()?;
                self.audio
                    .panic_track_midi(&params.track_id)
                    .map_err(audio_error)?;
                ControlOutput::Ok(())
            }

            RuntimeCommand::PluginCatalogList(_) => {
                ControlOutput::Plugins(plugins::load(&self.data_root).map_err(|error| {
                    command_error(format!("plugin catalog could not be loaded: {error}"))
                })?)
            }
            RuntimeCommand::PluginScan(params) => {
                self.ensure_plugin_discovery_online()?;
                let root = params
                    .path
                    .map(PathBuf::from)
                    .unwrap_or_else(default_plugin_root);
                ControlOutput::PluginScan(
                    self.scan_plugins(root)
                        .map_err(|error| command_error(format!("plugin scan failed: {error}")))?,
                )
            }
            RuntimeCommand::PluginScanStart(params) => {
                self.ensure_plugin_discovery_online()?;
                let root = params
                    .path
                    .map(PathBuf::from)
                    .unwrap_or_else(default_plugin_root);
                ControlOutput::Job(Some(self.start_plugin_scan(root).map_err(|error| {
                    command_error(format!("plugin scan could not start: {error}"))
                })?))
            }
            RuntimeCommand::PluginEditorOpen(params) => {
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                session_adapter::open_track_plugin_editor(
                    &mut context,
                    &params.track_id,
                    &params.device_id,
                )
                .map_err(|error| error.protocol_error())?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::DeviceInspect(params) => {
                match canonical_built_in_instrument_inspection(
                    &current,
                    &params.track_id,
                    &params.device_id,
                )? {
                    Some(inspection) => ControlOutput::DeviceInspection(inspection),
                    None => ControlOutput::DeviceInspection(
                        self.audio
                            .inspect_track_device(&params.track_id, &params.device_id)
                            .map_err(audio_error)?,
                    ),
                }
            }
            RuntimeCommand::DeviceParameterList(params) => {
                let _ = canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                ControlOutput::DeviceParameters(
                    self.audio
                        .list_track_device_parameters(&params.track_id, &params.device_id)
                        .map_err(audio_error)?,
                )
            }
            RuntimeCommand::DeviceParameterGet(params) => {
                let _ = canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                ControlOutput::DeviceParameter(
                    self.audio
                        .list_track_device_parameters(&params.track_id, &params.device_id)
                        .map_err(audio_error)?
                        .into_iter()
                        .find(|parameter| parameter.index == params.parameter_index)
                        .ok_or_else(|| {
                            command_error(format!(
                                "plugin parameter is not registered: {}",
                                params.parameter_index
                            ))
                        })?,
                )
            }
            RuntimeCommand::PluginPresetList(params) => {
                let _ = canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let presets = self
                    .audio
                    .list_track_plugin_programs(&params.track_id, &params.device_id)
                    .map_err(audio_error)?
                    .presets;
                if presets.is_empty() {
                    return Err(command_error(
                        "plugin does not expose host-visible programs",
                    ));
                }
                ControlOutput::PluginPresets(presets)
            }
            RuntimeCommand::PluginPresetGet(params) => {
                let _ = canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let programs = self
                    .audio
                    .list_track_plugin_programs(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let current_index = programs.current_index.ok_or_else(|| {
                    command_error("plugin does not expose a current host-visible program")
                })?;
                ControlOutput::PluginPreset(
                    programs
                        .presets
                        .into_iter()
                        .find(|preset| preset.index == current_index)
                        .ok_or_else(|| command_error("plugin current program is not registered"))?,
                )
            }
            RuntimeCommand::PluginPresetSet(params) => {
                if params.preset.is_some() == params.preset_index.is_some() {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidRequest,
                        "exactly one of preset or presetIndex is required",
                    ));
                }
                let (plugin_path, bypassed) =
                    canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let programs = self
                    .audio
                    .list_track_plugin_programs(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let program_index = resolve_plugin_preset(
                    &programs.presets,
                    params.preset.as_deref(),
                    params.preset_index,
                )?;
                let previous_program = programs.current_index;
                let previous_state = self
                    .audio
                    .get_track_plugin_state(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                let rollback = || {
                    if let Some(previous_program) = previous_program {
                        let _ = self.audio.set_track_plugin_program(
                            &params.track_id,
                            &params.device_id,
                            previous_program,
                        );
                    }
                    let _ = self.audio.set_track_plugin_state(
                        &params.track_id,
                        &params.device_id,
                        previous_state.clone(),
                    );
                };
                let state = self
                    .audio
                    .set_track_plugin_program(&params.track_id, &params.device_id, program_index)
                    .map_err(audio_error)?;
                let state_snapshot = match plugin_state_snapshot(&plugin_path, state) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        rollback();
                        return Err(error);
                    }
                };
                let commit = context.commit(|mut app| {
                    app.persist_track_plugin_state(
                        &params.track_id,
                        &params.device_id,
                        state_snapshot.parameter_values.clone(),
                        state_snapshot.state_data.clone(),
                        bypassed,
                    )
                });
                if let Err(error) = commit {
                    rollback();
                    return Err(error.protocol_error());
                }
                return adapter_mutation(commit.map(|(mutation, _)| mutation));
            }
            RuntimeCommand::PluginStateGet(params) => {
                let (plugin_path, _) =
                    canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let state = self
                    .audio
                    .get_track_plugin_state(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                ControlOutput::PluginState(plugin_state_snapshot(&plugin_path, state)?)
            }
            RuntimeCommand::PluginStateSet(params) => {
                let (plugin_path, bypassed) =
                    canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                validate_plugin_state(&params.state, &plugin_path)?;
                let previous_state = self
                    .audio
                    .get_track_plugin_state(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let native_state = plugin_state_value(&params.state, bypassed);
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                self.audio
                    .set_track_plugin_state(&params.track_id, &params.device_id, native_state)
                    .map_err(audio_error)?;
                let commit = context.commit(|mut app| {
                    app.persist_track_plugin_state(
                        &params.track_id,
                        &params.device_id,
                        params.state.parameter_values.clone(),
                        params.state.state_data.clone(),
                        bypassed,
                    )
                });
                if let Err(error) = commit {
                    let _ = self.audio.set_track_plugin_state(
                        &params.track_id,
                        &params.device_id,
                        previous_state,
                    );
                    return Err(error.protocol_error());
                }
                return adapter_mutation(commit.map(|(mutation, _)| mutation));
            }

            RuntimeCommand::MissingList(_) => {
                ControlOutput::Missing(missing::collect_missing(&self.data_root, &current.session))
            }

            RuntimeCommand::RecordStart(params) => {
                if self.safe_mode {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps recording input offline",
                    ));
                }
                let context = self.recording_context()?;
                ControlOutput::AudioStatus(Box::new(
                    match params.recording_session_id.as_deref() {
                        Some(id) => recording::record_another_take(&context, id),
                        None => recording::start_recording(&context),
                    }
                    .map_err(command_error)?,
                ))
            }
            RuntimeCommand::RecordStop(_) => {
                let context = self.recording_context()?;
                let result = recording::stop_recording(&context).map_err(command_error)?;
                let stopped_sequence = result.canonical.sequence;
                return Ok((
                    ControlOutput::RecordingStop(Box::new(result)),
                    stopped_sequence,
                ));
            }
            RuntimeCommand::RecordStatus(_) => {
                let context = self.recording_context()?;
                ControlOutput::AudioStatus(Box::new(
                    context
                        .audio
                        .refresh_status()
                        .map_err(|error| command_error(error.to_string()))?,
                ))
            }
            RuntimeCommand::RecordList(params) => {
                let context = self.recording_context()?;
                ControlOutput::Recordings(
                    recording::list_recordings(&context, params.query.as_deref())
                        .map_err(command_error)?,
                )
            }
            RuntimeCommand::RecordRename(params) => {
                let context = self.recording_context()?;
                ControlOutput::RecordingId(
                    recording::rename_recording(&context, &params.id, &params.new_name)
                        .map_err(command_error)?,
                )
            }
            RuntimeCommand::RecordArchive(params) => {
                let context = self.recording_context()?;
                ControlOutput::RecordingId(
                    recording::archive_recording(&context, &params.id).map_err(command_error)?,
                )
            }
            RuntimeCommand::RecordPromote(params) => {
                let context = self.recording_context()?;
                ControlOutput::RecordingId(
                    recording::promote_recording(&context, &params.id).map_err(command_error)?,
                )
            }
            RuntimeCommand::RecordTag(params) => {
                let context = self.recording_context()?;
                ControlOutput::LibraryAsset(
                    recording::tag_recording(&context, &params.id, params.tag, params.note)
                        .map_err(command_error)?,
                )
            }
            RuntimeCommand::RecordDelete(params) => {
                let context = self.recording_context()?;
                recording::delete_recording(&context, &params.id).map_err(command_error)?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::RecordDuplicates(_) => {
                let context = self.recording_context()?;
                ControlOutput::RecordingDuplicates(
                    recording::detect_duplicate_recordings(&context).map_err(command_error)?,
                )
            }
            RuntimeCommand::TakeActivate(params) => {
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                return adapter_mutation(session_adapter::activate_take(
                    &mut context,
                    &params.session_id,
                    &params.take_id,
                ));
            }
            RuntimeCommand::TakePlaceSeparateClip(params) => {
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                return adapter_mutation(session_adapter::place_take_as_separate_clip(
                    &mut context,
                    &params.take_id,
                ));
            }
            RuntimeCommand::TakeVariantSet(params) => {
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                return adapter_mutation(session_adapter::set_audio_clip_take_variant(
                    &mut context,
                    &params.clip_id,
                    params.variant,
                ));
            }
            RuntimeCommand::TakeComparisonStart(params) => {
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                ControlOutput::AudioStatus(Box::new(
                    session_adapter::start_take_comparison(&mut context, &params.take_id)
                        .map_err(|error| error.protocol_error())?,
                ))
            }
            RuntimeCommand::TakeComparisonSwitch(params) => {
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                ControlOutput::AudioStatus(Box::new(
                    session_adapter::switch_take_comparison_variant(&mut context, params.variant)
                        .map_err(|error| error.protocol_error())?,
                ))
            }
            RuntimeCommand::TakeComparisonStop(_) => {
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                ControlOutput::AudioStatus(Box::new(
                    session_adapter::stop_take_comparison(&mut context)
                        .map_err(|error| error.protocol_error())?,
                ))
            }
            RuntimeCommand::ProjectRestoreGeneration(params) => {
                let mut context = self.session_context(writer.take(), current.clone(), &publish)?;
                return adapter_mutation(session_adapter::restore_generation(
                    &mut context,
                    &params.file_name,
                ));
            }

            RuntimeCommand::RenderStart(params) => {
                ControlOutput::Job(Some(self.start_render(current.session, params.options)?))
            }
            RuntimeCommand::JobGet(params) => ControlOutput::Job(
                self.jobs
                    .status(&params.id)
                    .map(jobs::to_background_status)
                    .transpose()
                    .map_err(command_error)?,
            ),
            RuntimeCommand::JobCancel(params) => ControlOutput::Job(
                self.jobs
                    .cancel(&params.id)
                    .map(jobs::to_background_status)
                    .transpose()
                    .map_err(command_error)?,
            ),
            RuntimeCommand::AnalysisStart(params) => {
                let path = if let Some(asset_id) = params.asset_id {
                    let id = riffra_core::AssetId::from_normalized(&asset_id).map_err(|error| {
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
                    })?;
                    PathBuf::from(
                        crate::asset::resolve_content_location(&self.data_root, &id).ok_or_else(
                            || command_error(format!("asset is not available: {id}")),
                        )?,
                    )
                } else {
                    params.path.map(PathBuf::from).ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::InvalidRequest,
                            "analysis requires assetId or path",
                        )
                    })?
                };
                ControlOutput::Analysis(analysis::analyze(&path).map_err(command_error)?)
            }

            RuntimeCommand::LibrarySearch(params) => ControlOutput::Library(
                library::search(&self.data_root, &params.query).map_err(command_error)?,
            ),
            RuntimeCommand::LibraryAssetUpdate(params) => ControlOutput::LibraryAsset(
                library::update_metadata(&self.data_root, &params.id, params.tag, params.note)
                    .map_err(command_error)?,
            ),
            RuntimeCommand::LibraryRelated(params) => ControlOutput::Library(
                library::related(&self.data_root, &params.id).map_err(command_error)?,
            ),
            RuntimeCommand::LibraryInstrumentList(_) => ControlOutput::InstrumentLibrary(
                library::instruments::list(&self.data_root, self.built_in_instruments.as_ref())
                    .map_err(command_error)?,
            ),
            RuntimeCommand::LibraryInstrumentFavoriteSet(params) => {
                ControlOutput::InstrumentLibraryItem(
                    library::instruments::set_favorite(
                        &self.data_root,
                        self.built_in_instruments.as_ref(),
                        &params.instrument_id,
                        params.favorite,
                    )
                    .map_err(command_error)?,
                )
            }
            RuntimeCommand::LibraryInstrumentCategorySet(params) => {
                ControlOutput::InstrumentLibraryItem(
                    library::instruments::set_category_override(
                        &self.data_root,
                        self.built_in_instruments.as_ref(),
                        &params.instrument_id,
                        params.category,
                    )
                    .map_err(command_error)?,
                )
            }
            RuntimeCommand::LibraryInstrumentTagsSet(params) => {
                ControlOutput::InstrumentLibraryItem(
                    library::instruments::set_user_tags(
                        &self.data_root,
                        self.built_in_instruments.as_ref(),
                        &params.instrument_id,
                        params.tags,
                    )
                    .map_err(command_error)?,
                )
            }
            RuntimeCommand::LibraryInstrumentCollectionList(_) => {
                ControlOutput::InstrumentCollections(
                    library::instruments::list_collections(&self.data_root)
                        .map_err(command_error)?,
                )
            }
            RuntimeCommand::LibraryInstrumentCollectionCreate(params) => {
                ControlOutput::InstrumentCollection(
                    library::instruments::create_collection(&self.data_root, params.name)
                        .map_err(command_error)?,
                )
            }
            RuntimeCommand::LibraryInstrumentCollectionRename(params) => {
                ControlOutput::InstrumentCollection(
                    library::instruments::rename_collection(
                        &self.data_root,
                        params.id,
                        params.name,
                    )
                    .map_err(command_error)?,
                )
            }
            RuntimeCommand::LibraryInstrumentCollectionDelete(params) => {
                library::instruments::delete_collection(&self.data_root, params.id)
                    .map_err(command_error)?;
                ControlOutput::Ok(())
            }
            RuntimeCommand::LibraryInstrumentCollectionMembershipSet(params) => {
                ControlOutput::InstrumentLibraryItem(
                    library::instruments::set_collection_membership(
                        &self.data_root,
                        self.built_in_instruments.as_ref(),
                        params.collection_id,
                        &params.instrument_id,
                        params.included,
                    )
                    .map_err(command_error)?,
                )
            }
        };
        Ok((output, sequence))
    }

    fn ensure_transport_online(&self) -> Result<(), ProtocolError> {
        if self.safe_mode {
            return Err(runtime_unavailable(
                "Safe Mode keeps transport playback offline",
            ));
        }
        Ok(())
    }

    fn ensure_external_devices_online(&self) -> Result<(), ProtocolError> {
        if self.safe_mode {
            return Err(runtime_unavailable(
                "Safe Mode keeps external audio devices isolated",
            ));
        }
        Ok(())
    }

    fn ensure_midi_output_online(&self) -> Result<(), ProtocolError> {
        if self.safe_mode {
            return Err(runtime_unavailable("Safe Mode keeps MIDI output offline"));
        }
        Ok(())
    }

    fn ensure_plugin_discovery_online(&self) -> Result<(), ProtocolError> {
        if self.safe_mode {
            return Err(runtime_unavailable(
                "Safe Mode blocks VST3 discovery and load validation",
            ));
        }
        Ok(())
    }

    fn recording_context(self: &Arc<Self>) -> Result<RecordingContext, ProtocolError> {
        let snapshot = self.project.read();
        Ok(RecordingContext {
            host: Arc::clone(self),
            audio: self.audio.clone(),
            runtime: Arc::clone(&self.runtime),
            storage: self
                .project_store
                .session_store(&snapshot.canonical.project_id)
                .map_err(|error| command_error(error.to_string()))?,
            data_root: self.data_root.clone(),
            built_in_instruments: Arc::clone(&self.built_in_instruments),
            events: Arc::clone(&self.events),
            jobs: self.jobs.clone(),
            safe_mode: self.safe_mode,
        })
    }

    fn start_render(
        &self,
        session: riffra_core::CreativeSession,
        options: Option<RenderOptions>,
    ) -> Result<BackgroundJobStatus, ProtocolError> {
        let options = options.unwrap_or_default();
        let data_root = self.data_root.clone();
        let built_in_instruments = Arc::clone(&self.built_in_instruments);
        let worker = self.render_worker.clone();
        let jobs = self.jobs.clone();
        let (id, status) = jobs.start(JobKind::Render);
        let Some(cancelled) = jobs.cancellation_flag(&id) else {
            return Err(command_error("render job could not be registered"));
        };
        let job_id = id.clone();
        let worker_jobs = jobs.clone();
        jobs.spawn_worker(&id, "riffra-render-job", move || {
            worker_jobs.set_running(&job_id, "Rendering the canonical arrangement.");
            match render::render_timeline_with_cancellation(
                &worker,
                &data_root,
                built_in_instruments.as_ref(),
                &session,
                riffra_host::now_ms(),
                options,
                cancelled.as_ref(),
            ) {
                Ok(result) => match serde_json::to_value(result) {
                    Ok(value) => worker_jobs.complete(&job_id, value, "Offline render completed."),
                    Err(error) => jobs::fail(&worker_jobs, &data_root, &job_id, error.to_string()),
                },
                Err(error) => jobs::fail(&worker_jobs, &data_root, &job_id, error),
            }
        })
        .map_err(|error| command_error(format!("render job could not start: {error}")))?;
        jobs::to_background_status(status).map_err(command_error)
    }

    pub(crate) fn publish_commit<T>(
        &self,
        writer: &super::open_project::ProjectWriter<'_>,
        committed: super::open_project::Committed<T>,
    ) -> (ArrangementMutationResult, T) {
        let canonical = committed.snapshot.canonical.clone();
        library::index::refresh(
            &self.data_root,
            &writer.project().storage,
            &canonical.session,
        );
        self.events
            .emit(HostEvent::CanonicalStateChanged(canonical.clone()));
        let mutation = commit::project_committed(
            canonical,
            self.runtime.as_ref(),
            &self.data_root,
            self.built_in_instruments.as_ref(),
            self.safe_mode,
        );
        (mutation, committed.value)
    }
}

fn mutation_output(mutation: ArrangementMutationResult) -> (ControlOutput, u64) {
    let sequence = mutation.canonical.sequence;
    (ControlOutput::ArrangementMutation(mutation), sequence)
}

fn adapter_mutation(
    mutation: Result<ArrangementMutationResult, crate::session::error::AdapterError>,
) -> Result<(ControlOutput, u64), ProtocolError> {
    mutation
        .map(mutation_output)
        .map_err(|error| error.protocol_error())
}

fn audio_status(
    status: crate::NativeAudioResult<AudioStatus>,
) -> Result<ControlOutput, ProtocolError> {
    Ok(ControlOutput::AudioStatus(Box::new(
        status.map_err(audio_error)?,
    )))
}

pub(super) fn command_error(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::CommandFailed, message)
}

fn runtime_unavailable(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
}

pub(super) fn runtime_error(error: RuntimeError) -> ProtocolError {
    match error {
        RuntimeError::RuntimeUnavailable(message) => {
            ProtocolError::new(ErrorCode::RuntimeUnavailable, message).with_details(
                serde_json::json!({
                    "domain": "runtime",
                    "kind": "runtimeUnavailable",
                    "operation": "runtime",
                }),
            )
        }
        RuntimeError::ShuttingDown => {
            ProtocolError::new(ErrorCode::RuntimeUnavailable, "runtime is shutting down")
                .with_details(serde_json::json!({
                    "domain": "runtime",
                    "kind": "shuttingDown",
                    "operation": "runtime.shutdown",
                }))
        }
        RuntimeError::Native {
            kind,
            message,
            operation,
            details,
        } => {
            let code = match kind.as_str() {
                "deviceLost" | "transportLost" | "process" | "safeMode" => {
                    ErrorCode::RuntimeUnavailable
                }
                _ => ErrorCode::CommandFailed,
            };
            ProtocolError::new(code, message).with_details(serde_json::json!({
                "domain": "nativeAudio",
                "kind": kind,
                "operation": operation,
                "details": details,
            }))
        }
        RuntimeError::Timeout { message } => {
            ProtocolError::new(ErrorCode::CommandFailed, message.clone()).with_details(
                serde_json::json!({
                    "domain": "runtime",
                    "kind": "projectionTimeout",
                    "operation": "runtime.projection.prepare",
                }),
            )
        }
        RuntimeError::TransportLost { message } => {
            ProtocolError::new(ErrorCode::RuntimeUnavailable, message.clone()).with_details(
                serde_json::json!({
                    "domain": "runtime",
                    "kind": "transportLost",
                    "operation": "runtime.transport",
                }),
            )
        }
        RuntimeError::GenerationChanged { expected, actual } => ProtocolError::new(
            ErrorCode::RuntimeUnavailable,
            format!("runtime generation changed (expected {expected}, actual {actual})"),
        )
        .with_details(serde_json::json!({
            "domain": "runtime",
            "kind": "generationChanged",
            "operation": "runtime.generation",
            "expected": expected,
            "actual": actual,
        })),
        RuntimeError::Superseded { message } => {
            ProtocolError::new(ErrorCode::CommandFailed, message).with_details(serde_json::json!({
                "domain": "runtime",
                "kind": "superseded",
                "operation": "runtime.projection",
            }))
        }
        RuntimeError::Cancelled { message } => {
            ProtocolError::new(ErrorCode::CommandFailed, message).with_details(serde_json::json!({
                "domain": "runtime",
                "kind": "cancelled",
                "operation": "runtime.transport",
            }))
        }
        RuntimeError::NativeRejected(message) => {
            ProtocolError::new(ErrorCode::CommandFailed, message).with_details(serde_json::json!({
                "domain": "runtime",
                "kind": "projectionRejected",
                "operation": "runtime.projection.prepare",
            }))
        }
        RuntimeError::Internal(message) => ProtocolError::new(ErrorCode::CommandFailed, message)
            .with_details(serde_json::json!({
                "domain": "runtime",
                "kind": "internal",
                "operation": "runtime",
            })),
    }
}

pub(super) fn audio_error(error: crate::NativeAudioError) -> ProtocolError {
    let descriptor = error.descriptor();
    let code = match descriptor.kind.as_str() {
        "deviceLost" | "transportLost" | "generationChanged" | "process" | "safeMode" => {
            ErrorCode::RuntimeUnavailable
        }
        _ => ErrorCode::CommandFailed,
    };
    ProtocolError::new(code, descriptor.message).with_details(serde_json::json!({
        "domain": "nativeAudio",
        "kind": descriptor.kind,
        "operation": descriptor.operation,
        "details": descriptor.details,
    }))
}

fn canonical_plugin_device(
    canonical: &CanonicalState,
    track_id: &str,
    device_id: &str,
) -> Result<(String, bool), ProtocolError> {
    let track = canonical
        .session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| command_error(format!("track is not registered: {track_id}")))?;
    if let Some(instrument) = track
        .instrument
        .as_ref()
        .filter(|instrument| instrument.id == device_id)
    {
        let Some(vst3) = instrument.as_vst3() else {
            return Err(command_error(
                "built-in instruments do not expose VST3 plugin controls",
            ));
        };
        return Ok((vst3.path.to_owned(), instrument.bypassed));
    }
    let device = track
        .effects
        .iter()
        .find(|device| device.id == device_id)
        .ok_or_else(|| command_error(format!("track device is not registered: {device_id}")))?;
    Ok((device.plugin.path.clone(), device.bypassed))
}

/// Returns the canonical inspection of a built-in instrument, or `None` for a
/// VST3 device that only the native runtime can inspect.
fn canonical_built_in_instrument_inspection(
    canonical: &CanonicalState,
    track_id: &str,
    device_id: &str,
) -> Result<Option<DeviceInspection>, ProtocolError> {
    let track = canonical
        .session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| command_error(format!("track is not registered: {track_id}")))?;
    if let Some(instrument) = track
        .instrument
        .as_ref()
        .filter(|instrument| instrument.id == device_id)
    {
        if instrument.as_vst3().is_some() {
            return Ok(None);
        }
        return Ok(Some(DeviceInspection {
            id: instrument.id.clone(),
            name: instrument.name.clone(),
            source: "builtin".into(),
            bypassed: instrument.bypassed,
            capabilities: DeviceCapabilities::default(),
            parameter_count: 0,
            state_persisted: false,
        }));
    }
    if !track.effects.iter().any(|device| device.id == device_id) {
        return Err(command_error(format!(
            "track device is not registered: {device_id}"
        )));
    }
    Ok(None)
}

fn plugin_state_snapshot(
    plugin_path: &str,
    state: GraphPluginState,
) -> Result<PluginStateSnapshot, ProtocolError> {
    let snapshot = PluginStateSnapshot {
        schema_version: 1,
        plugin_path: plugin_path.into(),
        parameter_values: state.parameter_values,
        state_data: state.state_data,
    };
    if snapshot
        .parameter_values
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err(command_error(
            "Native plugin state contained a non-finite parameter value",
        ));
    }
    Ok(snapshot)
}

fn plugin_state_value(state: &PluginStateSnapshot, bypassed: bool) -> GraphPluginState {
    GraphPluginState {
        state_data: state.state_data.clone(),
        parameter_values: state.parameter_values.clone(),
        bypassed,
    }
}

fn validate_plugin_state(
    state: &PluginStateSnapshot,
    plugin_path: &str,
) -> Result<(), ProtocolError> {
    if state.schema_version != 1 {
        return Err(ProtocolError::new(
            ErrorCode::InvalidRequest,
            "plugin state schemaVersion must be 1",
        ));
    }
    if state.plugin_path != plugin_path {
        return Err(command_error("plugin state belongs to a different VST3"));
    }
    if state
        .parameter_values
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err(ProtocolError::new(
            ErrorCode::InvalidRequest,
            "plugin state parameter values must be finite",
        ));
    }
    Ok(())
}

fn resolve_plugin_preset(
    presets: &[PluginPresetInfo],
    name: Option<&str>,
    index: Option<u32>,
) -> Result<u32, ProtocolError> {
    if let Some(index) = index {
        if presets.iter().any(|preset| preset.index == index) {
            return Ok(index);
        }
        return Err(command_error(format!(
            "plugin preset is not registered: {index}"
        )));
    }
    let name = name.expect("preset name or index was validated");
    let matches = presets
        .iter()
        .filter(|preset| preset.name == name)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [preset] => Ok(preset.index),
        [] => Err(command_error(format!(
            "plugin preset is not registered: {name}"
        ))),
        _ => Err(command_error(format!(
            "plugin preset name is ambiguous: {name}"
        ))),
    }
}

#[derive(Debug)]
struct ResolvedInstrumentPreview {
    definition_json: String,
    definition_base_dir: PathBuf,
    preview: InstrumentPreviewDefinition,
}

fn resolve_instrument_preview(
    data_root: &std::path::Path,
    sonalloy: &std::path::Path,
    built_in_instruments: &BuiltInInstrumentCatalog,
    instrument_id: &str,
) -> Result<ResolvedInstrumentPreview, ProtocolError> {
    if let Some(preset_id) = instrument_id.strip_prefix("builtin:") {
        let definition = built_in_instruments
            .resolve(preset_id)
            .map_err(command_error)?;
        return Ok(ResolvedInstrumentPreview {
            definition_json: definition.definition_json.clone(),
            definition_base_dir: definition.base_dir.clone(),
            preview: definition.summary.preview.clone(),
        });
    }
    if instrument_id.starts_with("user:") {
        let instrument = UserInstrumentStore::new(data_root, sonalloy)
            .resolve(instrument_id)
            .map_err(command_error)?;
        let preview = instrument.preview.ok_or_else(|| {
            command_error(format!(
                "instrument '{instrument_id}' does not have a preview"
            ))
        })?;
        return Ok(ResolvedInstrumentPreview {
            definition_json: instrument.definition_json,
            definition_base_dir: instrument.package_root,
            preview,
        });
    }
    Err(command_error(format!(
        "instrument preview requires a builtin: or user: instrument ID: {instrument_id}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::{
        HelloRequest, HelloResponse, LocalHostClient, LocalHostRegistry, endpoint_path,
        new_instance_id, read_endpoint, transport,
    };

    #[test]
    fn resolves_builtin_and_user_previews_with_their_resource_base_directories() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-preview-resolution-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let builtin_root = data_root.join("built-in-instruments");
        let builtin_package = builtin_root.join("01-bass");
        std::fs::create_dir_all(&builtin_package).unwrap();
        std::fs::write(builtin_package.join("definition.json"), br#"{}"#).unwrap();
        std::fs::write(
            builtin_root.join("manifest.json"),
            br#"{"sourceRelease":"vtest","presets":[{"id":"01-bass","name":"Bass","author":"Riffra","description":"Low","category":"Bass","tags":["Low"],"recommendedRange":{"minMidi":36,"maxMidi":84},"preview":{"tempoBpm":120,"ticksPerBeat":480,"timeSignature":{"numerator":4,"denominator":4},"lengthTicks":1920,"notes":[{"tick":0,"durationTicks":480,"note":48,"velocity":100}]},"definitionPath":"01-bass/definition.json","resourceBasePath":"01-bass"}]}"#,
        )
        .unwrap();
        let user_uuid = new_instance_id();
        let user_id = format!("user:{user_uuid}");
        let user_package = data_root.join("instruments/user").join(&user_uuid);
        std::fs::create_dir_all(&user_package).unwrap();
        let user_definition = r#"{"metadata":{"name":"Glass Current","preview":{"tempo_bpm":100,"ticks_per_beat":480,"time_signature":{"numerator":4,"denominator":4},"length_ticks":1920,"notes":[{"tick":0,"duration_ticks":480,"note":60,"velocity":96}]}}}"#;
        std::fs::write(user_package.join("definition.json"), user_definition).unwrap();
        std::fs::write(
            user_package.join(".riffra-instrument.json"),
            serde_json::json!({
                "formatVersion": 1,
                "instrumentId": user_id.clone(),
                "definitionPath": "definition.json",
                "createdAtMs": 1,
                "updatedAtMs": 1
            })
            .to_string(),
        )
        .unwrap();
        let sonalloy = data_root.join("sonalloy");
        let catalog = BuiltInInstrumentCatalog::load(&builtin_root).unwrap();

        let builtin =
            resolve_instrument_preview(&data_root, &sonalloy, &catalog, "builtin:01-bass").unwrap();
        assert_eq!(builtin.definition_base_dir, builtin_package);
        assert_eq!(builtin.preview.tempo_bpm, 120.0);

        let user = resolve_instrument_preview(&data_root, &sonalloy, &catalog, &user_id).unwrap();
        assert_eq!(user.definition_base_dir, user_package);
        assert_eq!(user.definition_json, user_definition);
        assert_eq!(user.preview.tempo_bpm, 100.0);

        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn rejects_user_preview_requests_without_a_preview() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-preview-missing-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let user_uuid = new_instance_id();
        let user_id = format!("user:{user_uuid}");
        let package = data_root.join("instruments/user").join(&user_uuid);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("definition.json"),
            br#"{"metadata":{"name":"Haze Chord"}}"#,
        )
        .unwrap();
        std::fs::write(
            package.join(".riffra-instrument.json"),
            serde_json::json!({
                "formatVersion": 1,
                "instrumentId": user_id.clone(),
                "definitionPath": "definition.json",
                "createdAtMs": 1,
                "updatedAtMs": 1
            })
            .to_string(),
        )
        .unwrap();
        let sonalloy = data_root.join("sonalloy");
        let catalog =
            BuiltInInstrumentCatalog::load(crate::test_support::prepare_built_in_resource_root(
                &data_root.join("built-in-instruments"),
            ))
            .unwrap();

        let error =
            resolve_instrument_preview(&data_root, &sonalloy, &catalog, &user_id).unwrap_err();
        assert!(error.message.contains(&user_id));
        assert!(error.message.contains("does not have a preview"));

        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn instrument_library_commands_preserve_canonical_state() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-instrument-library-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let preset_root = data_root.join("built-in-instruments").join("01-bass");
        std::fs::create_dir_all(&preset_root).unwrap();
        std::fs::write(preset_root.join("definition.json"), br#"{}"#).unwrap();
        std::fs::write(
            preset_root.parent().unwrap().join("manifest.json"),
            br#"{"sourceRelease":"vtest","presets":[{"id":"01-bass","name":"Bass","author":"Riffra","description":"Low","category":"Bass","tags":["Low"],"recommendedRange":{"minMidi":36,"maxMidi":84},"preview":{"tempoBpm":120,"ticksPerBeat":480,"timeSignature":{"numerator":4,"denominator":4},"lengthTicks":1920,"notes":[{"tick":0,"durationTicks":480,"note":48,"velocity":100}]},"definitionPath":"01-bass/definition.json","resourceBasePath":"01-bass"}]}"#,
        )
        .unwrap();
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: preset_root.parent().unwrap().to_path_buf(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
                data_root.join("sonalloy"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let before = host.canonical_state().unwrap();

        let dispatch = |command: &str, params: serde_json::Value| {
            let response =
                host.dispatch_control(ControlRequest::new(command, command, params, Some(0)));
            assert!(response.ok, "{command}: {:?}", response.error);
            assert_eq!(response.sequence, Some(0), "{command}");
            response.result.unwrap().value
        };

        let listed: Vec<crate::InstrumentLibraryItem> =
            serde_json::from_value(dispatch("library.instrument.list", serde_json::json!({})))
                .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "builtin:01-bass");

        let favorite: crate::InstrumentLibraryItem = serde_json::from_value(dispatch(
            "library.instrument.favorite.set",
            serde_json::json!({"instrumentId":"builtin:01-bass","favorite":true}),
        ))
        .unwrap();
        assert!(favorite.favorite);

        let overridden: crate::InstrumentLibraryItem = serde_json::from_value(dispatch(
            "library.instrument.category.set",
            serde_json::json!({"instrumentId":"builtin:01-bass","category":" Basses "}),
        ))
        .unwrap();
        assert_eq!(overridden.category.as_deref(), Some("Basses"));
        let restored: crate::InstrumentLibraryItem = serde_json::from_value(dispatch(
            "library.instrument.category.set",
            serde_json::json!({"instrumentId":"builtin:01-bass","category":null}),
        ))
        .unwrap();
        assert_eq!(restored.category.as_deref(), Some("Bass"));

        let tagged: crate::InstrumentLibraryItem = serde_json::from_value(dispatch(
            "library.instrument.tags.set",
            serde_json::json!({"instrumentId":"builtin:01-bass","tags":["Verse","verse"," Lead "]}),
        ))
        .unwrap();
        assert_eq!(tagged.user_tags, ["Lead", "Verse"]);

        let created: crate::InstrumentCollection = serde_json::from_value(dispatch(
            "library.instrument.collection.create",
            serde_json::json!({"name":"Live Set"}),
        ))
        .unwrap();
        let member: crate::InstrumentLibraryItem = serde_json::from_value(dispatch(
            "library.instrument.collection.membership.set",
            serde_json::json!({"collectionId":created.id,"instrumentId":"builtin:01-bass","included":true}),
        ))
        .unwrap();
        assert_eq!(member.collection_ids, [created.id]);

        let renamed: crate::InstrumentCollection = serde_json::from_value(dispatch(
            "library.instrument.collection.rename",
            serde_json::json!({"id":created.id,"name":"Rehearsal"}),
        ))
        .unwrap();
        assert_eq!(renamed.name, "Rehearsal");
        let collections: Vec<crate::InstrumentCollection> = serde_json::from_value(dispatch(
            "library.instrument.collection.list",
            serde_json::json!({}),
        ))
        .unwrap();
        assert_eq!(collections, [renamed]);

        let removed: crate::InstrumentLibraryItem = serde_json::from_value(dispatch(
            "library.instrument.collection.membership.set",
            serde_json::json!({"collectionId":created.id,"instrumentId":"builtin:01-bass","included":false}),
        ))
        .unwrap();
        assert!(removed.collection_ids.is_empty());
        let deleted = dispatch(
            "library.instrument.collection.delete",
            serde_json::json!({"id":created.id}),
        );
        assert!(deleted.is_null());
        let collections: Vec<crate::InstrumentCollection> = serde_json::from_value(dispatch(
            "library.instrument.collection.list",
            serde_json::json!({}),
        ))
        .unwrap();
        assert!(collections.is_empty());

        assert_eq!(host.canonical_state().unwrap(), before);
        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn live_host_rejects_project_bound_request_without_expected_project_id() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-missing-project-id-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                &data_root,
            ),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
                data_root.join("sonalloy"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();

        let response = host.dispatch_control(ControlRequest::new(
            "missing-project-id",
            "track.add",
            serde_json::json!({"name": "Rejected", "kind": "instrument"}),
            Some(0),
        ));

        assert!(!response.ok);
        let error = response.error.unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            "expectedProjectId is required for Project-bound commands"
        );
        assert_eq!(
            host.canonical_state()
                .unwrap()
                .session
                .arrangement
                .tracks
                .len(),
            0
        );

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn safe_mode_host_publishes_endpoint_and_handles_attached_mutation() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-host-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                &data_root,
            ),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
                data_root.join("sonalloy"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let expected_project_id = host.bootstrap().unwrap().project_state.active_project_id;
        let descriptor = read_endpoint(&data_root).unwrap();

        {
            let mut stream = transport::connect(descriptor.endpoint()).unwrap();
            transport::write_frame(&mut stream, &HelloRequest::new()).unwrap();
            let hello: HelloResponse = transport::read_frame(&mut stream).unwrap();
            assert_eq!(hello.instance_id, descriptor.instance_id);

            transport::write_frame(
                &mut stream,
                &ControlRequest::new("session-get", "session.get", serde_json::json!({}), Some(0))
                    .with_expected_project_id(expected_project_id.clone()),
            )
            .unwrap();
            let session_response: ControlResponse = transport::read_frame(&mut stream).unwrap();
            assert!(session_response.ok);
            assert_eq!(session_response.sequence, Some(0));
            assert_eq!(
                session_response
                    .result
                    .as_ref()
                    .map(|result| result.result_type.as_str()),
                Some("session")
            );

            let request = ControlRequest::new(
                "host-test",
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
                Some(0),
            )
            .with_expected_project_id(expected_project_id);
            transport::write_frame(&mut stream, &request).unwrap();
            let response: ControlResponse = transport::read_frame(&mut stream).unwrap();
            assert!(response.ok);
            assert_eq!(response.sequence, Some(1));
        }

        assert_eq!(
            host.bootstrap().unwrap().runtime_projection.state,
            crate::RuntimeProjectionState::Idle
        );
        host.shutdown();
        assert!(!endpoint_path(&data_root).exists());
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn stale_render_and_undo_requests_are_rejected_by_the_canonical_sequence() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-sequence-guard-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                &data_root,
            ),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
                data_root.join("sonalloy"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let expected_project_id = host.bootstrap().unwrap().project_state.active_project_id;

        let mutation = host.dispatch_control(
            ControlRequest::new(
                "track-add",
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
                Some(0),
            )
            .with_expected_project_id(expected_project_id.clone()),
        );
        assert!(mutation.ok);
        assert_eq!(mutation.sequence, Some(1));

        let undo = host.dispatch_control(
            ControlRequest::new("stale-undo", "undo", serde_json::json!({}), Some(0))
                .with_expected_project_id(expected_project_id.clone()),
        );
        assert!(!undo.ok);
        assert_eq!(
            undo.error.as_ref().map(|error| error.code),
            Some(ErrorCode::Conflict)
        );

        let render = host.dispatch_control(
            ControlRequest::new(
                "stale-render",
                "render.start",
                serde_json::json!({}),
                Some(0),
            )
            .with_expected_project_id(expected_project_id),
        );
        assert!(!render.ok);
        assert_eq!(
            render.error.as_ref().map(|error| error.code),
            Some(ErrorCode::Conflict)
        );

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn host_info_returns_the_lightweight_selector_payload() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-info-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                &data_root,
            ),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
                data_root.join("sonalloy"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let client = LocalHostClient::connect_data_root(&data_root).unwrap();

        let response = client
            .request(&ControlRequest::new(
                "info",
                "host.info",
                serde_json::json!({}),
                None,
            ))
            .unwrap();

        assert!(response.ok);
        let info = response.result.unwrap().value;
        assert_eq!(info["instanceId"], host.identity().instance_id);
        assert_eq!(info["pid"], host.identity().pid);
        assert_eq!(info["dataRoot"], data_root.to_string_lossy().into_owned());
        assert!(info["projectName"].is_null());
        assert_eq!(info["safeMode"], true);
        assert_eq!(info["runtimeState"], "offline");

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn shared_client_receives_bootstrap_and_canonical_events() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-client-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                &data_root,
            ),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
                data_root.join("sonalloy"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let client = LocalHostClient::connect_data_root(&data_root).unwrap();
        let mut events = client.open_event_stream().unwrap();

        let bootstrap = client
            .request(&ControlRequest::new(
                "bootstrap",
                "host.bootstrap",
                serde_json::json!({}),
                Some(0),
            ))
            .unwrap();
        assert!(bootstrap.ok);
        let bootstrap: HostBootstrap =
            serde_json::from_value(bootstrap.result.unwrap().value).unwrap();
        assert_eq!(bootstrap.canonical.sequence, 0);
        let expected_project_id = bootstrap.project_state.active_project_id;

        let mutation = client
            .request(
                &ControlRequest::new(
                    "track-add",
                    "track.add",
                    serde_json::json!({"name": "Synth", "kind": "instrument"}),
                    Some(0),
                )
                .with_expected_project_id(expected_project_id),
            )
            .unwrap();
        assert!(mutation.ok);
        assert_eq!(
            mutation
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("arrangementMutation")
        );
        let mutation_result: crate::api::output::ArrangementMutationResult =
            serde_json::from_value(mutation.result.unwrap().value).unwrap();
        assert_eq!(mutation_result.canonical.sequence, 1);
        assert!(matches!(
            mutation_result.projection,
            crate::api::output::ArrangementProjectionOutcome::NotRequired
        ));
        let event = events.recv().unwrap();
        assert_eq!(event.event, "canonical-state-changed");
        assert_eq!(event.payload["sequence"], 1);

        let discovered = LocalHostRegistry::current_user()
            .discover(|registration| {
                crate::api::ControlCommand::from(crate::api::RuntimeCommand::HostStatus(
                    crate::api::params::EmptyParams::default(),
                ))
                .into_request(format!("discovery-{}", registration.instance_id), None)
            })
            .unwrap()
            .into_iter()
            .find(|entry| entry.registration.instance_id == host.identity().instance_id);
        assert!(discovered.is_some());
        drop(discovered);

        host.shutdown();
        assert!(!endpoint_path(&data_root).exists());
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn an_open_client_cannot_mutate_after_shutdown_and_the_root_reopens() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-shutdown-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                &data_root,
            ),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
                data_root.join("sonalloy"),
            ),
        };
        let host = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();
        let descriptor = read_endpoint(&data_root).unwrap();
        let mut stream = transport::connect(descriptor.endpoint()).unwrap();
        transport::write_frame(&mut stream, &HelloRequest::new()).unwrap();
        let _: HelloResponse = transport::read_frame(&mut stream).unwrap();

        transport::write_frame(
            &mut stream,
            &ControlRequest::new(
                "shutdown-request",
                "host.shutdown",
                serde_json::json!({}),
                Some(0),
            ),
        )
        .unwrap();
        let shutdown_response: ControlResponse = transport::read_frame(&mut stream).unwrap();
        assert!(shutdown_response.ok);
        transport::write_frame(
            &mut stream,
            &ControlRequest::new(
                "after-shutdown",
                "track.add",
                serde_json::json!({"name": "Rejected", "kind": "audio"}),
                Some(0),
            ),
        )
        .unwrap();
        let response: ControlResponse = transport::read_frame(&mut stream).unwrap();
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::HostUnavailable)
        );
        drop(stream);
        drop(host);

        let reopened = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        assert_eq!(reopened.canonical_state().unwrap().sequence, 0);
        reopened.shutdown();
        drop(reopened);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn normal_host_returns_arrangement_mutation_before_shutdown() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-startup-shutdown-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                &data_root,
            ),
            safe_mode: false,
            binaries: RuntimeBinaries::new(
                data_root.join("missing-riffra-audio"),
                data_root.join("missing-riffra-plugin-scan"),
                data_root.join("missing-riffra-render"),
                data_root.join("missing-sonalloy"),
            ),
        };
        let host = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();
        let expected_project_id = host.bootstrap().unwrap().project_state.active_project_id;

        let response = host.dispatch_control(
            ControlRequest::new(
                "track-add",
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
                Some(0),
            )
            .with_expected_project_id(expected_project_id.clone()),
        );

        assert!(response.ok);
        assert_eq!(
            response
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("arrangementMutation")
        );
        let mutation: crate::api::output::ArrangementMutationResult =
            serde_json::from_value(response.result.unwrap().value).unwrap();
        assert_eq!(mutation.canonical.sequence, 1);
        assert!(matches!(
            mutation.projection,
            crate::api::output::ArrangementProjectionOutcome::Queued
                | crate::api::output::ArrangementProjectionOutcome::Failed { .. }
        ));

        let marker = host.dispatch_control(
            ControlRequest::new(
                "marker-add",
                "marker.add",
                serde_json::json!({"name": "Verse", "position": "1:1"}),
                Some(1),
            )
            .with_expected_project_id(expected_project_id.clone()),
        );
        assert!(marker.ok);
        assert_eq!(
            marker
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("arrangementMutation")
        );
        let marker: crate::api::output::ArrangementMutationResult =
            serde_json::from_value(marker.result.unwrap().value).unwrap();
        assert_eq!(marker.canonical.sequence, 2);
        assert!(matches!(
            marker.projection,
            crate::api::output::ArrangementProjectionOutcome::NotRequired
        ));

        let settings = host.dispatch_control(
            ControlRequest::new(
                "session-settings-update",
                "session.settings.update",
                serde_json::json!({"note": "authoring note"}),
                Some(2),
            )
            .with_expected_project_id(expected_project_id),
        );
        assert!(settings.ok);
        assert_eq!(
            settings
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("arrangementMutation")
        );
        let settings: crate::api::output::ArrangementMutationResult =
            serde_json::from_value(settings.result.unwrap().value).unwrap();
        assert_eq!(settings.canonical.sequence, 3);
        assert!(matches!(
            settings.projection,
            crate::api::output::ArrangementProjectionOutcome::NotRequired
        ));

        host.shutdown();
        drop(host);

        let reopened = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        reopened.shutdown();
        drop(reopened);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn master_gain_set_commits_canonical_state_and_requests_projection() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-master-gain-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                &data_root,
            ),
            safe_mode: false,
            binaries: RuntimeBinaries::new(
                data_root.join("missing-riffra-audio"),
                data_root.join("missing-riffra-plugin-scan"),
                data_root.join("missing-riffra-render"),
                data_root.join("missing-sonalloy"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let expected_project_id = host.bootstrap().unwrap().project_state.active_project_id;

        // Act
        let response = host.dispatch_control(
            ControlRequest::new(
                "master-gain-set",
                "audio.master-gain.set",
                serde_json::json!({"gainDb": -9.0}),
                Some(0),
            )
            .with_expected_project_id(expected_project_id),
        );

        // Assert
        assert!(response.ok);
        let mutation: crate::api::output::ArrangementMutationResult =
            serde_json::from_value(response.result.unwrap().value).unwrap();
        assert_eq!(mutation.canonical.sequence, 1);
        assert_eq!(mutation.canonical.session.settings.master_db, -9.0);
        assert!(matches!(
            mutation.projection,
            crate::api::output::ArrangementProjectionOutcome::Queued
                | crate::api::output::ArrangementProjectionOutcome::Failed { .. }
        ));

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }
}
