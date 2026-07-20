import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  AssetId,
  BackgroundJobStatus,
  BootstrapState,
  CreativeSession,
  DesignTool,
  LibraryAsset,
  MissingDependency,
  MidiProbe,
  PluginEntry,
  RecordingAsset,
  RenderOptions,
  RenderResult,
  ScanReport,
  SeparationResult,
  Workspace,
} from '@/lib/domain';
import { isUsableRecording } from '@/lib/recordings';
import { audioCommandSucceeded } from '@/lib/audio-safety';
import { defaultNativeApi } from '@/native/native';
import type { NativeApi } from '@/native/native-api';
import { workspaces } from '@/constants';
import { useLibrary } from './useLibrary';
import { useInbox } from './useInbox';
import { useSession } from './useSession';
import { useAudio } from './useAudio';

export function useApp(api: NativeApi = defaultNativeApi) {
  const {
    bootstrap,
    startAnalysisJob,
    startSeparationJob,
    startRenderJob,
    startRenderStemsJob,
    startScanJob,
    getBackgroundJob,
    cancelBackgroundJob,
    listRecordings,
    analyzeAsset,
    probeMidiDevices,
    probeAudioDevices,
    listSeparations,
    renderTimeline,
    loadPluginIntoRack: loadPluginIntoRackApi,
    clearPluginFromRack: clearPluginFromRackApi,
    openPluginEditor: openPluginEditorApi,
    setRackPluginBypassed,
    setRackPluginParameter,
    restoreCurrentRack,
    createSamplePad: createSamplePadApi,
    updateSamplePad: updateSamplePadApi,
    removeSamplePad: removeSamplePadApi,
    previewAsset: previewAssetApi,
    stopSamplePreview,
    stopSamplePreviewKey,
    getAudioStatus,
    setEmergencyMute,
    setMasterGainDb,
    getMissingDependencies,
    relinkMissingDependency,
    disableMissingPlugin,
    addAudioClipToArrangement,
    openAssetInDesign: openAssetInDesignApi,
    switchWorkspace: switchWorkspaceApi,
    saveRackDefinition,
    listRackDefinitions,
    loadRackDefinitionAsset,
    sendMidiToPlugin,
  } = api;
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [audio, setAudio] = useState<AudioStatus>({
    state: 'starting',
    driver: null,
    inputDevice: null,
    inputChannel: null,
    inputChannels: [],
    outputDevice: null,
    outputChannels: [],
    sampleRate: null,
    bufferSize: null,
    roundTripMs: null,
    recording: {
      active: false,
      directory: null,
      sampleRate: null,
      rawChannels: null,
      processedChannels: null,
      samplesWritten: 0,
      droppedBlocks: 0,
      missingSamples: 0,
      dropoutStartSample: null,
      dropoutEndSample: null,
      recoveryStatus: 'clean',
    },
    midiInputs: [],
    midiOutputs: [],
    midiInputActive: false,
    midiMessages: 0,
    lastMidiNote: null,
    midiPadMappings: 0,
    midiPadTriggers: 0,
    inputPeak: 0,
    outputPeak: 0,
    invalidSamples: 0,
    feedbackSuspected: false,
    message: 'Audio supervisor is starting.',
  });
  const [plugins, setPlugins] = useState<PluginEntry[]>([]);
  const [missingPluginPaths, setMissingPluginPaths] = useState<string[]>([]);
  const [missingDependencies, setMissingDependencies] = useState<MissingDependency[]>([]);
  const [recordings, setRecordings] = useState<RecordingAsset[]>([]);
  const [separations, setSeparations] = useState<SeparationResult[]>([]);
  const [separationBusy, setSeparationBusy] = useState<string | null>(null);
  const [separationMessage, setSeparationMessage] = useState(
    'Ready for a local stereo channel split.',
  );
  const [separationPreviewingAssetId, setSeparationPreviewingAssetId] = useState<AssetId | null>(
    null,
  );
  const [renderResult, setRenderResult] = useState<RenderResult | null>(null);
  const [stemResults, setStemResults] = useState<RenderResult[]>([]);
  const [renderPreviewing, setRenderPreviewing] = useState(false);
  const [transportPlaying, setTransportPlaying] = useState(false);
  const [renderMessage, setRenderMessage] = useState('Timeline render has not been requested.');
  const [previewPadId, setPreviewPadId] = useState<string | null>(null);
  const [midi, setMidi] = useState<MidiProbe>({
    inputs: [],
    outputs: [],
    refreshedAtMs: 0,
    message: 'MIDI device list has not been refreshed.',
  });
  const [deviceProbe, setDeviceProbe] = useState<AudioDeviceProbe>({
    drivers: [],
    midiInputs: [],
    midiOutputs: [],
    refreshedAtMs: 0,
    message: 'Audio device list has not been refreshed.',
  });
  const [analysis, setAnalysis] = useState<AudioAnalysis | null>(null);
  const [referenceId, setReferenceId] = useState<string | null>(null);
  const [referencePreviewingId, setReferencePreviewingId] = useState<string | null>(null);
  const [referenceSyncPreviewing, setReferenceSyncPreviewing] = useState(false);
  const [referenceLoopPreview, setReferenceLoopPreview] = useState(false);
  const [referenceAnalyses, setReferenceAnalyses] = useState<Record<string, AudioAnalysis>>({});
  const [, setScanMessage] = useState('VST3を検出中…');
  const [commandOpen, setCommandOpen] = useState(false);
  const [focusMode, setFocusMode] = useState(false);
  const [backgroundJob, setBackgroundJob] = useState<BackgroundJobStatus | null>(null);
  const activeJobId = useRef<string | null>(null);

  const library = useLibrary(api, { setAudio, setPreviewPadId });
  const reloadRecordings = useCallback(async () => {
    setRecordings(await listRecordings());
  }, [listRecordings]);
  const {
    librarySection,
    setLibrarySection,
    libraryQuery,
    setLibraryQuery,
    libraryResults,
    setLibraryResults,
    selectedLibraryAsset,
    setSelectedLibraryAsset,
    relatedAssets,
    setRelatedAssets,
    query,
    selectLibraryAsset,
    previewSelectedLibraryAsset,
    editSelectedLibraryAsset,
  } = library;

  const sessionHook = useSession(api, { setBoot, setAudio, setMissingPluginPaths });
  const {
    session,
    setSession,
    undoStack,
    setUndoStack,
    redoStack,
    setRedoStack,
    autosaveError,
    setAutosaveError,
    exportMessage,
    setExportMessage,
    previousSession,
    historySkip,
    undo,
    redo,
    captureSnapshot,
    recallSnapshot,
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
  } = sessionHook;
  // UI helper for applying a Rust Session Operation and surfacing a rejected
  // intent. Production state is never assembled or flushed from React here.
  const runSessionOp = useCallback(
    async <T>(op: () => Promise<T | null>, label: string): Promise<T | null> => {
      const result = await op();
      if (result == null) {
        setAutosaveError(`${label} could not be applied.`);
        return null;
      }
      setAutosaveError(null);
      return result;
    },
    [setAutosaveError],
  );
  const switchWorkspace = useCallback(
    async (workspace: Workspace) => {
      const next = await runSessionOp(() => switchWorkspaceApi(workspace), 'Workspace switch');
      if (next) setSession(next);
    },
    [runSessionOp, setSession, switchWorkspaceApi],
  );
  const openAssetInDesign = useCallback(
    async (assetId: AssetId, tool: DesignTool): Promise<void> => {
      const next = await runSessionOp(
        () => openAssetInDesignApi(assetId, tool),
        'Open asset in Design',
      );
      if (next) setSession(next);
    },
    [openAssetInDesignApi, runSessionOp, setSession],
  );
  const clearRelocatedMissingDependencies = useCallback(
    (recording: RecordingAsset) => {
      const previousDirectory = recording.path.replace(/[\\/]+$/, '').toLocaleLowerCase();
      setMissingDependencies((current) =>
        current.filter((item) => {
          const path = item.path.toLocaleLowerCase();
          return !(
            path === previousDirectory ||
            (path.startsWith(previousDirectory) &&
              /^[\\/]/.test(path.slice(previousDirectory.length)))
          );
        }),
      );
    },
    [setMissingDependencies],
  );
  const inbox = useInbox(api, recordings, {
    reload: reloadRecordings,
    onRelocate: clearRelocatedMissingDependencies,
  });

  const audioHook = useAudio(api, {
    audio,
    setAudio,
    session,
    setRecordings,
  });
  const {
    audioPreferenceMessage,
    setAudioPreferenceMessage,
    recordCountdown,
    setRecordCountdown,
    recordingCommandPending,
    setRecordingCommandPending,
    recordingCommandLock,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
    disableMidi,
    toggleMute,
    startRecordingNow,
    toggleRecording,
  } = audioHook;

  const loadPluginIntoRack = useCallback(
    async (
      plugin: PluginEntry,
      parameterValues: number[] = [],
      bypassed = false,
      stateData: string | null = null,
    ) => {
      const { session: nextSession, audio: nextAudio } = await loadPluginIntoRackApi(
        plugin.path,
        parameterValues,
        bypassed,
        stateData,
      );
      setAudio(nextAudio);
      setSession(nextSession);
    },
    [loadPluginIntoRackApi],
  );

  const clearPluginFromRack = useCallback(async () => {
    const { session: nextSession, audio: nextAudio } = await clearPluginFromRackApi();
    setAudio(nextAudio);
    setSession(nextSession);
  }, [clearPluginFromRackApi]);

  const openPluginEditor = useCallback(async () => {
    setAudio(await openPluginEditorApi());
  }, [openPluginEditorApi]);

  const sendMidi = useCallback(
    async (bytes: number[]) => {
      setAudio(await sendMidiToPlugin(bytes));
    },
    [sendMidiToPlugin],
  );

  const togglePluginBypass = useCallback(
    async (bypassed: boolean) => {
      const { session: nextSession, audio: nextAudio } = await setRackPluginBypassed(bypassed);
      setAudio(nextAudio);
      setSession(nextSession);
    },
    [setRackPluginBypassed],
  );

  const setPluginParameterValue = useCallback(
    async (index: number, value: number) => {
      const { session: nextSession, audio: nextAudio } = await setRackPluginParameter(index, value);
      setAudio(nextAudio);
      setSession(nextSession);
    },
    [setRackPluginParameter],
  );

  const runBackgroundJob = useCallback(
    async (
      start: () => Promise<BackgroundJobStatus>,
      onCompleted: (result: unknown) => void,
      onFailed: (message: string) => void,
    ): Promise<boolean> => {
      if (activeJobId.current) return false;
      let started: BackgroundJobStatus;
      try {
        started = await start();
      } catch (error) {
        onFailed(error instanceof Error ? error.message : String(error));
        return false;
      }
      activeJobId.current = started.id;
      setBackgroundJob(started);
      let latest = started;
      try {
        while (!['completed', 'failed', 'cancelled'].includes(latest.state)) {
          await new Promise((resolve) => window.setTimeout(resolve, 75));
          const next = await getBackgroundJob(started.id);
          if (!next) {
            onFailed('Background job disappeared before it reported a result.');
            return false;
          }
          latest = next;
          setBackgroundJob(next);
        }
        if (latest.state !== 'completed') {
          onFailed(latest.message);
          return false;
        }
        onCompleted(latest.result);
        return true;
      } catch (error) {
        onFailed(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        activeJobId.current = null;
        window.setTimeout(
          () => setBackgroundJob((current) => (current?.id === started.id ? null : current)),
          500,
        );
      }
    },
    [getBackgroundJob],
  );

  const cancelActiveJob = useCallback(async () => {
    const id = activeJobId.current;
    if (!id) return;
    const status = await cancelBackgroundJob(id);
    if (status) setBackgroundJob(status);
  }, [cancelBackgroundJob]);

  const openRecordingAnalysis = useCallback(
    async (recording: RecordingAsset) => {
      if (!isUsableRecording(recording)) return;
      if (recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      await runBackgroundJob(
        () => startAnalysisJob(assetId),
        (result) => {
          if (!result || typeof result !== 'object') return;
          setAnalysis(result as AudioAnalysis);
          void openAssetInDesign(assetId, 'analyze');
        },
        () => setAnalysis(null),
      );
    },
    [openAssetInDesign, runBackgroundJob, startAnalysisJob],
  );

  const openLibraryAssetAnalysis = useCallback(
    async (asset: LibraryAsset) => {
      if (asset.kind !== 'audio') return;
      const result = await analyzeAsset(asset.id);
      if (!result) return;
      setAnalysis(result);
      await openAssetInDesign(asset.id, 'analyze');
    },
    [analyzeAsset, openAssetInDesign],
  );

  const saveCurrentRack = useCallback(async () => {
    if (!session) return;
    const path = window.prompt('Rack definition path', 'rack-definition.json');
    if (!path?.trim()) return;
    const name = window.prompt('Rack definition name', 'Rack Definition');
    if (!name?.trim()) return;
    await saveRackDefinition(name.trim(), path.trim());
    setRackDefinitions(await listRackDefinitions());
  }, [listRackDefinitions, saveRackDefinition, session]);

  const loadSavedRack = useCallback(
    async (assetId: AssetId) => {
      const result = await loadRackDefinitionAsset(assetId);
      if (!result) return;
      setSession(result.session);
      setAudio(result.audio);
    },
    [loadRackDefinitionAsset, setAudio, setSession],
  );

  const [rackDefinitions, setRackDefinitions] = useState<LibraryAsset[]>([]);
  useEffect(() => {
    void listRackDefinitions().then(setRackDefinitions);
  }, [listRackDefinitions, saveRackDefinition]);

  const selectReference = useCallback(
    async (recording: RecordingAsset) => {
      if (recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      setReferenceId(recording.id);
      const existing = referenceAnalyses[recording.id];
      if (existing) return;
      const next = await analyzeAsset(assetId);
      if (next) setReferenceAnalyses((current) => ({ ...current, [recording.id]: next }));
    },
    [analyzeAsset, referenceAnalyses],
  );

  const previewReference = useCallback(
    async (recording: RecordingAsset) => {
      if (recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      await stopSamplePreview();
      setAudio(await previewAssetApi(assetId, { looped: referenceLoopPreview }));
      setReferencePreviewingId(recording.id);
      setReferenceSyncPreviewing(false);
    },
    [previewAssetApi, referenceLoopPreview, stopSamplePreview],
  );

  const previewReferencePair = useCallback(async () => {
    const targetAssetId = session?.designContext.targetAssetId;
    if (!analysis || !targetAssetId || !referenceId) return;
    const reference = recordings.find((recording) => recording.id === referenceId);
    if (!reference) return;
    const referenceAssetId = reference.processedAssetId ?? reference.rawAssetId;
    if (!referenceAssetId) return;
    await stopSamplePreview();
    await previewAssetApi(targetAssetId, { looped: referenceLoopPreview });
    setAudio(await previewAssetApi(referenceAssetId, { looped: referenceLoopPreview }));
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(true);
  }, [
    analysis,
    previewAssetApi,
    recordings,
    referenceId,
    referenceLoopPreview,
    session,
    stopSamplePreview,
  ]);

  const stopReferencePreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(false);
  }, []);

  const runSeparation = useCallback(
    async (recording: RecordingAsset) => {
      if (recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      await openAssetInDesign(assetId, 'separate');
      setSeparationBusy(recording.id);
      setSeparationMessage('Writing Left / Right WAV assets…');
      await runBackgroundJob(
        () => startSeparationJob(assetId),
        (value) => {
          const result = value as SeparationResult;
          setSeparations((current) => [result, ...current.filter((item) => item.id !== result.id)]);
          setSeparationMessage(result.message);
        },
        (message) => setSeparationMessage(`Separation failed: ${message}`),
      );
      setSeparationBusy(null);
    },
    [openAssetInDesign, runBackgroundJob, startSeparationJob],
  );

  const previewSeparation = useCallback(
    async (assetId: AssetId) => {
      setAudio(await previewAssetApi(assetId, {}));
      setSeparationPreviewingAssetId(assetId);
    },
    [previewAssetApi],
  );

  const stopSeparationPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setSeparationPreviewingAssetId(null);
  }, [stopSamplePreview]);

  const addSeparationToTimeline = useCallback(
    async (assetId: AssetId, name: string, durationMs: number) => {
      if (!session) return;
      const next = await runSessionOp(
        () => addAudioClipToArrangement(assetId, name, durationMs),
        'Add clip to timeline',
      );
      if (next) setSession(next);
    },
    [addAudioClipToArrangement, runSessionOp, session, setSession],
  );

  const runTimelineRender = useCallback(
    async (options: RenderOptions) => {
      setRenderResult(null);
      setStemResults([]);
      setRenderPreviewing(false);
      setRenderMessage('Rendering a new stereo WAV…');
      await runBackgroundJob(
        () => startRenderJob(options),
        (value) => {
          const result = value as RenderResult;
          setRenderResult(result);
          setRenderMessage(result.message);
        },
        (message) => setRenderMessage(`Render failed: ${message}`),
      );
    },
    [runBackgroundJob, startRenderJob],
  );

  const runTimelineStemRender = useCallback(
    async (options: RenderOptions) => {
      setRenderResult(null);
      setStemResults([]);
      setRenderPreviewing(false);
      setRenderMessage('Rendering independent track stems…');
      await runBackgroundJob(
        () => startRenderStemsJob(options),
        (value) => {
          const results = Array.isArray(value) ? (value as RenderResult[]) : [];
          if (!results.length) {
            setRenderMessage(
              'Stem render returned no completed result; source clips remain unchanged.',
            );
            return;
          }
          setStemResults(results);
          setRenderMessage(`${results.length} track stems rendered without changing source clips.`);
        },
        (message) => setRenderMessage(`Stem render failed: ${message}`),
      );
    },
    [runBackgroundJob, startRenderStemsJob],
  );

  const previewTimelineRender = useCallback(async () => {
    if (!renderResult) return;
    setAudio(
      await previewAssetApi(renderResult.assetId, {
        looped: session?.settings.loopEnabled ?? false,
      }),
    );
    setRenderPreviewing(true);
    setTransportPlaying(true);
  }, [previewAssetApi, renderResult, session?.settings.loopEnabled]);

  const stopTimelinePreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setRenderPreviewing(false);
    setTransportPlaying(false);
  }, []);

  const playTransport = useCallback(async () => {
    if (!session) return;
    let result = renderResult;
    if (!result) {
      result = await renderTimeline({
        rangeStartMs: 0,
        rangeEndMs: null,
        normalize: false,
        trackId: null,
      });
      if (!result) return;
      setRenderResult(result);
    }
    setAudio(await previewAssetApi(result.assetId, { looped: session.settings.loopEnabled }));
    setTransportPlaying(true);
  }, [previewAssetApi, renderResult, renderTimeline, session]);

  const stopTransport = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setTransportPlaying(false);
    setRenderPreviewing(false);
  }, []);

  const previewSamplePad = useCallback(
    async (pad: CreativeSession['playState']['sampleInstrument']['pads'][number]) => {
      const nextAudio = await previewAssetApi(pad.assetId, {
        startMs: pad.startMs,
        endMs: pad.endMs,
        looped: pad.loopEnabled,
        gain: Math.pow(10, (pad.gainDb ?? 0) / 20),
        voiceKey: pad.midiKey,
      });
      setAudio(nextAudio);
      setPreviewPadId(pad.id);
    },
    [previewAssetApi],
  );

  const stopPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setPreviewPadId(null);
  }, []);

  const relinkMissing = useCallback(async (item: MissingDependency, newPath: string) => {
    if (!item.assetId) return;
    const next = await relinkMissingDependency(item.assetId, newPath);
    setSession(next);
    setMissingDependencies(await getMissingDependencies());
  }, []);

  const disableMissingPluginDevice = useCallback(async (deviceId: string) => {
    const next = await disableMissingPlugin(deviceId);
    setSession(next);
    setMissingDependencies(await getMissingDependencies());
  }, []);

  const ignoreMissing = useCallback((item: MissingDependency) => {
    setMissingDependencies((current) =>
      current.filter((candidate) => !(candidate.kind === item.kind && candidate.id === item.id)),
    );
  }, []);

  const placeRecording = useCallback(
    async (recording: RecordingAsset) => {
      if (!session) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      const durationMs = Math.max(
        1,
        Math.round((recording.samplesWritten / (recording.sampleRate ?? 1)) * 1_000),
      );
      const next = await runSessionOp(
        () => addAudioClipToArrangement(assetId, recording.name, durationMs),
        'Place recording',
      );
      if (next) setSession(next);
    },
    [addAudioClipToArrangement, runSessionOp, session, setSession],
  );

  const createSamplePad = useCallback(
    async (recording: RecordingAsset) => {
      if (!session || recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      const { session: nextSession, audio: nextAudio } = await createSamplePadApi(
        assetId,
        recording.name,
      );
      setSession(nextSession);
      setAudio(nextAudio);
    },
    [createSamplePadApi, session],
  );

  const updateSamplePad = useCallback(
    async (
      padId: string,
      patch: {
        startMs?: number;
        endMs?: number;
        gainDb?: number;
        loopEnabled?: boolean;
      },
    ) => {
      const { session: nextSession, audio: nextAudio } = await updateSamplePadApi(padId, patch);
      setSession(nextSession);
      setAudio(nextAudio);
    },
    [updateSamplePadApi],
  );

  const removeSamplePad = useCallback(
    async (padId: string) => {
      const { session: nextSession, audio: nextAudio } = await removeSamplePadApi(padId);
      setSession(nextSession);
      setAudio(nextAudio);
    },
    [removeSamplePadApi],
  );

  useEffect(() => {
    void bootstrap().then((state) => {
      setBoot(state);
      setSession(state.session);
      void getMissingDependencies().then(setMissingDependencies);
      if (!state.safeMode)
        void (async () => {
          const result = await setMasterGainDb(state.session.settings.masterDb);
          setAudio(result.audio);
          setSession(result.session);
          let startupAudio = result.audio;
          try {
            startupAudio = await restoreCurrentRack();
            setAudio(startupAudio);
          } catch (error) {
            setScanMessage(
              `Rack restore failed: ${error instanceof Error ? error.message : String(error)}`,
            );
          }
          if (audioCommandSucceeded(startupAudio) && !startupAudio.feedbackSuspected) {
            setAudio(await setEmergencyMute(false));
          }
        })();
      void runBackgroundJob(
        () => startScanJob(state.vst3Root),
        (value) => {
          const report = value as ScanReport;
          setPlugins(report.plugins);
          setMissingPluginPaths(
            state.session.rack.devices
              .filter((device) => device.kind === 'plugin' && device.path)
              .filter(
                (device) =>
                  !report.plugins.some(
                    (plugin) => plugin.path === device.path && plugin.scanState === 'validated',
                  ),
              )
              .map((device) => device.path as string),
          );
          setScanMessage(
            report.issues.length
              ? `${report.plugins.length}件 · ${report.issues.length}件の注意`
              : `${report.plugins.length}件を検出`,
          );
        },
        (message) => setScanMessage(`VST3 scan failed: ${message}`),
      );
    });
    void listRecordings().then(setRecordings);
    void listSeparations().then(setSeparations);
    void probeMidiDevices().then(setMidi);
    void probeAudioDevices().then(setDeviceProbe);
    void enableMidi();
    let cancelled = false;
    let audioPoll: number | null = null;
    const refreshAudio = async () => {
      try {
        const nextAudio = await getAudioStatus();
        if (!cancelled) setAudio(nextAudio);
      } finally {
        if (!cancelled) audioPoll = window.setTimeout(refreshAudio, 200);
      }
    };
    void refreshAudio();
    return () => {
      cancelled = true;
      if (audioPoll !== null) window.clearTimeout(audioPoll);
    };
  }, []);

  useEffect(() => {
    const pluginIsInstrument =
      audio.plugin != null && audio.plugin.loaded && audio.plugin.inputChannels === 0;
    const keyboardKeys = ['z', 's', 'x', 'd', 'c', 'v', 'g', 'b', 'h', 'n', 'j', 'm'];
    const onKey = (event: KeyboardEvent) => {
      if (pluginIsInstrument) return;
      const target = event.target as HTMLElement | null;
      if (target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA') return;
      const index = keyboardKeys.indexOf(event.key.toLowerCase());
      const pad = index >= 0 ? session?.playState.sampleInstrument.pads[index] : undefined;
      if (pad) {
        event.preventDefault();
        void previewSamplePad(pad);
      }
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (pluginIsInstrument) return;
      const target = event.target as HTMLElement | null;
      if (target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA') return;
      const index = keyboardKeys.indexOf(event.key.toLowerCase());
      const pad = index >= 0 ? session?.playState.sampleInstrument.pads[index] : undefined;
      if (pad?.loopEnabled) {
        event.preventDefault();
        void stopSamplePreviewKey(pad.midiKey).then(setAudio);
      }
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('keyup', onKeyUp);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('keyup', onKeyUp);
    };
  }, [previewSamplePad, session?.playState.sampleInstrument.pads, audio.plugin]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing = target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA';
      if (event.ctrlKey && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setCommandOpen((open) => !open);
        return;
      }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === 'z') {
        event.preventDefault();
        if (event.shiftKey) {
          void redo();
        } else {
          void undo();
        }
        return;
      }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === 'y') {
        event.preventDefault();
        void redo();
        return;
      }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'm') {
        event.preventDefault();
        void toggleMute();
        return;
      }
      if (!typing && event.key >= '1' && event.key <= '4')
        void switchWorkspace(workspaces[Number(event.key) - 1].id);
      if (event.key === 'Escape') setCommandOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [redo, switchWorkspace, toggleMute, undo]);

  const persistedPlugin = session?.rack.devices.find((device) => device.kind === 'plugin') ?? null;
  const selectedPlugin = useMemo(() => {
    if (persistedPlugin?.path) {
      return plugins.find((plugin) => plugin.path === persistedPlugin.path) ?? null;
    }
    if (audio.plugin?.name) {
      return plugins.find((plugin) => plugin.name === audio.plugin?.name) ?? null;
    }
    return null;
  }, [audio.plugin?.name, persistedPlugin?.path, plugins]);
  const selectedPluginName =
    selectedPlugin?.name ?? persistedPlugin?.name ?? audio.plugin?.name ?? null;
  const selectedPluginVendor = selectedPlugin?.vendor ?? (selectedPluginName ? 'VST3' : null);
  const visiblePlugins = query
    ? plugins.filter((plugin) =>
        `${plugin.name} ${plugin.vendor ?? ''} ${plugin.path}`.toLowerCase().includes(query),
      )
    : plugins;
  const visibleRecordings = query
    ? recordings.filter((recording) =>
        `${recording.name} ${recording.state} ${recording.path}`.toLowerCase().includes(query),
      )
    : recordings;
  const usableRecordings = recordings.filter(isUsableRecording);
  return {
    boot,
    setBoot,
    session,
    setSession,
    audio,
    setAudio,
    audioPreferenceMessage,
    setAudioPreferenceMessage,
    autosaveError,
    setAutosaveError,
    plugins,
    setPlugins,
    missingPluginPaths,
    setMissingPluginPaths,
    missingDependencies,
    relinkMissing,
    disableMissingPluginDevice,
    ignoreMissing,
    recordings,
    setRecordings,
    separations,
    setSeparations,
    separationBusy,
    setSeparationBusy,
    separationMessage,
    setSeparationMessage,
    separationPreviewingAssetId,
    setSeparationPreviewingAssetId,
    renderResult,
    setRenderResult,
    stemResults,
    setStemResults,
    renderPreviewing,
    setRenderPreviewing,
    transportPlaying,
    setTransportPlaying,
    recordCountdown,
    setRecordCountdown,
    recordingCommandPending,
    setRecordingCommandPending,
    renderMessage,
    setRenderMessage,
    previewPadId,
    setPreviewPadId,
    exportMessage,
    setExportMessage,
    midi,
    setMidi,
    deviceProbe,
    setDeviceProbe,
    analysis,
    setAnalysis,
    referenceId,
    setReferenceId,
    referencePreviewingId,
    setReferencePreviewingId,
    referenceSyncPreviewing,
    setReferenceSyncPreviewing,
    referenceLoopPreview,
    setReferenceLoopPreview,
    referenceAnalyses,
    setReferenceAnalyses,
    librarySection,
    setLibrarySection,
    libraryQuery,
    setLibraryQuery,
    libraryResults,
    setLibraryResults,
    selectedLibraryAsset,
    setSelectedLibraryAsset,
    relatedAssets,
    setRelatedAssets,
    commandOpen,
    setCommandOpen,
    focusMode,
    setFocusMode,
    backgroundJob,
    cancelActiveJob,
    undoStack,
    setUndoStack,
    redoStack,
    setRedoStack,
    previousSession,
    historySkip,
    recordingCommandLock,
    loadPluginIntoRack,
    clearPluginFromRack,
    openPluginEditor,
    sendMidi,
    togglePluginBypass,
    setPluginParameterValue,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
    disableMidi,
    undo,
    redo,
    captureSnapshot,
    recallSnapshot,
    openRecordingAnalysis,
    openLibraryAssetAnalysis,
    selectReference,
    previewReference,
    previewReferencePair,
    stopReferencePreview,
    runSeparation,
    previewSeparation,
    stopSeparationPreview,
    addSeparationToTimeline,
    runSessionOp,
    runTimelineRender,
    runTimelineStemRender,
    previewTimelineRender,
    stopTimelinePreview,
    playTransport,
    stopTransport,
    previewSamplePad,
    stopPreview,
    placeRecording,
    createSamplePad,
    updateSamplePad,
    removeSamplePad,
    saveCurrentRack,
    loadSavedRack,
    rackDefinitions,
    switchWorkspace,
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
    selectLibraryAsset,
    editSelectedLibraryAsset,
    previewSelectedLibraryAsset,
    toggleMute,
    startRecordingNow,
    toggleRecording,
    persistedPlugin,
    selectedPlugin,
    selectedPluginName,
    selectedPluginVendor,
    query,
    visiblePlugins,
    visibleRecordings,
    usableRecordings,
    inbox,
    setScanMessage,
    api,
  };
}
