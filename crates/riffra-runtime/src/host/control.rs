use super::lifecycle::default_plugin_root;
use super::project;
use super::*;
use crate::runtime_snapshot::runtime_timeline_snapshot;
use std::time::Duration;

impl HostState {
    fn response(
        &self,
        request_id: String,
        result_type: &'static str,
        value: Value,
        sequence: u64,
    ) -> ControlResponse {
        ControlResponse::success(
            request_id,
            sequence,
            CommandResult {
                result_type: result_type.into(),
                value,
            },
        )
    }

    fn failure(request_id: String, error: ProtocolError) -> ControlResponse {
        ControlResponse::failure(request_id, None, error)
    }

    pub(super) fn session_context(&self) -> Result<SessionContext<'_>, ProtocolError> {
        let storage = self
            .project_store
            .active_session_store()
            .map_err(|error| command_error(error.to_string()))?;
        Ok(SessionContext {
            core: self.core.as_ref(),
            audio: self.core.audio(),
            runtime: self.runtime.as_ref(),
            storage,
            data_root: &self.data_root,
            built_in_instruments: self.built_in_instruments.as_ref(),
            safe_mode: self.core.safe_mode(),
            events: self.events.as_ref(),
            project_commit: None,
        })
    }

    pub(super) fn flush_plugin_persistence(&self) -> Result<(), ProtocolError> {
        let project_id = self
            .project_store
            .active_project_id()
            .map_err(|error| command_error(error.to_string()))?;
        let commands = self
            .plugin_persistence_commands
            .lock()
            .map_err(|_| command_error("plugin persistence lock was poisoned"))?;
        if let Some(commands) = commands.as_ref() {
            let (result, receiver) = std::sync::mpsc::channel();
            commands
                .send(super::persistence::PluginPersistenceCommand::FlushProject {
                    project_id,
                    result,
                })
                .map_err(|_| command_error("plugin persistence worker is unavailable"))?;
            let outcome = receiver
                .recv()
                .map_err(|_| command_error("plugin persistence worker stopped unexpectedly"))?;
            outcome.map_err(command_error)?;
        }
        Ok(())
    }

    pub(super) fn keep_plugin_persistence_project(&self, project_id: &str) {
        if let Ok(commands) = self.plugin_persistence_commands.lock()
            && let Some(commands) = commands.as_ref()
        {
            let (result, receiver) = std::sync::mpsc::channel();
            if commands
                .send(super::persistence::PluginPersistenceCommand::KeepProject {
                    project_id: project_id.to_owned(),
                    result,
                })
                .is_ok()
            {
                let _ = receiver.recv();
            }
        }
    }

    fn session_context_with_project_commit(
        &self,
        expected_project_id: Option<String>,
    ) -> Result<SessionContext<'_>, ProtocolError> {
        let storage = self
            .project_store
            .active_session_store()
            .map_err(|error| command_error(error.to_string()))?;
        Ok(SessionContext {
            core: self.core.as_ref(),
            audio: self.core.audio(),
            runtime: self.runtime.as_ref(),
            storage,
            data_root: &self.data_root,
            built_in_instruments: self.built_in_instruments.as_ref(),
            safe_mode: self.core.safe_mode(),
            events: self.events.as_ref(),
            project_commit: expected_project_id.map(|expected_project_id| {
                crate::session::context::ProjectCommitContext {
                    project_store: &self.project_store,
                    command_gate: &self._command_gate,
                    expected_project_id,
                }
            }),
        })
    }

    pub(crate) fn dispatch_request(&self, request: ControlRequest) -> ControlResponse {
        self.dispatch_request_with_shutdown(request, false)
    }

    pub(super) fn dispatch_persistence_request(&self, request: ControlRequest) -> ControlResponse {
        self.dispatch_request_inner(request, true, true)
    }

    fn dispatch_request_with_shutdown(
        &self,
        request: ControlRequest,
        allow_shutdown: bool,
    ) -> ControlResponse {
        let _lifecycle = self
            .lifecycle_gate
            .read()
            .expect("Host lifecycle gate was poisoned");
        self.dispatch_request_inner(request, allow_shutdown, false)
    }

    fn dispatch_request_inner(
        &self,
        request: ControlRequest,
        allow_shutdown: bool,
        bypass_command_gate: bool,
    ) -> ControlResponse {
        if !allow_shutdown && self.shutting_down.load(Ordering::Acquire) {
            return Self::failure(
                request.request_id,
                ProtocolError::new(ErrorCode::HostUnavailable, "Riffra Host has shut down"),
            );
        }
        let _command_gate =
            if !bypass_command_gate && requires_command_gate(request.command.as_str()) {
                Some(
                    self._command_gate
                        .lock()
                        .expect("Host command gate was poisoned"),
                )
            } else {
                None
            };
        let request_id = request.request_id.clone();
        if let Err(error) = request.validate() {
            return Self::failure(request_id, error);
        }
        let current = match self.canonical() {
            Ok(current) => current,
            Err(error) => return Self::failure(request_id, command_error(error.to_string())),
        };
        if let Some(expected_sequence) = request.expected_sequence
            && expected_sequence != current.sequence
        {
            return Self::failure(
                request_id,
                ProtocolError::conflict(expected_sequence, current.sequence),
            );
        }
        let active_project_id = match self.project_store.active_project_id() {
            Ok(project_id) => project_id,
            Err(error) => return Self::failure(request_id, command_error(error.to_string())),
        };
        if let Err(error) = crate::dispatcher::validate_project_precondition(
            &request.command,
            request.expected_project_id.as_deref(),
            &active_project_id,
        ) {
            return Self::failure(request_id, error);
        }
        match self.dispatch(
            request.command.as_str(),
            request.params,
            current,
            request.expected_project_id,
        ) {
            Ok((result_type, value, sequence)) => {
                self.response(request_id, result_type, value, sequence)
            }
            Err(error) => Self::failure(request_id, error),
        }
    }

    fn dispatch(
        &self,
        command: &str,
        params: Value,
        current: CanonicalState,
        expected_project_id: Option<String>,
    ) -> Result<(&'static str, Value, u64), ProtocolError> {
        if command == "audio.master-gain.set" {
            let params: MasterGainParams = decode(params)?;
            let context = self.session_context()?;
            let pair = session_adapter::set_master_gain_db(&context, params.gain_db)
                .map_err(|error| error.protocol_error())?;
            return Ok((
                "sessionAudioPair",
                serde_json::to_value(&pair).map_err(serialize_error)?,
                pair.canonical.sequence,
            ));
        }
        if project::handles(command) {
            return project::dispatch(self, command, params, current);
        }
        if !is_host_runtime_command(command) {
            let project_commit = is_long_project_operation(command)
                .then_some(expected_project_id)
                .flatten();
            if let Some(result) = self.dispatch_shared_session(
                command,
                params.clone(),
                current.sequence,
                project_commit,
            )? {
                return Ok(result);
            }
            let current_sequence = current.sequence;
            let storage = self
                .project_store
                .active_session_store()
                .map_err(|error| command_error(error.to_string()))?;
            let result = HostDispatcher::borrowed(
                &self.core,
                &storage,
                &self.project_store,
                &self.data_root,
                &self.built_in_instruments,
            )
            .dispatch_with_canonical(
                riffra_control::ControlCommand::new(command, params),
                current,
            )
            .map_err(|error| error.protocol_error())?;
            if result.sequence > current_sequence {
                let mutation = self.after_canonical_commit(result.projection_effect())?;
                let sequence = mutation.canonical.sequence;
                return Ok((
                    "arrangementMutation",
                    serde_json::to_value(mutation).map_err(serialize_error)?,
                    sequence,
                ));
            }
            return Ok((result.result_type, result.value, result.sequence));
        }

        match command {
            "device.inspect" => {
                let params: DeviceInspectParams = decode(params)?;
                if let Some(inspection) = canonical_non_plugin_device_inspection(
                    &current,
                    &params.track_id,
                    &params.device_id,
                )? {
                    return Ok((
                        "deviceInspection",
                        serde_json::to_value(inspection).map_err(serialize_error)?,
                        current.sequence,
                    ));
                }
                let value = self
                    .core
                    .audio()
                    .inspect_track_device(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let inspection: crate::model::DeviceInspection =
                    serde_json::from_value(value).map_err(serialize_error)?;
                Ok((
                    "deviceInspection",
                    serde_json::to_value(inspection).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "device.parameter.list" => {
                let params: DeviceParameterListParams = decode(params)?;
                let _ = canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let value = self
                    .core
                    .audio()
                    .list_track_device_parameters(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let parameters = value
                    .get("parameters")
                    .cloned()
                    .ok_or_else(|| command_error("Native plugin response omitted parameters"))?;
                let parameters: Vec<crate::model::DeviceParameterInfo> =
                    serde_json::from_value(parameters).map_err(serialize_error)?;
                Ok((
                    "deviceParameters",
                    serde_json::to_value(parameters).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "device.parameter.get" => {
                let params: DeviceParameterGetParams = decode(params)?;
                let _ = canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let value = self
                    .core
                    .audio()
                    .list_track_device_parameters(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let parameters = value
                    .get("parameters")
                    .and_then(Value::as_array)
                    .ok_or_else(|| command_error("Native plugin response omitted parameters"))?;
                let parameter = parameters
                    .iter()
                    .find(|parameter| {
                        parameter
                            .get("index")
                            .and_then(Value::as_u64)
                            .and_then(|index| u32::try_from(index).ok())
                            == Some(params.parameter_index)
                    })
                    .cloned()
                    .ok_or_else(|| {
                        command_error(format!(
                            "plugin parameter is not registered: {}",
                            params.parameter_index
                        ))
                    })?;
                let parameter: crate::model::DeviceParameterInfo =
                    serde_json::from_value(parameter).map_err(serialize_error)?;
                Ok((
                    "deviceParameter",
                    serde_json::to_value(parameter).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "plugin.state.get" => {
                let params: PluginDeviceParams = decode(params)?;
                let (plugin_path, _) =
                    canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let value = self
                    .core
                    .audio()
                    .get_track_plugin_state(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let state = native_plugin_state_value(&value)?;
                let snapshot = plugin_state_snapshot(&plugin_path, state)?;
                Ok((
                    "pluginState",
                    serde_json::to_value(snapshot).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "plugin.preset.list" => {
                let params: PluginDeviceParams = decode(params)?;
                let _ = canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let value = self
                    .core
                    .audio()
                    .list_track_plugin_programs(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let presets = plugin_presets(&value)?;
                if presets.is_empty() {
                    return Err(command_error(
                        "plugin does not expose host-visible programs",
                    ));
                }
                Ok((
                    "pluginPresets",
                    serde_json::to_value(presets).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "plugin.preset.get" => {
                let params: PluginDeviceParams = decode(params)?;
                let _ = canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let value = self
                    .core
                    .audio()
                    .list_track_plugin_programs(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let presets = plugin_presets(&value)?;
                let current_index = native_current_program(&value).ok_or_else(|| {
                    command_error("plugin does not expose a current host-visible program")
                })?;
                let preset = presets
                    .into_iter()
                    .find(|preset| preset.index == current_index)
                    .ok_or_else(|| command_error("plugin current program is not registered"))?;
                Ok((
                    "pluginPreset",
                    serde_json::to_value(preset).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "plugin.state.set" => {
                let params: PluginStateSetParams = decode(params)?;
                let (plugin_path, bypassed) =
                    canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                validate_plugin_state(&params.state, &plugin_path)?;
                let previous = self
                    .core
                    .audio()
                    .get_track_plugin_state(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let previous_state = native_plugin_state_value(&previous)?;
                let native_state = plugin_state_value(&params.state, bypassed)?;
                let context =
                    self.session_context_with_project_commit(expected_project_id.clone())?;
                self.core
                    .audio()
                    .set_track_plugin_state(&params.track_id, &params.device_id, native_state)
                    .map_err(audio_error)?;
                let commit = session_adapter::commit_core_application(&context, |core, store| {
                    core.application(store).persist_track_plugin_state(
                        &params.track_id,
                        &params.device_id,
                        params.state.parameter_values.clone(),
                        params.state.state_data.clone(),
                        bypassed,
                    )
                });
                if let Err(error) = commit {
                    let _ = self.core.audio().set_track_plugin_state(
                        &params.track_id,
                        &params.device_id,
                        previous_state,
                    );
                    return Err(error.protocol_error());
                }
                let mutation = session_adapter::arrangement_mutation_without_projection(&context)
                    .map_err(|error| error.protocol_error())?;
                let sequence = mutation.canonical.sequence;
                Ok((
                    "arrangementMutation",
                    serde_json::to_value(mutation).map_err(serialize_error)?,
                    sequence,
                ))
            }
            "plugin.preset.set" => {
                let params: PluginPresetSetParams = decode(params)?;
                if params.preset.is_some() == params.preset_index.is_some() {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidRequest,
                        "exactly one of preset or presetIndex is required",
                    ));
                }
                let (plugin_path, bypassed) =
                    canonical_plugin_device(&current, &params.track_id, &params.device_id)?;
                let programs = self
                    .core
                    .audio()
                    .list_track_plugin_programs(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let presets = plugin_presets(&programs)?;
                let program_index =
                    resolve_plugin_preset(&presets, params.preset.as_deref(), params.preset_index)?;
                let previous_program = native_current_program(&programs);
                let previous = self
                    .core
                    .audio()
                    .get_track_plugin_state(&params.track_id, &params.device_id)
                    .map_err(audio_error)?;
                let previous_state = native_plugin_state_value(&previous)?;
                let context =
                    self.session_context_with_project_commit(expected_project_id.clone())?;
                let rollback = || {
                    if let Some(previous_program) = previous_program {
                        let _ = self.core.audio().set_track_plugin_program(
                            &params.track_id,
                            &params.device_id,
                            previous_program,
                        );
                    }
                    let _ = self.core.audio().set_track_plugin_state(
                        &params.track_id,
                        &params.device_id,
                        previous_state.clone(),
                    );
                };
                let changed = self
                    .core
                    .audio()
                    .set_track_plugin_program(&params.track_id, &params.device_id, program_index)
                    .map_err(audio_error)?;
                let state = match native_plugin_state_value(&changed) {
                    Ok(state) => state,
                    Err(error) => {
                        rollback();
                        return Err(error);
                    }
                };
                let state_snapshot = match plugin_state_snapshot(&plugin_path, state) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        rollback();
                        return Err(error);
                    }
                };
                let commit = session_adapter::commit_core_application(&context, |core, store| {
                    core.application(store).persist_track_plugin_state(
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
                let mutation = session_adapter::arrangement_mutation_without_projection(&context)
                    .map_err(|error| error.protocol_error())?;
                let sequence = mutation.canonical.sequence;
                Ok((
                    "arrangementMutation",
                    serde_json::to_value(mutation).map_err(serialize_error)?,
                    sequence,
                ))
            }
            "host.status" => Ok((
                "hostStatus",
                serde_json::json!({
                    "instanceId": self.identity().instance_id.clone(),
                    "pid": self.identity().pid,
                    "safeMode": self.core.safe_mode(),
                    "dataRoot": self.data_root.to_string_lossy(),
                    "runtimeGeneration": self.core.audio().runtime_generation(),
                }),
                current.sequence,
            )),
            "host.info" => Ok((
                "hostInfo",
                serde_json::json!({
                    "instanceId": self.identity().instance_id.clone(),
                    "pid": self.identity().pid,
                    "dataRoot": self.data_root.to_string_lossy(),
                    "projectName": current.session.project_name,
                    "safeMode": self.core.safe_mode(),
                    "runtimeState": serde_json::to_value(
                        self.core.audio().status().map_err(audio_error)?.state,
                    )
                    .map_err(serialize_error)?,
                }),
                current.sequence,
            )),
            "host.bootstrap" => Ok((
                "hostBootstrap",
                serde_json::to_value(
                    self.bootstrap()
                        .map_err(|error| command_error(error.to_string()))?,
                )
                .map_err(serialize_error)?,
                current.sequence,
            )),
            "instrument.builtin.list" => Ok((
                "builtInInstruments",
                serde_json::to_value(self.built_in_instruments.summaries())
                    .map_err(serialize_error)?,
                current.sequence,
            )),
            "host.shutdown" => {
                self.shutdown_requested.store(true, Ordering::Release);
                self.shutting_down.store(true, Ordering::Release);
                Ok(("ok", Value::Null, current.sequence))
            }
            "audio.master-gain.preview" => {
                let params: MasterGainParams = decode(params)?;
                if !params.gain_db.is_finite() {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidRequest,
                        "master gain must be finite",
                    ));
                }
                self.core
                    .audio()
                    .preview_master_gain_db(params.gain_db)
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "audio.emergency-mute" => {
                let params: MuteParams = decode(params)?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(
                        self.core
                            .audio()
                            .set_emergency_mute_from_user(params.muted)
                            .map_err(audio_error)?,
                    )
                    .map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "audio.feedback-protection.reset" => Ok((
                "audioStatus",
                serde_json::to_value(
                    self.core
                        .audio()
                        .reset_feedback_protection()
                        .map_err(audio_error)?,
                )
                .map_err(serialize_error)?,
                current.sequence,
            )),
            "midi.listening.enable" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode blocks MIDI input; offline MIDI remains available",
                    ));
                }
                Ok((
                    "audioStatus",
                    serde_json::to_value(
                        self.core
                            .audio()
                            .enable_midi_listening()
                            .map_err(audio_error)?,
                    )
                    .map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "midi.listening.disable" => Ok((
                "audioStatus",
                serde_json::to_value(
                    self.core
                        .audio()
                        .disable_midi_listening()
                        .map_err(audio_error)?,
                )
                .map_err(serialize_error)?,
                current.sequence,
            )),
            "plugin.editor.open" => {
                let params: PluginEditorParams = decode(params)?;
                let context = self.session_context()?;
                session_adapter::open_track_plugin_editor(
                    &context,
                    &params.track_id,
                    &params.device_id,
                )
                .map_err(|error| error.protocol_error())?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "take.comparison.start" => {
                let params: TakeIdParams = decode(params)?;
                let context = self.session_context()?;
                let status = session_adapter::start_take_comparison(&context, &params.take_id)
                    .map_err(|error| error.protocol_error())?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "take.comparison.switch" => {
                let params: TakeComparisonParams = decode(params)?;
                let context = self.session_context()?;
                let status =
                    session_adapter::switch_take_comparison_variant(&context, params.variant)
                        .map_err(|error| error.protocol_error())?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "take.comparison.stop" => {
                let context = self.session_context()?;
                let status = session_adapter::stop_take_comparison(&context)
                    .map_err(|error| error.protocol_error())?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "runtime.projection.get" => Ok((
                "runtimeProjection",
                serde_json::to_value(self.runtime.status()).map_err(serialize_error)?,
                current.sequence,
            )),
            "runtime.projection.retry" => {
                let target = self
                    .canonical()
                    .map_err(|error| command_error(error.to_string()))?;
                self.runtime
                    .apply_and_wait(
                        runtime_timeline_snapshot(
                            &self.data_root,
                            self.built_in_instruments.as_ref(),
                            &target.session,
                        ),
                        riffra_core::ProjectionKey {
                            sequence: target.sequence,
                            session_revision: target.session.arrangement.revision,
                        },
                        Duration::from_secs(60),
                    )
                    .map_err(runtime_error)?;
                Ok((
                    "runtimeProjection",
                    serde_json::to_value(self.runtime.status()).map_err(serialize_error)?,
                    target.sequence,
                ))
            }
            "transport.play" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                self.runtime
                    .request_play_when_ready(riffra_core::ProjectionKey {
                        sequence: current.sequence,
                        session_revision: current.session.arrangement.revision,
                    })
                    .map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.stop" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                self.runtime.stop().map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.go-to-start" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                self.runtime
                    .stop_and_seek_to_start(|| {
                        self.core
                            .audio()
                            .seek_timeline(0)
                            .map_err(RuntimeError::from)
                    })
                    .map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.seek" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                let params: SeekParams = decode(params)?;
                self.core
                    .audio()
                    .seek_timeline(params.tick)
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "audio.status" => Ok((
                "audioStatus",
                serde_json::to_value(self.core.audio().status().map_err(audio_error)?)
                    .map_err(serialize_error)?,
                current.sequence,
            )),
            "audio.probe" => Ok((
                "audioProbe",
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps audio device probing offline",
                    ));
                } else {
                    serde_json::to_value(
                        self.core
                            .audio()
                            .probe_devices(std::time::Duration::from_secs(10))
                            .map_err(command_error)?,
                    )
                    .map_err(serialize_error)?
                },
                current.sequence,
            )),
            "audio.channels.probe" => {
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps audio channel probing offline",
                    ));
                }
                let params: AudioChannelsProbeParams = decode(params)?;
                let channels = self
                    .core
                    .audio()
                    .probe_device_channels(
                        &params.driver,
                        &params.input_device,
                        &params.output_device,
                        std::time::Duration::from_secs(10),
                    )
                    .map_err(command_error)?;
                Ok((
                    "deviceChannels",
                    serde_json::to_value(channels).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "audio.recover" => {
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps external audio devices isolated",
                    ));
                }
                let status = self
                    .recover_audio_device()
                    .map_err(|error| command_error(error.to_string()))?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "audio.startup.retry" => {
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps external audio devices isolated",
                    ));
                }
                let status = self
                    .retry_runtime_startup()
                    .map_err(|error| command_error(error.to_string()))?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "audio.driver.get" => Ok((
                "audioDriver",
                serde_json::to_value(
                    self.audio_preferences
                        .lock()
                        .map_err(|_| command_error("audio preferences lock was poisoned"))?
                        .clone(),
                )
                .map_err(serialize_error)?,
                current.sequence,
            )),
            "audio.driver.set" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps external audio devices isolated",
                    ));
                }
                let config: AudioDriverConfig = decode(params)?;
                let status = self.set_audio_driver(config)?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "asset.preview" => {
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode blocks live sample preview",
                    ));
                }
                let params: AssetPreviewParams = decode(params)?;
                let asset_id =
                    riffra_core::AssetId::from_normalized(&params.asset_id).map_err(|error| {
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
                    })?;
                let status = crate::asset::application::preview_asset(
                    &AssetPreviewContext {
                        audio: self.core.audio(),
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
                .map_err(command_error)?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "asset.preview.stop" => Ok((
                "audioStatus",
                serde_json::to_value(self.core.audio().stop_preview().map_err(audio_error)?)
                    .map_err(serialize_error)?,
                current.sequence,
            )),
            "midi.send" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable("Safe Mode keeps MIDI output offline"));
                }
                let params: MidiSendParams = decode(params)?;
                self.core
                    .audio()
                    .send_track_midi(&params.track_id, &params.bytes)
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "midi.target.set" => {
                let params: LiveMidiTargetParams = decode(params)?;
                self.core
                    .audio()
                    .set_live_midi_target(params.track_id.as_deref())
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "midi.panic" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable("Safe Mode keeps MIDI output offline"));
                }
                let params: TrackIdParams = decode(params)?;
                self.core
                    .audio()
                    .panic_track_midi(&params.track_id)
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "plugin.catalog.list" => {
                let catalog = plugins::load(&self.data_root).map_err(|error| {
                    command_error(format!("plugin catalog could not be loaded: {error}"))
                })?;
                Ok((
                    "plugins",
                    serde_json::to_value(catalog).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "plugin.scan" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode blocks VST3 discovery and load validation",
                    ));
                }
                let params: PluginScanParams = decode(params)?;
                let root = params
                    .path
                    .map(PathBuf::from)
                    .unwrap_or_else(default_plugin_root);
                let report = self
                    .scan_plugins(root)
                    .map_err(|error| command_error(format!("plugin scan failed: {error}")))?;
                Ok((
                    "pluginScan",
                    serde_json::to_value(report).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "plugin.scan.start" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode blocks VST3 discovery and load validation",
                    ));
                }
                let params: PluginScanParams = decode(params)?;
                let root = params
                    .path
                    .map(PathBuf::from)
                    .unwrap_or_else(default_plugin_root);
                let status = self.start_plugin_scan(root).map_err(|error| {
                    command_error(format!("plugin scan could not start: {error}"))
                })?;
                Ok((
                    "job",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "missing.list" => {
                let missing = missing::collect_missing(&self.data_root, &current.session);
                Ok((
                    "missing",
                    serde_json::to_value(missing).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "record.start" | "record.stop" | "record.status" | "record.list" | "record.rename"
            | "record.archive" | "record.promote" | "record.tag" | "record.delete"
            | "record.duplicates" => {
                let _recording = self
                    .recording_gate
                    .lock()
                    .map_err(|_| command_error("recording operation lock was poisoned"))?;
                let context = RecordingContext {
                    core: Arc::clone(&self.core),
                    audio: self.core.audio().clone(),
                    runtime: Arc::clone(&self.runtime),
                    storage: self
                        .project_store
                        .active_session_store()
                        .map_err(|error| command_error(error.to_string()))?,
                    data_root: self.data_root.clone(),
                    built_in_instruments: Arc::clone(&self.built_in_instruments),
                    events: Arc::clone(&self.events),
                    jobs: self.jobs.clone(),
                    safe_mode: self.core.safe_mode(),
                };
                let mut sequence = current.sequence;
                let value = match command {
                    "record.start" => {
                        if self.core.safe_mode() {
                            return Err(runtime_unavailable(
                                "Safe Mode keeps recording input offline",
                            ));
                        }
                        let params: RecordStartParams = decode(params)?;
                        let status = match params.recording_session_id.as_deref() {
                            Some(id) => recording::record_another_take(&context, id),
                            None => recording::start_recording(&context),
                        }
                        .map_err(command_error)?;
                        serde_json::to_value(status).map_err(serialize_error)?
                    }
                    "record.stop" => {
                        let result = recording::stop_recording(&context).map_err(command_error)?;
                        sequence = result.canonical.sequence;
                        if sequence > current.sequence {
                            self.events
                                .emit(HostEvent::CanonicalStateChanged(result.canonical.clone()));
                        }
                        serde_json::to_value(result).map_err(serialize_error)?
                    }
                    "record.status" => serde_json::to_value(
                        context
                            .audio
                            .refresh_status()
                            .map_err(|error| error.to_string())
                            .map_err(command_error)?,
                    )
                    .map_err(serialize_error)?,
                    "record.list" => {
                        let params: RecordListParams = decode(params)?;
                        serde_json::to_value(
                            recording::list_recordings(&context, params.query.as_deref())
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.rename" => {
                        let params: RecordRenameParams = decode(params)?;
                        serde_json::to_value(
                            recording::rename_recording(&context, &params.id, &params.new_name)
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.archive" => {
                        let params: RecordIdParams = decode(params)?;
                        serde_json::to_value(
                            recording::archive_recording(&context, &params.id)
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.promote" => {
                        let params: RecordIdParams = decode(params)?;
                        serde_json::to_value(
                            recording::promote_recording(&context, &params.id)
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.tag" => {
                        let params: RecordTagParams = decode(params)?;
                        serde_json::to_value(
                            recording::tag_recording(&context, &params.id, params.tag, params.note)
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.delete" => {
                        let params: RecordIdParams = decode(params)?;
                        recording::delete_recording(&context, &params.id).map_err(command_error)?;
                        Value::Null
                    }
                    "record.duplicates" => serde_json::to_value(
                        recording::detect_duplicate_recordings(&context).map_err(command_error)?,
                    )
                    .map_err(serialize_error)?,
                    _ => unreachable!(),
                };
                Ok(("recording", value, sequence))
            }
            "render.start" => {
                let params: RenderStartParams = decode(params)?;
                let options = params.options.unwrap_or_default();
                let session = current.session.clone();
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
                            Ok(value) => {
                                worker_jobs.complete(&job_id, value, "Offline render completed.")
                            }
                            Err(error) => {
                                jobs::fail(&worker_jobs, &data_root, &job_id, error.to_string())
                            }
                        },
                        Err(error) => jobs::fail(&worker_jobs, &data_root, &job_id, error),
                    }
                })
                .map_err(|error| command_error(format!("render job could not start: {error}")))?;
                Ok((
                    "job",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "job.get" | "job.cancel" => {
                let params: JobIdParams = decode(params)?;
                let status = if command == "job.cancel" {
                    self.jobs.cancel(&params.id)
                } else {
                    self.jobs.status(&params.id)
                };
                let status = status
                    .map(jobs::to_background_status)
                    .transpose()
                    .map_err(command_error)?;
                Ok((
                    "job",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "library.search" => {
                let params: LibrarySearchParams = decode(params)?;
                let result =
                    library::search(&self.data_root, &params.query).map_err(command_error)?;
                Ok((
                    "library",
                    serde_json::to_value(result).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "library.asset.update" => {
                let params: LibraryUpdateParams = decode(params)?;
                let result =
                    library::update_metadata(&self.data_root, &params.id, params.tag, params.note)
                        .map_err(command_error)?;
                Ok((
                    "libraryAsset",
                    serde_json::to_value(result).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "library.related" => {
                let params: LibraryIdParams = decode(params)?;
                let result =
                    library::related(&self.data_root, &params.id).map_err(command_error)?;
                Ok((
                    "library",
                    serde_json::to_value(result).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "analysis.start" => {
                let params: AnalysisParams = decode(params)?;
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
                let result = analysis::analyze(&path).map_err(command_error)?;
                Ok((
                    "analysis",
                    serde_json::to_value(result).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            _ => Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                format!("unknown command: {command}"),
            )),
        }
    }

    fn dispatch_shared_session(
        &self,
        command: &str,
        params: Value,
        current_sequence: u64,
        project_commit: Option<String>,
    ) -> Result<Option<(&'static str, Value, u64)>, ProtocolError> {
        let context = self.session_context_with_project_commit(project_commit)?;
        let result = match command {
            "track.audio-input.set" => {
                let params: AudioInputParams = decode(params)?;
                Some(
                    session_adapter::set_track_audio_input(
                        &context,
                        &params.track_id,
                        Some(params.channel_index),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "track.audio-input.clear" => {
                let params: SessionTrackIdParams = decode(params)?;
                Some(
                    session_adapter::set_track_audio_input(&context, &params.track_id, None)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "track.midi-input.set" => {
                let params: MidiInputParams = decode(params)?;
                Some(
                    session_adapter::set_track_midi_input(
                        &context,
                        &params.track_id,
                        riffra_core::MidiInputRoute {
                            device_id: params.device_id,
                            channel: params.channel,
                        },
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "track.midi-input.clear" => {
                let params: SessionTrackIdParams = decode(params)?;
                Some(
                    session_adapter::set_track_midi_input(
                        &context,
                        &params.track_id,
                        riffra_core::MidiInputRoute::default(),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "instrument.builtin.set" => {
                let params: BuiltInInstrumentParams = decode(params)?;
                Some(
                    session_adapter::set_track_builtin_instrument_with_expected_sequence(
                        &context,
                        &params.track_id,
                        &params.preset_id,
                        Some(current_sequence),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "instrument.vst3.set" => {
                let params: PluginPathParams = decode(params)?;
                Some(
                    session_adapter::set_track_vst3_instrument_with_expected_sequence(
                        &context,
                        &params.track_id,
                        &params.plugin_path,
                        Some(current_sequence),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "instrument.clear" => {
                let params: SessionTrackIdParams = decode(params)?;
                Some(
                    session_adapter::clear_track_instrument(&context, &params.track_id)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "effect.add" => {
                let params: PluginPathParams = decode(params)?;
                Some(
                    session_adapter::add_track_effect_with_expected_sequence(
                        &context,
                        &params.track_id,
                        &params.plugin_path,
                        Some(current_sequence),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "effect.remove" => {
                let params: EffectRemoveParams = decode(params)?;
                Some(
                    session_adapter::remove_track_effect(
                        &context,
                        &params.track_id,
                        &params.device_id,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "effect.reorder" => {
                let params: EffectReorderParams = decode(params)?;
                Some(
                    session_adapter::reorder_track_effects(
                        &context,
                        &params.track_id,
                        &params.device_ids,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "device.bypass" => {
                let params: DeviceBypassParams = decode(params)?;
                Some(
                    session_adapter::set_track_device_bypassed(
                        &context,
                        &params.track_id,
                        &params.device_id,
                        params.bypassed,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "device.parameter.set" => {
                let params: DeviceParameterParams = decode(params)?;
                Some(
                    session_adapter::set_track_device_parameter(
                        &context,
                        &params.track_id,
                        &params.device_id,
                        params.parameter_index,
                        params.value,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "missing.relink" => {
                let params: MissingRelinkParams = decode(params)?;
                let asset_id =
                    riffra_core::AssetId::from_normalized(&params.asset_id).map_err(|error| {
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
                    })?;
                Some(
                    session_adapter::relink_missing_dependency(
                        &context,
                        asset_id,
                        &params.new_path,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "missing.disable-plugin" => {
                let params: DeviceIdParams = decode(params)?;
                Some(
                    session_adapter::disable_missing_plugin(&context, &params.device_id)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "missing.replace-plugin" => {
                let params: MissingPluginReplaceParams = decode(params)?;
                Some(
                    session_adapter::replace_missing_track_plugin_with_expected_sequence(
                        &context,
                        &params.device_id,
                        &params.new_path,
                        Some(current_sequence),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "undo" => {
                Some(session_adapter::undo(&context).map_err(|error| error.protocol_error())?)
            }
            "redo" => {
                Some(session_adapter::redo(&context).map_err(|error| error.protocol_error())?)
            }
            "project.restore-generation" => {
                let params: ProjectRestoreParams = decode(params)?;
                Some(
                    session_adapter::restore_generation(&context, &params.file_name)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "plugin.state.persist" => {
                let params: PluginStatePersistParams = decode(params)?;
                Some(
                    session_adapter::persist_track_plugin_state(
                        &context,
                        &params.track_id,
                        &params.device_id,
                        params.parameter_values,
                        params.state_data,
                        params.bypassed,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "plugin.parameter.persist" => {
                let params: PluginParameterPersistParams = decode(params)?;
                Some(
                    session_adapter::persist_track_plugin_parameter(
                        &context,
                        &params.track_id,
                        &params.device_id,
                        params.parameter_index,
                        params.value,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "audio-clip.take-variant.set" => {
                let params: TakeVariantParams = decode(params)?;
                Some(
                    session_adapter::set_audio_clip_take_variant(
                        &context,
                        &params.clip_id,
                        params.variant,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "take.activate" => {
                let params: TakeActivateParams = decode(params)?;
                Some(
                    session_adapter::activate_take(&context, &params.session_id, &params.take_id)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "take.place-separate-clip" => {
                let params: TakeIdParams = decode(params)?;
                Some(
                    session_adapter::place_take_as_separate_clip(&context, &params.take_id)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            _ => None,
        };
        Ok(result.map(|value| {
            let sequence = value.canonical.sequence;
            (
                "arrangementMutation",
                serde_json::to_value(value).expect("runtime mutation results serialize"),
                sequence,
            )
        }))
    }

    pub(super) fn after_canonical_commit(
        &self,
        effect: CanonicalMutationEffect,
    ) -> Result<crate::model::ArrangementMutationResult, ProtocolError> {
        let canonical = self
            .canonical()
            .map_err(|error| command_error(error.to_string()))?;
        let storage = self
            .project_store
            .active_session_store()
            .map_err(|error| command_error(error.to_string()))?;
        library::index::refresh(&self.data_root, &storage, &canonical.session);
        self.events
            .emit(HostEvent::CanonicalStateChanged(canonical.clone()));
        let mutation = commit::finalize_arrangement_mutation(
            canonical,
            self.runtime.as_ref(),
            &self.data_root,
            self.built_in_instruments.as_ref(),
            self.core.safe_mode(),
            effect,
        )
        .map_err(command_error)?;
        Ok(mutation)
    }
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ProtocolError> {
    serde_json::from_value(value).map_err(|error| {
        ProtocolError::new(
            ErrorCode::InvalidRequest,
            format!("invalid command parameters: {error}"),
        )
    })
}

pub(super) fn command_error(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::CommandFailed, message)
}

fn runtime_unavailable(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
}

fn runtime_error(error: RuntimeError) -> ProtocolError {
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

fn audio_error(error: crate::NativeAudioError) -> ProtocolError {
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
        .rack
        .devices
        .iter()
        .find(|device| device.id == device_id)
        .ok_or_else(|| command_error(format!("track device is not registered: {device_id}")))?;
    if device.kind != riffra_core::DeviceKind::Plugin {
        return Err(command_error(
            "non-plugin devices do not expose VST3 plugin controls",
        ));
    }
    let path = device
        .path
        .clone()
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| command_error("plugin device has no VST3 path"))?;
    Ok((path, device.bypassed))
}

fn canonical_non_plugin_device_inspection(
    canonical: &CanonicalState,
    track_id: &str,
    device_id: &str,
) -> Result<Option<crate::model::DeviceInspection>, ProtocolError> {
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
        return Ok(Some(crate::model::DeviceInspection {
            id: instrument.id.clone(),
            name: instrument.name.clone(),
            source: "builtin".into(),
            bypassed: instrument.bypassed,
            capabilities: crate::model::DeviceCapabilities::default(),
            parameter_count: 0,
            state_persisted: false,
        }));
    }
    let device = track
        .rack
        .devices
        .iter()
        .find(|device| device.id == device_id)
        .ok_or_else(|| command_error(format!("track device is not registered: {device_id}")))?;
    if device.kind == riffra_core::DeviceKind::Plugin {
        return Ok(None);
    }
    Ok(Some(crate::model::DeviceInspection {
        id: device.id.clone(),
        name: device.name.clone(),
        source: "rack".into(),
        bypassed: device.bypassed,
        capabilities: crate::model::DeviceCapabilities::default(),
        parameter_count: 0,
        state_persisted: false,
    }))
}

fn native_plugin_state_value(value: &Value) -> Result<Value, ProtocolError> {
    let state = value.get("state").cloned().unwrap_or_else(|| value.clone());
    if !state.is_object() {
        return Err(command_error("Native plugin response omitted state"));
    }
    Ok(state)
}

fn plugin_state_snapshot(
    plugin_path: &str,
    state: Value,
) -> Result<crate::model::PluginStateSnapshot, ProtocolError> {
    let object = state
        .as_object()
        .ok_or_else(|| command_error("Native plugin state was not an object"))?;
    let parameter_values = object
        .get("parameterValues")
        .cloned()
        .ok_or_else(|| command_error("Native plugin state omitted parameterValues"))?;
    let state_data = object.get("stateData").cloned().unwrap_or(Value::Null);
    let snapshot: crate::model::PluginStateSnapshot = serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "pluginPath": plugin_path,
        "parameterValues": parameter_values,
        "stateData": state_data,
    }))
    .map_err(serialize_error)?;
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

fn plugin_state_value(
    state: &crate::model::PluginStateSnapshot,
    bypassed: bool,
) -> Result<Value, ProtocolError> {
    let mut value = serde_json::to_value(state).map_err(serialize_error)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| command_error("plugin state did not form an object"))?;
    object.remove("schemaVersion");
    object.remove("pluginPath");
    object.insert("bypassed".into(), Value::Bool(bypassed));
    Ok(value)
}

fn validate_plugin_state(
    state: &crate::model::PluginStateSnapshot,
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

fn plugin_presets(value: &Value) -> Result<Vec<crate::model::PluginPresetInfo>, ProtocolError> {
    let programs = value
        .get("programs")
        .cloned()
        .ok_or_else(|| command_error("Native plugin response omitted programs"))?;
    serde_json::from_value(programs).map_err(serialize_error)
}

fn native_current_program(value: &Value) -> Option<u32> {
    value
        .get("currentIndex")
        .and_then(Value::as_i64)
        .filter(|index| *index >= 0)
        .and_then(|index| u32::try_from(index).ok())
}

fn resolve_plugin_preset(
    presets: &[crate::model::PluginPresetInfo],
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

fn serialize_error(error: serde_json::Error) -> ProtocolError {
    command_error(error.to_string())
}

fn requires_command_gate(command: &str) -> bool {
    crate::dispatcher::command_requires_project_id(command) && !is_long_project_operation(command)
}

fn is_long_project_operation(command: &str) -> bool {
    matches!(
        command,
        "instrument.builtin.set" | "instrument.vst3.set" | "effect.add" | "missing.replace-plugin"
    )
}

fn is_host_runtime_command(command: &str) -> bool {
    matches!(
        command,
        "host.status"
            | "host.info"
            | "host.bootstrap"
            | "host.shutdown"
            | "instrument.builtin.list"
            | "audio.master-gain.preview"
            | "audio.emergency-mute"
            | "audio.feedback-protection.reset"
            | "midi.listening.enable"
            | "midi.listening.disable"
            | "runtime.projection.get"
            | "runtime.projection.retry"
            | "transport.play"
            | "transport.stop"
            | "transport.go-to-start"
            | "transport.seek"
            | "audio.status"
            | "audio.probe"
            | "audio.channels.probe"
            | "audio.recover"
            | "audio.startup.retry"
            | "audio.driver.set"
            | "audio.driver.get"
            | "asset.preview"
            | "asset.preview.stop"
            | "midi.send"
            | "midi.target.set"
            | "midi.panic"
            | "plugin.catalog.list"
            | "plugin.scan"
            | "plugin.scan.start"
            | "missing.list"
            | "record.start"
            | "record.stop"
            | "record.status"
            | "record.list"
            | "record.rename"
            | "record.archive"
            | "record.promote"
            | "record.tag"
            | "record.delete"
            | "record.duplicates"
            | "render.start"
            | "job.get"
            | "job.cancel"
            | "library.search"
            | "library.asset.update"
            | "library.related"
            | "analysis.start"
            | "plugin.editor.open"
            | "device.inspect"
            | "device.parameter.list"
            | "device.parameter.get"
            | "plugin.preset.list"
            | "plugin.preset.get"
            | "plugin.preset.set"
            | "plugin.state.get"
            | "plugin.state.set"
            | "take.comparison.start"
            | "take.comparison.switch"
            | "take.comparison.stop"
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackIdParams {
    track_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuiltInInstrumentParams {
    track_id: String,
    preset_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeekParams {
    tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MasterGainParams {
    gain_db: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MuteParams {
    muted: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginEditorParams {
    track_id: String,
    device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginStatePersistParams {
    track_id: String,
    device_id: String,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginParameterPersistParams {
    track_id: String,
    device_id: String,
    parameter_index: i32,
    value: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRestoreParams {
    file_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeIdParams {
    take_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeActivateParams {
    session_id: String,
    take_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeVariantParams {
    clip_id: String,
    variant: riffra_core::AudioTakeVariant,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeComparisonParams {
    variant: riffra_core::AudioTakeVariant,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiSendParams {
    track_id: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveMidiTargetParams {
    track_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginScanParams {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioChannelsProbeParams {
    driver: String,
    input_device: String,
    output_device: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetPreviewParams {
    asset_id: String,
    #[serde(default)]
    start_ms: u64,
    #[serde(default)]
    end_ms: Option<u64>,
    #[serde(default)]
    looped: bool,
    #[serde(default = "default_preview_gain")]
    gain: f32,
}

fn default_preview_gain() -> f32 {
    1.0
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordStartParams {
    recording_session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordListParams {
    query: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordIdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordRenameParams {
    id: String,
    new_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordTagParams {
    id: String,
    tag: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderStartParams {
    options: Option<RenderOptions>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobIdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibrarySearchParams {
    query: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryUpdateParams {
    id: String,
    tag: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisParams {
    asset_id: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionTrackIdParams {
    track_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::{
        ControlCommand, HelloRequest, HelloResponse, LocalHostClient, LocalHostRegistry,
        endpoint_path, new_instance_id, read_endpoint, transport,
    };

    #[test]
    fn project_bound_runtime_commands_take_the_command_gate() {
        for command in ["transport.play", "record.start", "render.start"] {
            assert!(super::requires_command_gate(command), "{command}");
        }
        for command in [
            "host.status",
            "project.list",
            "instrument.vst3.set",
            "effect.add",
        ] {
            assert!(!super::requires_command_gate(command), "{command}");
        }
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
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();

        let response = host.dispatch_control(ControlRequest::new(
            "missing-project-id",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Rejected", "kind": "instrument"}),
            ),
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
                &ControlRequest::new(
                    "session-get",
                    ControlCommand::new("session.get", serde_json::json!({})),
                    Some(0),
                )
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
                ControlCommand::new(
                    "track.add",
                    serde_json::json!({"name": "Synth", "kind": "instrument"}),
                ),
                Some(0),
            )
            .with_expected_project_id(expected_project_id);
            transport::write_frame(&mut stream, &request).unwrap();
            let response: ControlResponse = transport::read_frame(&mut stream).unwrap();
            assert!(response.ok);
            assert_eq!(response.sequence, Some(1));
        }

        assert_eq!(
            host.runtime_status().unwrap().state,
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
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let expected_project_id = host.bootstrap().unwrap().project_state.active_project_id;

        let mutation = host.dispatch_control(
            ControlRequest::new(
                "track-add",
                ControlCommand::new(
                    "track.add",
                    serde_json::json!({"name": "Synth", "kind": "instrument"}),
                ),
                Some(0),
            )
            .with_expected_project_id(expected_project_id.clone()),
        );
        assert!(mutation.ok);
        assert_eq!(mutation.sequence, Some(1));

        let undo = host.dispatch_control(
            ControlRequest::new(
                "stale-undo",
                ControlCommand::new("undo", serde_json::json!({})),
                Some(0),
            )
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
                ControlCommand::new("render.start", serde_json::json!({})),
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
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let client = LocalHostClient::connect_data_root(&data_root).unwrap();

        let response = client
            .request(&ControlRequest::new(
                "info",
                ControlCommand::new("host.info", serde_json::json!({})),
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
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let client = LocalHostClient::connect_data_root(&data_root).unwrap();
        let mut events = client.open_event_stream().unwrap();

        let bootstrap = client
            .request(&ControlRequest::new(
                "bootstrap",
                ControlCommand::new("host.bootstrap", serde_json::json!({})),
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
                    ControlCommand::new(
                        "track.add",
                        serde_json::json!({"name": "Synth", "kind": "instrument"}),
                    ),
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
        let mutation_result: crate::model::ArrangementMutationResult =
            serde_json::from_value(mutation.result.unwrap().value).unwrap();
        assert_eq!(mutation_result.canonical.sequence, 1);
        assert!(matches!(
            mutation_result.projection,
            crate::model::ArrangementProjectionOutcome::NotRequired
        ));
        let event = events.recv().unwrap();
        assert_eq!(event.event, "canonical-state-changed");
        assert_eq!(event.payload["sequence"], 1);

        let discovered = LocalHostRegistry::current_user()
            .discover()
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
                ControlCommand::new("host.shutdown", serde_json::json!({})),
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
                ControlCommand::new(
                    "track.add",
                    serde_json::json!({"name": "Rejected", "kind": "audio"}),
                ),
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
            ),
        };
        let host = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();
        let expected_project_id = host.bootstrap().unwrap().project_state.active_project_id;

        let response = host.dispatch_control(
            ControlRequest::new(
                "track-add",
                ControlCommand::new(
                    "track.add",
                    serde_json::json!({"name": "Synth", "kind": "instrument"}),
                ),
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
        let mutation: crate::model::ArrangementMutationResult =
            serde_json::from_value(response.result.unwrap().value).unwrap();
        assert_eq!(mutation.canonical.sequence, 1);
        assert!(matches!(
            mutation.projection,
            crate::model::ArrangementProjectionOutcome::Queued
                | crate::model::ArrangementProjectionOutcome::Failed { .. }
        ));

        let marker = host.dispatch_control(
            ControlRequest::new(
                "marker-add",
                ControlCommand::new(
                    "marker.add",
                    serde_json::json!({"name": "Verse", "tick": 0}),
                ),
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
        let marker: crate::model::ArrangementMutationResult =
            serde_json::from_value(marker.result.unwrap().value).unwrap();
        assert_eq!(marker.canonical.sequence, 2);
        assert!(matches!(
            marker.projection,
            crate::model::ArrangementProjectionOutcome::NotRequired
        ));

        let settings = host.dispatch_control(
            ControlRequest::new(
                "session-settings-update",
                ControlCommand::new(
                    "session.settings.update",
                    serde_json::json!({"note": "authoring note"}),
                ),
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
        let settings: crate::model::ArrangementMutationResult =
            serde_json::from_value(settings.result.unwrap().value).unwrap();
        assert_eq!(settings.canonical.sequence, 3);
        assert!(matches!(
            settings.projection,
            crate::model::ArrangementProjectionOutcome::NotRequired
        ));

        host.shutdown();
        drop(host);

        let reopened = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        reopened.shutdown();
        drop(reopened);
        let _ = std::fs::remove_dir_all(data_root);
    }
}
