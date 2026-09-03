use super::lifecycle::default_plugin_root;
use super::*;

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

    fn session_context(&self) -> SessionContext<'_> {
        SessionContext {
            core: self.core.as_ref(),
            audio: self.core.audio(),
            runtime: self.runtime.as_ref(),
            data_root: &self.data_root,
            safe_mode: self.core.safe_mode(),
            events: self.events.as_ref(),
        }
    }

    pub(crate) fn dispatch_request(&self, request: ControlRequest) -> ControlResponse {
        self.dispatch_request_with_shutdown(request, false)
    }

    pub(super) fn dispatch_persistence_request(&self, request: ControlRequest) -> ControlResponse {
        self.dispatch_request_inner(request, true)
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
        self.dispatch_request_inner(request, allow_shutdown)
    }

    fn dispatch_request_inner(
        &self,
        request: ControlRequest,
        allow_shutdown: bool,
    ) -> ControlResponse {
        if !allow_shutdown && self.shutting_down.load(Ordering::Acquire) {
            return Self::failure(
                request.request_id,
                ProtocolError::new(ErrorCode::HostUnavailable, "Riffra Host has shut down"),
            );
        }
        let _command_gate = if requires_command_gate(request.command.as_str()) {
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
        match self.dispatch(request.command.as_str(), request.params, current) {
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
    ) -> Result<(&'static str, Value, u64), ProtocolError> {
        if command == "audio.master-gain.set" {
            let params: MasterGainParams = decode(params)?;
            let pair = session_adapter::set_master_gain_db(&self.session_context(), params.gain_db)
                .map_err(|error| error.protocol_error())?;
            return Ok((
                "sessionAudioPair",
                serde_json::to_value(&pair).map_err(serialize_error)?,
                pair.canonical.sequence,
            ));
        }
        if !is_host_runtime_command(command) {
            if let Some(result) =
                self.dispatch_shared_session(command, params.clone(), current.sequence)?
            {
                return Ok(result);
            }
            let current_sequence = current.sequence;
            let result = HostDispatcher::borrowed(&self.core, &self.storage, &self.data_root)
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
                session_adapter::open_track_plugin_editor(
                    &self.session_context(),
                    &params.track_id,
                    &params.device_id,
                )
                .map_err(|error| error.protocol_error())?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "take.comparison.start" => {
                let params: TakeIdParams = decode(params)?;
                let status = session_adapter::start_take_comparison(
                    &self.session_context(),
                    &params.take_id,
                )
                .map_err(|error| error.protocol_error())?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "take.comparison.switch" => {
                let params: TakeComparisonParams = decode(params)?;
                let status = session_adapter::switch_take_comparison_variant(
                    &self.session_context(),
                    params.variant,
                )
                .map_err(|error| error.protocol_error())?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "take.comparison.stop" => {
                let status = session_adapter::stop_take_comparison(&self.session_context())
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
                if self.runtime.reset_for_repair() {
                    Ok((
                        "runtimeProjection",
                        serde_json::to_value(self.runtime.status()).map_err(serialize_error)?,
                        current.sequence,
                    ))
                } else {
                    Err(ProtocolError::new(
                        ErrorCode::CommandFailed,
                        "runtime projection is not waiting for repair",
                    ))
                }
            }
            "transport.play" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                let params: TransportParams = decode(params)?;
                self.runtime
                    .apply_and_play(
                        params.transport_sequence,
                        crate::runtime_snapshot::runtime_timeline_snapshot(
                            &self.data_root,
                            &current.session,
                        ),
                        riffra_core::ProjectionKey {
                            sequence: current.sequence,
                            session_revision: current.session.arrangement.revision,
                        },
                        std::time::Duration::from_secs(30),
                    )
                    .map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.stop" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                let params: TransportParams = decode(params)?;
                self.runtime
                    .stop(params.transport_sequence)
                    .map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.go-to-start" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                let params: TransportParams = decode(params)?;
                self.runtime
                    .stop_and_seek_to_start(params.transport_sequence, || {
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
                    core: &self.core,
                    audio: self.core.audio(),
                    runtime: &self.runtime,
                    data_root: &self.data_root,
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
    ) -> Result<Option<(&'static str, Value, u64)>, ProtocolError> {
        let context = self.session_context();
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
            "instrument.set" => {
                let params: PluginPathParams = decode(params)?;
                Some(
                    session_adapter::set_track_instrument_with_expected_sequence(
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
            "project.import-scratch" => {
                let params: ProjectImportParams = decode(params)?;
                Some(
                    session_adapter::import_session(&context, &params.path)
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

    fn after_canonical_commit(
        &self,
        effect: CanonicalMutationEffect,
    ) -> Result<crate::model::ArrangementMutationResult, ProtocolError> {
        let canonical = self
            .canonical()
            .map_err(|error| command_error(error.to_string()))?;
        library::index::refresh(&self.data_root, &canonical.session);
        self.events
            .emit(HostEvent::CanonicalStateChanged(canonical.clone()));
        let mutation = commit::finalize_arrangement_mutation(
            canonical,
            self.runtime.as_ref(),
            &self.data_root,
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
            ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
        }
        RuntimeError::ShuttingDown => {
            ProtocolError::new(ErrorCode::RuntimeUnavailable, "runtime is shutting down")
        }
        error => ProtocolError::new(ErrorCode::CommandFailed, error.to_string()),
    }
}

fn audio_error(error: crate::NativeAudioError) -> ProtocolError {
    ProtocolError::new(ErrorCode::RuntimeUnavailable, error.to_string())
}

fn serialize_error(error: serde_json::Error) -> ProtocolError {
    command_error(error.to_string())
}

fn requires_command_gate(command: &str) -> bool {
    !is_host_runtime_command(command)
        && !matches!(
            command,
            // These operations validate and prepare an external VST candidate
            // before attempting the canonical commit. Their expected
            // sequence is checked by the adapter at commit time, so holding
            // the short canonical-operation gate across process work would
            // only block unrelated reads and transport controls.
            "instrument.set" | "effect.add" | "missing.replace-plugin"
        )
}

fn is_host_runtime_command(command: &str) -> bool {
    matches!(
        command,
        "host.status"
            | "host.info"
            | "host.bootstrap"
            | "host.shutdown"
            | "audio.master-gain.preview"
            | "audio.emergency-mute"
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
struct SeekParams {
    tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransportParams {
    transport_sequence: u64,
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
struct ProjectImportParams {
    path: PathBuf,
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
    fn safe_mode_host_publishes_endpoint_and_handles_attached_mutation() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-host-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
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
                ),
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
            );
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
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();

        let mutation = host.dispatch_control(ControlRequest::new(
            "track-add",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
            ),
            Some(0),
        ));
        assert!(mutation.ok);
        assert_eq!(mutation.sequence, Some(1));

        let undo = host.dispatch_control(ControlRequest::new(
            "stale-undo",
            ControlCommand::new("undo", serde_json::json!({})),
            Some(0),
        ));
        assert!(!undo.ok);
        assert_eq!(
            undo.error.as_ref().map(|error| error.code),
            Some(ErrorCode::Conflict)
        );

        let render = host.dispatch_control(ControlRequest::new(
            "stale-render",
            ControlCommand::new("render.start", serde_json::json!({})),
            Some(0),
        ));
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

        let mutation = client
            .request(&ControlRequest::new(
                "track-add",
                ControlCommand::new(
                    "track.add",
                    serde_json::json!({"name": "Synth", "kind": "instrument"}),
                ),
                Some(0),
            ))
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
            safe_mode: false,
            binaries: RuntimeBinaries::new(
                data_root.join("missing-riffra-audio"),
                data_root.join("missing-riffra-plugin-scan"),
                data_root.join("missing-riffra-render"),
            ),
        };
        let host = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();

        let response = host.dispatch_control(ControlRequest::new(
            "track-add",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
            ),
            Some(0),
        ));

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

        let marker = host.dispatch_control(ControlRequest::new(
            "marker-add",
            ControlCommand::new(
                "marker.add",
                serde_json::json!({"name": "Verse", "tick": 0}),
            ),
            Some(1),
        ));
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

        let settings = host.dispatch_control(ControlRequest::new(
            "session-settings-update",
            ControlCommand::new(
                "session.settings.update",
                serde_json::json!({"note": "authoring note"}),
            ),
            Some(2),
        ));
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
