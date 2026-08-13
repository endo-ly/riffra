import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  AssetId,
  BootstrapState,
  CreativeSession,
  DesktopViewState,
  DesignTool,
  LibraryAsset,
  MissingDependency,
  MidiProbe,
  PluginEntry,
  RecordingAsset,
  RenderResult,
  ScanReport,
  SeparationResult,
  Workspace,
} from '@/model/domain';
import { defaultViewState, toAssetId } from '@/model/domain';
import { isUsableRecording } from '@/shared/recordings';
import { isEditableTypingTarget } from '@/features/arrange/play-surface/musical-typing';
import { startingAudioStatus } from '@/shared/audio/audio-defaults';
import type { AudioMeters } from '@/shared/audio/audio-meters';
import { publishAudioMeters } from '@/shared/audio/audio-meters';
import { logNativeError } from '@/native/invoke';
import { defaultNativeApi } from '@/native/native';
import type { NativeApi } from '@/native/native-api';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { workspaces } from '@/app/workspaces';
import { useRuntimeRestartNotification } from '@/app/runtime/useRuntimeRestartNotification';
import { useBackgroundJobs } from '@/app/runtime/useBackgroundJobs';
import { useTransportController } from '@/features/transport/useTransportController';
import { useWorkspaceNavigation } from '@/app/navigation/useWorkspaceNavigation';
import { useLibrary } from '@/features/library/useLibrary';
import { useInbox } from '@/features/library/useInbox';
import { useProject } from '@/features/project/useProject';
import { useAudioSettings } from '@/features/settings/useAudioSettings';

export function useAppController(api: NativeApi = defaultNativeApi) {
  const {
    bootstrap,
    startAnalysisJob,
    startSeparationJob,
    startScanJob,
    listRecordings,
    analyzeAsset,
    probeMidiDevices,
    probeAudioDevices,
    listSeparations,
    createSamplePad: createSamplePadApi,
    updateSamplePad: updateSamplePadApi,
    removeSamplePad: removeSamplePadApi,
    previewAsset: previewAssetApi,
    stopSamplePreview,
    stopSamplePreviewKey,
    getAudioStatus,
    getMissingDependencies,
    relinkMissingDependency,
    disableMissingPlugin,
    replaceMissingTrackPlugin,
    addAudioClipToArrangement,
    openAssetInDesign: openAssetInDesignApi,
    onAudioStatus,
    onAudioMeters,
    onTrackPluginStateChanged,
    onTrackPluginParameterChanged,
    persistTrackPluginState,
    persistTrackPluginParameter,
    retryRuntimeProjection,
    retryStartupRuntime: retryStartupRuntimeApi,
    onRuntimeStartupFinished,
  } = api;
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [viewState, setViewState] = useState(defaultViewState);
  const [audio, setAudio] = useState<AudioStatus>(startingAudioStatus());
  const [plugins, setPlugins] = useState<PluginEntry[]>([]);
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
  const [renderPreviewing, setRenderPreviewing] = useState(false);
  const [previewPadId, setPreviewPadId] = useState<string | null>(null);
  const [midi, setMidi] = useState<MidiProbe>({
    inputs: [],
    outputs: [],
    refreshedAtMs: 0,
    message: 'MIDI device list has not been refreshed.',
  });
  const [deviceProbe, setDeviceProbe] = useState<AudioDeviceProbe>({
    drivers: [],
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
  const [runtimeStarted, setRuntimeStarted] = useState(false);
  const [runtimeStartupFinished, setRuntimeStartupFinished] = useState(false);
  const startupScanStarted = useRef(false);
  const startupRuntimeRecoveryAttempted = useRef(false);
  const runtimeStartupEventReceived = useRef(false);
  const bootstrapPromise = useRef<Promise<BootstrapState> | null>(null);
  const sessionRef = useRef<CreativeSession | null>(null);
  const viewStateRef = useRef<DesktopViewState>(viewState);
  const pendingPluginChanges = useRef(
    new Map<
      string,
      {
        trackId: string;
        deviceId: string;
        parameters: Map<number, number>;
        state: Parameters<typeof persistTrackPluginState>[0] | null;
      }
    >(),
  );
  const { activeJobId, backgroundJob, runBackgroundJob, cancelActiveJob } = useBackgroundJobs(api);

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
    importMidi,
  } = library;

  const sessionHook = useProject(api, { setBoot });
  const {
    session: canonicalSession,
    setSession: setSessionState,
    historyState,
    autosaveError,
    setAutosaveError,
    exportMessage,
    setExportMessage,
    undo,
    redo,
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
  } = sessionHook;
  const session = canonicalSession;
  sessionRef.current = canonicalSession;
  viewStateRef.current = viewState;
  const setSessionFromChildOperation = useCallback(
    (nextSession: CreativeSession) => {
      sessionRef.current = nextSession;
      setSessionState(nextSession);
    },
    [setSessionState],
  );
  const setSession = setSessionFromChildOperation;
  const setNavigationWorkspace = useCallback((workspace: Workspace) => {
    setViewState((current) => {
      const next = { ...current, workspace };
      viewStateRef.current = next;
      return next;
    });
  }, []);
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
  useRuntimeRestartNotification({
    api,
    setScanMessage,
  });

  const { transportPlaying, playTransport, stopTransport, goToStart } = useTransportController({
    api,
    sessionRef,
    playbackMode: viewState.workspace === 'arrange' ? 'timeline' : 'preview',
    renderResult,
    setRenderResult,
    setAudio,
    setRenderPreviewing,
  });

  const switchWorkspace = useWorkspaceNavigation({
    api,
    viewStateRef,
    setNavigationWorkspace,
    runSessionOp,
    setAutosaveError,
  });
  const openAssetInDesign = useCallback(
    async (assetId: AssetId, tool: DesignTool): Promise<void> => {
      const next = await runSessionOp(
        () => openAssetInDesignApi(assetId, tool),
        'Open asset in Design',
      );
      if (next) {
        setViewState(next);
        viewStateRef.current = next;
      }
    },
    [openAssetInDesignApi, runSessionOp],
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

  const audioHook = useAudioSettings(api, {
    audio,
    setAudio,
    session,
    setSession,
    setRecordings,
  });
  const {
    audioPreferenceMessage,
    setAudioPreferenceMessage,
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

  const applyScanReport = useCallback((report: ScanReport) => {
    setPlugins(report.plugins);
    setScanMessage(
      report.issues.length
        ? `${report.plugins.length}件 · ${report.issues.length}件の注意`
        : `${report.plugins.length}件を検出`,
    );
  }, []);

  const openRecordingAnalysis = useCallback(
    async (recording: RecordingAsset) => {
      if (!isUsableRecording(recording)) return;
      if (recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      await runBackgroundJob(
        () => startAnalysisJob(assetId),
        (result) => {
          setAnalysis(result);
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
      const result = await analyzeAsset(toAssetId(asset.id));
      if (!result) return;
      setAnalysis(result);
      await openAssetInDesign(toAssetId(asset.id), 'analyze');
    },
    [analyzeAsset, openAssetInDesign],
  );

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
    const targetAssetId = viewState.designContext.targetAssetId;
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
  }, [api]);

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
        (result) => {
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
    async (assetId: AssetId, name: string, _durationMs: number) => {
      if (!session) return;
      const next = await runSessionOp(
        () => addAudioClipToArrangement(assetId, name),
        'Add clip to timeline',
      );
      if (next) setSession(next);
    },
    [addAudioClipToArrangement, runSessionOp, session, setSession],
  );

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
  }, [setAudio, stopSamplePreview]);

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

  const replaceMissingPluginDevice = useCallback(
    async (deviceId: string, newPath: string) => {
      const next = await replaceMissingTrackPlugin(deviceId, newPath);
      setSession(next);
      setMissingDependencies(await getMissingDependencies());
    },
    [getMissingDependencies, replaceMissingTrackPlugin, setSession],
  );

  const rescanMissingPlugins = useCallback(async () => {
    const completed = await runBackgroundJob(
      () => startScanJob(boot?.vst3Root),
      applyScanReport,
      (message) => setScanMessage(`VST3 scan failed: ${message}`),
    );
    if (!completed) return;
    try {
      await retryRuntimeProjection();
    } finally {
      setMissingDependencies(await getMissingDependencies());
    }
  }, [
    applyScanReport,
    boot?.vst3Root,
    getMissingDependencies,
    runBackgroundJob,
    startScanJob,
    retryRuntimeProjection,
  ]);

  const retryStartupRuntimeAfterScan = useCallback(async () => {
    if (startupRuntimeRecoveryAttempted.current || runtimeStarted) return;
    startupRuntimeRecoveryAttempted.current = true;
    try {
      setAudio(await retryStartupRuntimeApi());
    } catch (error) {
      setScanMessage(
        `Startup runtime restore failed after the catalog scan: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }, [retryStartupRuntimeApi, runtimeStarted, setAudio]);

  useEffect(() => {
    if (
      startupScanStarted.current ||
      activeJobId.current ||
      backgroundJob != null ||
      !boot?.nativeAvailable ||
      boot.safeMode ||
      !runtimeStartupFinished
    ) {
      return;
    }
    startupScanStarted.current = true;
    void (async () => {
      const completed = await runBackgroundJob(
        () => startScanJob(boot.vst3Root),
        applyScanReport,
        (message) => setScanMessage(`VST3 scan failed: ${message}`),
      );
      if (completed) await retryStartupRuntimeAfterScan();
    })();
  }, [
    applyScanReport,
    backgroundJob,
    boot,
    retryStartupRuntimeAfterScan,
    runBackgroundJob,
    runtimeStartupFinished,
    startScanJob,
  ]);

  const ignoreMissing = useCallback((item: MissingDependency) => {
    setMissingDependencies((current) =>
      current.filter((candidate) => !(candidate.kind === item.kind && candidate.id === item.id)),
    );
  }, []);

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
    let disposed = false;
    let deferredStartupTimer: ReturnType<typeof setTimeout> | null = null;
    let unlistenRuntimeStartupFinished: (() => void) | null = null;
    const runtimeStartupListener = onRuntimeStartupFinished((event) => {
      if (disposed) return;
      runtimeStartupEventReceived.current = true;
      setRuntimeStartupFinished(true);
      setRuntimeStarted(event.succeeded);
    }).catch((error) => {
      logNativeError('onRuntimeStartupFinished')(error);
      return () => undefined;
    });
    void runtimeStartupListener.then((unlisten) => {
      if (disposed) unlisten();
      else unlistenRuntimeStartupFinished = unlisten;
    });
    const bootstrapOperation =
      bootstrapPromise.current ??
      (bootstrapPromise.current = runtimeStartupListener.then(() => bootstrap()));
    void bootstrapOperation
      .then((state) => {
        if (disposed) return;
        setBoot(state);
        setViewState(state.viewState);
        setSession(state.session);
        setPlugins(state.pluginCatalog);
        if (!runtimeStartupEventReceived.current) {
          setRuntimeStarted(state.runtimeStarted);
          setRuntimeStartupFinished(state.runtimeStartupFinished);
        }
        void getMissingDependencies()
          .then(setMissingDependencies)
          .catch(logNativeError('getMissingDependencies'));
        if (disposed) return;

        // Let the first workspace paint before starting filesystem/device
        // discovery. These jobs are useful, but none is required to render
        // the initial shell or restore the Session audio graph.
        deferredStartupTimer = setTimeout(() => {
          if (disposed) return;
          void listRecordings().then(setRecordings).catch(logNativeError('listRecordings'));
          void listSeparations().then(setSeparations).catch(logNativeError('listSeparations'));
          void probeMidiDevices().then(setMidi).catch(logNativeError('probeMidiDevices'));
          void probeAudioDevices().then(setDeviceProbe).catch(logNativeError('probeAudioDevices'));
          void enableMidi().catch(logNativeError('enableMidi'));
          void getAudioStatus().then(setAudio).catch(logNativeError('getAudioStatus'));
        }, 150);
      })
      .catch(logNativeError('bootstrap'));
    let audioStatusTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingAudioStatus: AudioStatus | null = null;
    let lastAppliedAudioStatus: AudioStatus | null = null;
    const unlistenAudio = onAudioStatus((status) => {
      publishAudioMeters({
        inputPeak: status.inputPeak,
        outputPeak: status.outputPeak,
        invalidSamples: status.invalidSamples,
        feedbackSuspected: status.feedbackSuspected,
      });
      pendingAudioStatus = status;
      if (audioStatusTimer != null) return;
      audioStatusTimer = setTimeout(() => {
        audioStatusTimer = null;
        const next = pendingAudioStatus;
        pendingAudioStatus = null;
        if (disposed || next == null) return;
        // AudioStatus frames arrive continuously from the native side even
        // when nothing meaningful changed. Reapplying an identical object
        // would re-render the entire App tree on a fixed 100 ms cadence, so
        // only propagate frames whose content actually differs.
        if (
          lastAppliedAudioStatus != null &&
          audioStatusSignature(lastAppliedAudioStatus) === audioStatusSignature(next)
        ) {
          return;
        }
        lastAppliedAudioStatus = next;
        setAudio(next);
      }, 100);
    });
    const unlistenMeters = onAudioMeters((meters: AudioMeters) => {
      publishAudioMeters(meters);
    });
    let pluginSaveTimer: ReturnType<typeof setTimeout> | null = null;
    let pluginSaveRunning = false;
    let pluginSaveFlushRequested = false;
    let pluginSaveCompletion: Promise<boolean> | null = null;
    let closeRequested = false;
    type PluginChangeBatch = [
      string,
      {
        trackId: string;
        deviceId: string;
        parameters: Map<number, number>;
        state: Parameters<typeof persistTrackPluginState>[0] | null;
      },
    ];
    const mergeFailedPluginBatch = (batch: PluginChangeBatch[]) => {
      for (const [key, failed] of batch) {
        const current = pendingPluginChanges.current.get(key);
        if (current == null) {
          pendingPluginChanges.current.set(key, {
            trackId: failed.trackId,
            deviceId: failed.deviceId,
            parameters: new Map(failed.parameters),
            state: failed.state,
          });
          continue;
        }
        if (current.state == null && failed.state != null) current.state = failed.state;
        if (current.state == null) {
          for (const [parameterIndex, value] of failed.parameters) {
            if (!current.parameters.has(parameterIndex))
              current.parameters.set(parameterIndex, value);
          }
        }
      }
    };
    const runPluginFlush = async (): Promise<boolean> => {
      let succeeded = true;
      try {
        do {
          pluginSaveFlushRequested = false;
          const batch = [...pendingPluginChanges.current.entries()] as PluginChangeBatch[];
          pendingPluginChanges.current.clear();
          if (batch.length === 0) break;
          try {
            let latest: CreativeSession | null = null;
            for (const [, pending] of batch) {
              if (pending.state != null) latest = await persistTrackPluginState(pending.state);
              for (const [parameterIndex, value] of pending.parameters) {
                latest = await persistTrackPluginParameter({
                  trackId: pending.trackId,
                  deviceId: pending.deviceId,
                  parameterIndex,
                  value,
                });
              }
            }
            if (latest != null) {
              setSession(latest);
              setAutosaveError(null);
            }
          } catch (error: unknown) {
            mergeFailedPluginBatch(batch);
            succeeded = false;
            setAutosaveError(
              error instanceof Error
                ? error.message
                : `Track Plugin state could not be saved: ${String(error)}`,
            );
            break;
          }
        } while (pluginSaveFlushRequested || pendingPluginChanges.current.size > 0);
      } finally {
        pluginSaveRunning = false;
      }
      return succeeded;
    };
    const flushPluginChanges = (): Promise<boolean> => {
      if (pluginSaveRunning) {
        pluginSaveFlushRequested = true;
        return pluginSaveCompletion ?? Promise.resolve(true);
      }
      pluginSaveRunning = true;
      const completion = runPluginFlush();
      pluginSaveCompletion = completion;
      void completion.then(
        () => {
          if (pluginSaveCompletion === completion) pluginSaveCompletion = null;
        },
        () => {
          if (pluginSaveCompletion === completion) pluginSaveCompletion = null;
        },
      );
      return completion;
    };
    const schedulePluginSave = () => {
      if (closeRequested || pluginSaveTimer != null || pluginSaveRunning) return;
      pluginSaveTimer = setTimeout(() => {
        pluginSaveTimer = null;
        void flushPluginChanges().then(() => {
          if (pendingPluginChanges.current.size > 0) schedulePluginSave();
        });
      }, 100);
    };
    const enqueuePluginParameter = (change: {
      trackId: string;
      deviceId: string;
      parameterIndex: number;
      value: number;
    }) => {
      if (closeRequested) return;
      const key = `${change.trackId}\u0000${change.deviceId}`;
      const pending = pendingPluginChanges.current.get(key) ?? {
        trackId: change.trackId,
        deviceId: change.deviceId,
        parameters: new Map<number, number>(),
        state: null,
      };
      pending.parameters.set(change.parameterIndex, change.value);
      pendingPluginChanges.current.set(key, pending);
      schedulePluginSave();
    };
    const unlistenTrackPluginParameter = onTrackPluginParameterChanged(enqueuePluginParameter);
    const unlistenTrackPluginState = onTrackPluginStateChanged((change) => {
      if (closeRequested) return;
      const key = `${change.trackId}\u0000${change.deviceId}`;
      // A full final snapshot already contains every parameter, so older
      // intermediate parameter events are intentionally coalesced away.
      pendingPluginChanges.current.set(key, {
        trackId: change.trackId,
        deviceId: change.deviceId,
        parameters: new Map(),
        state: change,
      });
      void flushPluginChanges();
    });
    let unlistenClose: (() => void) | null = null;
    let closeListenerCancelled = false;
    try {
      const currentWindow = getCurrentWindow();
      void currentWindow
        .onCloseRequested(async (event) => {
          if (closeRequested) {
            event.preventDefault();
            return;
          }
          closeRequested = true;
          // Take control of the close request while the last debounced plugin
          // batch gets one bounded flush attempt. Calling destroy() below is
          // intentional: it bypasses this close-request event and avoids
          // relying on a second close dispatch while the WebView is shutting
          // down.
          event.preventDefault();
          if (pluginSaveTimer != null) {
            clearTimeout(pluginSaveTimer);
            pluginSaveTimer = null;
          }
          let saved = false;
          try {
            saved = await Promise.race([
              flushPluginChanges(),
              new Promise<boolean>((resolve) => window.setTimeout(() => resolve(false), 3000)),
            ]);
          } catch (error) {
            console.error('[native] final Plugin State flush failed', error);
          }
          if (!saved) console.error('[native] final Plugin State flush timed out or failed');
          try {
            await currentWindow.destroy();
          } catch (error) {
            console.error('[native] window close failed', error);
            closeRequested = false;
          }
        })
        .then((unlisten) => {
          if (closeListenerCancelled) unlisten();
          else unlistenClose = unlisten;
        })
        .catch(() => undefined);
    } catch {
      // The browser test/runtime has no Tauri window; native builds register it.
    }
    return () => {
      disposed = true;
      closeListenerCancelled = true;
      if (deferredStartupTimer != null) clearTimeout(deferredStartupTimer);
      if (pluginSaveTimer != null) clearTimeout(pluginSaveTimer);
      if (audioStatusTimer != null) clearTimeout(audioStatusTimer);
      unlistenClose?.();
      if (!closeRequested) void flushPluginChanges();
      unlistenAudio();
      unlistenRuntimeStartupFinished?.();
      unlistenMeters();
      unlistenTrackPluginParameter();
      unlistenTrackPluginState();
    };
  }, []);

  useEffect(() => {
    const keyboardKeys = ['z', 's', 'x', 'd', 'c', 'v', 'g', 'b', 'h', 'n', 'j', 'm'];
    const onKey = (event: KeyboardEvent) => {
      if (isEditableTypingTarget(event.target)) return;
      const index = keyboardKeys.indexOf(event.key.toLowerCase());
      const pad = index >= 0 ? session?.playState.sampleInstrument.pads[index] : undefined;
      if (pad) {
        event.preventDefault();
        void previewSamplePad(pad);
      }
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (isEditableTypingTarget(event.target)) return;
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
  }, [previewSamplePad, session?.playState.sampleInstrument.pads, stopSamplePreviewKey]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const typing = isEditableTypingTarget(event.target);
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
      if (!typing && event.key >= '1' && Number(event.key) <= workspaces.length)
        void switchWorkspace(workspaces[Number(event.key) - 1].id);
      if (event.key === 'Escape') setCommandOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [redo, switchWorkspace, toggleMute, undo]);

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
    viewState,
    session,
    setSession: setSessionFromChildOperation,
    audio,
    setAudio,
    audioPreferenceMessage,
    setAudioPreferenceMessage,
    autosaveError,
    setAutosaveError,
    plugins,
    setPlugins,
    missingDependencies,
    relinkMissing,
    disableMissingPluginDevice,
    replaceMissingPluginDevice,
    rescanMissingPlugins,
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
    renderPreviewing,
    setRenderPreviewing,
    transportPlaying,
    recordingCommandPending,
    setRecordingCommandPending,
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
    importMidi,
    commandOpen,
    setCommandOpen,
    focusMode,
    setFocusMode,
    backgroundJob,
    cancelActiveJob,
    historyState,
    recordingCommandLock,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
    disableMidi,
    undo,
    redo,
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
    playTransport,
    stopTransport,
    goToStart,
    previewSamplePad,
    stopPreview,
    createSamplePad,
    updateSamplePad,
    removeSamplePad,
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
    query,
    visiblePlugins,
    visibleRecordings,
    usableRecordings,
    inbox,
    setScanMessage,
    api,
  };
}

function audioStatusSignature(status: AudioStatus): string {
  return JSON.stringify([
    status.state,
    status.driver,
    status.inputDevice,
    status.inputChannel,
    status.inputChannels,
    status.outputDevice,
    status.outputChannels,
    status.sampleRate,
    status.bufferSize,
    status.roundTripMs,
    status.timelineTick,
    status.recording,
    status.midiInputs,
    status.midiOutputs,
    status.midiInputActive,
    status.midiMessages,
    status.lastMidiNote,
    status.midiPadMappings,
    status.midiPadTriggers,
    // Live peaks, invalid-sample counters, and feedback state are published
    // through the dedicated audio-meters external store. Including them here
    // would make every meter frame invalidate the whole App tree again.
    status.message,
  ]);
}
