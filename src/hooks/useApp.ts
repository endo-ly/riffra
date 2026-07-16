import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  AssetId,
  BackgroundJobStatus,
  BootstrapState,
  CreativeSession,
  MissingDependency,
  LibraryAsset,
  MidiProbe,
  PluginEntry,
  RecordingAsset,
  RenderOptions,
  RenderResult,
  ScanReport,
  SeparationResult,
} from '@/lib/domain';
import { isUsableRecording } from '@/lib/recordings';
import {
  pluginParameterValuesForSession,
  shouldRestoreIndividualParameters,
} from '@/lib/plugin-session';
import { audioCommandSucceeded } from '@/lib/audio-safety';
import {
  rackWithPluginBypassed,
  rackWithPluginLoaded,
  rackWithPluginParameter,
  rackWithoutPlugin,
} from '@/lib/rack';
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
    loadPlugin,
    clearPlugin,
    previewSample,
    stopSamplePreview,
    stopSamplePreviewKey,
    getAudioStatus,
    setPluginBypassed,
    setPluginParameter,
    setPluginState,
    setMasterGainDb,
    configureSamplePads,
    getMissingDependencies,
    relinkMissingDependency,
    disableMissingPlugin,
    resolveAssetContentLocation,
    addAudioClipToArrangement,
    saveRackDefinition,
    loadRackDefinition,
  } = api;
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [audio, setAudio] = useState<AudioStatus>({
    state: 'starting',
    driver: null,
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
    saveTimer,
    previousSession,
    historySkip,
    undo,
    redo,
    captureSnapshot,
    recallSnapshot,
    switchWorkspace,
    switchDesignTool,
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
  } = sessionHook;
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
    setSession,
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
    connectMidiInput,
    disconnectMidiInput,
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
      let nextAudio = await loadPlugin(plugin.path);
      if (!audioCommandSucceeded(nextAudio)) {
        setAudio(nextAudio);
        return;
      }
      if (stateData) nextAudio = await setPluginState(stateData);
      if (shouldRestoreIndividualParameters(stateData)) {
        for (const [index, value] of parameterValues.entries()) {
          if (index >= (nextAudio.plugin?.parameters.length ?? 0)) break;
          nextAudio = await setPluginParameter(index, value);
        }
      }
      if (bypassed) nextAudio = await setPluginBypassed(true);
      setAudio(nextAudio);
      setSession((current) =>
        current
          ? {
              ...current,
              rack: rackWithPluginLoaded(current.rack, plugin, nextAudio.plugin, {
                parameterValues,
                bypassed,
                stateData,
              }),
            }
          : current,
      );
    },
    [],
  );

  const clearPluginFromRack = useCallback(async () => {
    const nextAudio = await clearPlugin();
    setAudio(nextAudio);
    if (!audioCommandSucceeded(nextAudio)) return;
    setSession((current) =>
      current ? { ...current, rack: rackWithoutPlugin(current.rack) } : current,
    );
  }, []);

  const togglePluginBypass = useCallback(async (bypassed: boolean) => {
    const nextAudio = await setPluginBypassed(bypassed);
    setAudio(nextAudio);
    if (!audioCommandSucceeded(nextAudio)) return;
    setSession((current) =>
      current
        ? {
            ...current,
            rack: rackWithPluginBypassed(current.rack, bypassed),
          }
        : current,
    );
  }, []);

  const setPluginParameterValue = useCallback(async (index: number, value: number) => {
    const nextAudio = await setPluginParameter(index, value);
    setAudio(nextAudio);
    if (!audioCommandSucceeded(nextAudio)) return;
    const values = nextAudio.plugin
      ? pluginParameterValuesForSession(nextAudio.plugin.parameters)
      : undefined;
    if (values)
      setSession((current) =>
        current
          ? {
              ...current,
              rack: rackWithPluginParameter(
                current.rack,
                values,
                nextAudio.plugin?.stateData ??
                  current.rack.devices.find((device) => device.kind === 'plugin')?.stateData ??
                  null,
              ),
            }
          : current,
      );
  }, []);

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
          setSession((current) =>
            current
              ? {
                  ...current,
                  workspace: 'design',
                  designContext: {
                    ...current.designContext,
                    activeTool: 'analyze',
                    targetAssetId: assetId,
                  },
                }
              : current,
          );
        },
        () => setAnalysis(null),
      );
    },
    [runBackgroundJob, startAnalysisJob],
  );

  const openLibraryAssetAnalysis = useCallback(
    async (asset: LibraryAsset) => {
      if (asset.kind !== 'audio') return;
      const result = await analyzeAsset(asset.id);
      if (!result) return;
      setAnalysis(result);
      setSession((current) =>
        current
          ? {
              ...current,
              workspace: 'design',
              designContext: {
                ...current.designContext,
                activeTool: 'analyze',
                targetAssetId: asset.id,
              },
            }
          : current,
      );
    },
    [analyzeAsset],
  );

  const saveCurrentRack = useCallback(async () => {
    if (!session) return;
    const path = window.prompt('Rack definition path', 'rack-definition.json');
    if (!path?.trim()) return;
    const name = window.prompt('Rack definition name', 'Rack Definition');
    if (!name?.trim()) return;
    await saveRackDefinition(name.trim(), path.trim());
  }, [saveRackDefinition, session]);

  const loadSavedRack = useCallback(async () => {
    const path = window.prompt('Rack definition path to load', 'rack-definition.json');
    if (!path?.trim()) return;
    const rack = await loadRackDefinition(path.trim());
    if (!rack) return;
    setSession((current) => (current ? { ...current, rack } : current));
  }, [loadRackDefinition]);

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
      const path = await resolveAssetContentLocation(assetId);
      if (!path) return;
      await stopSamplePreview();
      setAudio(await previewSample(path, 0, 0, referenceLoopPreview));
      setReferencePreviewingId(recording.id);
      setReferenceSyncPreviewing(false);
    },
    [referenceLoopPreview, resolveAssetContentLocation, stopSamplePreview],
  );

  const previewReferencePair = useCallback(async () => {
    const targetAssetId = session?.designContext.targetAssetId;
    if (!analysis || !targetAssetId || !referenceId) return;
    const reference = recordings.find((recording) => recording.id === referenceId);
    if (!reference) return;
    const referenceAssetId = reference.processedAssetId ?? reference.rawAssetId;
    if (!referenceAssetId) return;
    const targetPath = await resolveAssetContentLocation(targetAssetId);
    const referencePath = await resolveAssetContentLocation(referenceAssetId);
    if (!targetPath || !referencePath) return;
    await stopSamplePreview();
    await previewSample(targetPath, 0, 0, referenceLoopPreview);
    setAudio(await previewSample(referencePath, 0, 0, referenceLoopPreview));
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(true);
  }, [
    analysis,
    previewSample,
    recordings,
    referenceId,
    referenceLoopPreview,
    resolveAssetContentLocation,
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
      setSession((current) =>
        current
          ? {
              ...current,
              workspace: 'design',
              designContext: {
                ...current.designContext,
                activeTool: 'separate',
                targetAssetId: assetId,
              },
            }
          : current,
      );
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
    [runBackgroundJob, startSeparationJob],
  );

  const previewSeparation = useCallback(
    async (assetId: AssetId) => {
      const path = await resolveAssetContentLocation(assetId);
      if (!path) return;
      setAudio(await previewSample(path, 0, 0));
      setSeparationPreviewingAssetId(assetId);
    },
    [previewSample, resolveAssetContentLocation],
  );

  const stopSeparationPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setSeparationPreviewingAssetId(null);
  }, [stopSamplePreview]);

  const addSeparationToTimeline = useCallback(
    async (assetId: AssetId, name: string, durationMs: number) => {
      if (!session) return;
      const next = await addAudioClipToArrangement(assetId, name, durationMs);
      if (next) setSession(next);
    },
    [addAudioClipToArrangement, session],
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
    setAudio(await previewSample(renderResult.path, 0, 0, session?.settings.loopEnabled ?? false));
    setRenderPreviewing(true);
    setTransportPlaying(true);
  }, [renderResult, session?.settings.loopEnabled]);

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
    setAudio(await previewSample(result.path, 0, 0, session.settings.loopEnabled));
    setTransportPlaying(true);
  }, [renderResult, session]);

  const stopTransport = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setTransportPlaying(false);
    setRenderPreviewing(false);
  }, []);

  const previewSamplePad = useCallback(
    async (pad: CreativeSession['playState']['sampleInstrument']['pads'][number]) => {
      const path = await resolveAssetContentLocation(pad.assetId);
      if (!path) return;
      const nextAudio = await previewSample(
        path,
        pad.startMs,
        pad.endMs,
        pad.loopEnabled,
        Math.pow(10, (pad.gainDb ?? 0) / 20),
        pad.midiKey,
      );
      setAudio(nextAudio);
      setPreviewPadId(pad.id);
    },
    [previewSample, resolveAssetContentLocation],
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
      const next = await addAudioClipToArrangement(assetId, recording.name, durationMs);
      if (next) setSession(next);
    },
    [addAudioClipToArrangement, session],
  );

  const createSamplePad = useCallback(
    async (recording: RecordingAsset) => {
      if (!session || recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (
        !assetId ||
        session.playState.sampleInstrument.pads.some((pad) => pad.assetId === assetId)
      )
        return;
      const index = session.playState.sampleInstrument.pads.length;
      const endMs =
        recording.sampleRate && recording.samplesWritten
          ? Math.max(1, Math.round((recording.samplesWritten / recording.sampleRate) * 1000))
          : 1_000;
      setSession({
        ...session,
        playState: {
          ...session.playState,
          sampleInstrument: {
            ...session.playState.sampleInstrument,
            pads: [
              ...session.playState.sampleInstrument.pads,
              {
                id: `pad:${recording.id}`,
                name: recording.name,
                assetId,
                startMs: 0,
                endMs,
                midiKey: 36 + index,
                gainDb: 0,
                loopEnabled: false,
              },
            ],
          },
        },
        workspace: 'design',
        designContext: { ...session.designContext, activeTool: 'sample', targetAssetId: assetId },
      });
    },
    [session],
  );

  useEffect(() => {
    void bootstrap().then((state) => {
      setBoot(state);
      setSession(state.session);
      void getMissingDependencies().then(setMissingDependencies);
      if (!state.safeMode) void setMasterGainDb(state.session.settings.masterDb).then(setAudio);
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
          const persisted = state.session.rack.devices.find(
            (device) => device.kind === 'plugin' && device.path,
          );
          const restored =
            persisted &&
            report.plugins.find(
              (plugin) => plugin.path === persisted.path && plugin.scanState === 'validated',
            );
          if (restored)
            void loadPluginIntoRack(
              restored,
              persisted?.parameterValues ?? [],
              persisted?.bypassed ?? false,
              persisted?.stateData ?? null,
            );
        },
        (message) => setScanMessage(`VST3 scan failed: ${message}`),
      );
    });
    void listRecordings().then(setRecordings);
    void listSeparations().then(setSeparations);
    void probeMidiDevices().then(setMidi);
    void probeAudioDevices().then(setDeviceProbe);
    const refreshAudio = () => void getAudioStatus().then(setAudio);
    refreshAudio();
    const audioPoll = window.setInterval(refreshAudio, 1000);
    return () => {
      window.clearInterval(audioPoll);
    };
  }, []);

  useEffect(() => {
    if (!session || !boot || boot.safeMode) return;
    void configureSamplePads(session.playState.sampleInstrument.pads).then(setAudio);
  }, [audio.state, boot, session?.playState.sampleInstrument.pads]);

  useEffect(() => {
    const keyboardKeys = ['z', 's', 'x', 'd', 'c', 'v', 'g', 'b', 'h', 'n', 'j', 'm'];
    const onKey = (event: KeyboardEvent) => {
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
  }, [previewSamplePad, session?.playState.sampleInstrument.pads]);

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
          redo();
        } else {
          undo();
        }
        return;
      }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === 'y') {
        event.preventDefault();
        redo();
        return;
      }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'm') {
        event.preventDefault();
        void toggleMute();
        return;
      }
      if (!typing && event.key >= '1' && event.key <= '4')
        switchWorkspace(workspaces[Number(event.key) - 1].id);
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
    saveTimer,
    previousSession,
    historySkip,
    recordingCommandLock,
    loadPluginIntoRack,
    clearPluginFromRack,
    togglePluginBypass,
    setPluginParameterValue,
    recoverAudio,
    selectAudioDriver,
    connectMidiInput,
    disconnectMidiInput,
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
    saveCurrentRack,
    loadSavedRack,
    switchWorkspace,
    switchDesignTool,
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
