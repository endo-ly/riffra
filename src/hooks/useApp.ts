import { useCallback, useEffect, useMemo, useState } from 'react';
import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  BootstrapState,
  MidiProbe,
  PluginEntry,
  RecordingAsset,
  RenderOptions,
  RenderResult,
  Session,
  SeparationResult,
} from '@/lib/domain';
import { createTimelineClip, isUsableRecording } from '@/lib/recordings';
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
import { useSession } from './useSession';
import { useAudio } from './useAudio';

export function useApp(api: NativeApi = defaultNativeApi) {
  const {
    bootstrap,
    scanVst3Folder,
    listRecordings,
    analyzeAudio,
    probeMidiDevices,
    probeAudioDevices,
    listSeparations,
    separateChannels,
    renderTimeline,
    renderTimelineStems,
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
  const [recordings, setRecordings] = useState<RecordingAsset[]>([]);
  const [separations, setSeparations] = useState<SeparationResult[]>([]);
  const [separationBusy, setSeparationBusy] = useState<string | null>(null);
  const [separationMessage, setSeparationMessage] = useState(
    'Ready for a local stereo channel split.',
  );
  const [separationPreviewingPath, setSeparationPreviewingPath] = useState<string | null>(null);
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

  const library = useLibrary(api, { setAudio, setPreviewPadId });
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
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
  } = sessionHook;

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
                  current.rack.find((device) => device.kind === 'plugin')?.stateData ??
                  null,
              ),
            }
          : current,
      );
  }, []);

  const openRecordingAnalysis = useCallback(async (recording: RecordingAsset) => {
    if (!isUsableRecording(recording)) return;
    if (recording.error) return;
    const path = recording.processedPath ?? recording.rawPath;
    if (!path) return;
    setAnalysis(await analyzeAudio(path));
    setSession((current) => (current ? { ...current, workspace: 'analyze' } : current));
  }, []);

  const selectReference = useCallback(
    async (recording: RecordingAsset) => {
      if (recording.error) return;
      const path = recording.processedPath ?? recording.rawPath;
      if (!path) return;
      setReferenceId(recording.id);
      const existing = referenceAnalyses[recording.id];
      if (existing) return;
      const next = await analyzeAudio(path);
      if (next) setReferenceAnalyses((current) => ({ ...current, [recording.id]: next }));
    },
    [referenceAnalyses],
  );

  const previewReference = useCallback(
    async (recording: RecordingAsset) => {
      if (recording.error) return;
      const path = recording.processedPath ?? recording.rawPath;
      if (!path) return;
      await stopSamplePreview();
      setAudio(await previewSample(path, 0, 0, referenceLoopPreview));
      setReferencePreviewingId(recording.id);
      setReferenceSyncPreviewing(false);
    },
    [referenceLoopPreview],
  );

  const previewReferencePair = useCallback(async () => {
    if (!analysis || !referenceId) return;
    const reference = recordings.find((recording) => recording.id === referenceId);
    if (!reference) return;
    const referencePath = reference.processedPath ?? reference.rawPath;
    if (!referencePath) return;
    await stopSamplePreview();
    await previewSample(analysis.path, 0, 0, referenceLoopPreview);
    setAudio(await previewSample(referencePath, 0, 0, referenceLoopPreview));
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(true);
  }, [analysis, recordings, referenceId, referenceLoopPreview]);

  const stopReferencePreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(false);
  }, []);

  const runSeparation = useCallback(async (recording: RecordingAsset) => {
    if (recording.error) return;
    const path = recording.processedPath ?? recording.rawPath;
    if (!path) return;
    setSeparationBusy(recording.id);
    setSeparationMessage('Writing Left / Right WAV assets…');
    const result = await separateChannels(path);
    setSeparationBusy(null);
    if (!result) {
      setSeparationMessage('Separation failed; the source and saved session remain unchanged.');
      return;
    }
    setSeparations((current) => [result, ...current.filter((item) => item.id !== result.id)]);
    setSeparationMessage(result.message);
  }, []);

  const previewSeparation = useCallback(async (path: string) => {
    setAudio(await previewSample(path, 0, 0));
    setSeparationPreviewingPath(path);
  }, []);

  const stopSeparationPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setSeparationPreviewingPath(null);
  }, []);

  const addSeparationToTimeline = useCallback(
    async (path: string, name: string) => {
      if (!session) return;
      const metrics = await analyzeAudio(path);
      const durationMs = metrics?.durationMs ?? 1_000;
      const startMs = Math.max(
        0,
        ...session.timeline.map((clip) => clip.startMs + clip.durationMs),
      );
      setSession({
        ...session,
        timeline: [
          ...session.timeline,
          {
            id: `clip:separation:${Date.now()}`,
            assetPath: path,
            name,
            trackId: session.tracks[0]?.id ?? 'main',
            startMs,
            durationMs,
            sourceInMs: 0,
            sourceOutMs: 0,
            loopEnabled: false,
            gainDb: 0,
            fadeInMs: 0,
            fadeOutMs: 0,
            pan: 0,
            muted: false,
          },
        ],
        workspace: 'arrange',
      });
    },
    [session],
  );

  const runTimelineRender = useCallback(async (options: RenderOptions) => {
    setRenderResult(null);
    setStemResults([]);
    setRenderPreviewing(false);
    setRenderMessage('Rendering a new stereo WAV…');
    const result = await renderTimeline(options);
    if (!result) {
      setRenderMessage('Render failed; source clips and the session remain unchanged.');
      return;
    }
    setRenderResult(result);
    setRenderMessage(result.message);
  }, []);

  const runTimelineStemRender = useCallback(async (options: RenderOptions) => {
    setRenderResult(null);
    setStemResults([]);
    setRenderPreviewing(false);
    setRenderMessage('Rendering independent track stems…');
    const results = await renderTimelineStems(options);
    if (!results.length) {
      setRenderMessage('Stem render failed; source clips and the session remain unchanged.');
      return;
    }
    setStemResults(results);
    setRenderMessage(`${results.length} track stems rendered without changing source clips.`);
  }, []);

  const previewTimelineRender = useCallback(async () => {
    if (!renderResult) return;
    setAudio(await previewSample(renderResult.path, 0, 0, session?.loopEnabled ?? false));
    setRenderPreviewing(true);
    setTransportPlaying(true);
  }, [renderResult, session?.loopEnabled]);

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
    setAudio(await previewSample(result.path, 0, 0, session.loopEnabled));
    setTransportPlaying(true);
  }, [renderResult, session]);

  const stopTransport = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setTransportPlaying(false);
    setRenderPreviewing(false);
  }, []);

  const previewSamplePad = useCallback(async (pad: Session['samplePads'][number]) => {
    const nextAudio = await previewSample(
      pad.assetPath,
      pad.startMs,
      pad.endMs,
      pad.loopEnabled,
      Math.pow(10, (pad.gainDb ?? 0) / 20),
      pad.midiKey,
    );
    setAudio(nextAudio);
    setPreviewPadId(pad.id);
  }, []);

  const stopPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setPreviewPadId(null);
  }, []);

  const placeRecording = useCallback(
    (recording: RecordingAsset) => {
      if (!session) return;
      const clip = createTimelineClip(session, recording);
      if (!clip) return;
      setSession({
        ...session,
        timeline: [...session.timeline, clip],
        workspace: 'arrange',
      });
    },
    [session],
  );

  const createSamplePad = useCallback(
    (recording: RecordingAsset) => {
      if (!session || recording.error) return;
      const assetPath = recording.processedPath ?? recording.rawPath;
      if (!assetPath || session.samplePads.some((pad) => pad.assetPath === assetPath)) return;
      const index = session.samplePads.length;
      const endMs =
        recording.sampleRate && recording.samplesWritten
          ? Math.max(1, Math.round((recording.samplesWritten / recording.sampleRate) * 1000))
          : 1_000;
      setSession({
        ...session,
        samplePads: [
          ...session.samplePads,
          {
            id: `pad:${recording.id}`,
            name: recording.name,
            assetPath,
            startMs: 0,
            endMs,
            midiKey: 36 + index,
            gainDb: 0,
            loopEnabled: false,
          },
        ],
        workspace: 'sample',
      });
    },
    [session],
  );

  useEffect(() => {
    void bootstrap().then((state) => {
      setBoot(state);
      setSession(state.session);
      if (!state.safeMode) void setMasterGainDb(state.session.masterDb).then(setAudio);
      void scanVst3Folder(state.vst3Root).then((report) => {
        setPlugins(report.plugins);
        setMissingPluginPaths(
          state.session.rack
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
        const persisted = state.session.rack.find(
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
      });
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
    void configureSamplePads(session.samplePads).then(setAudio);
  }, [audio.state, boot, session?.samplePads]);

  useEffect(() => {
    const keyboardKeys = ['z', 's', 'x', 'd', 'c', 'v', 'g', 'b', 'h', 'n', 'j', 'm'];
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA') return;
      const index = keyboardKeys.indexOf(event.key.toLowerCase());
      const pad = index >= 0 ? session?.samplePads[index] : undefined;
      if (pad) {
        event.preventDefault();
        void previewSamplePad(pad);
      }
    };
    const onKeyUp = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA') return;
      const index = keyboardKeys.indexOf(event.key.toLowerCase());
      const pad = index >= 0 ? session?.samplePads[index] : undefined;
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
  }, [previewSamplePad, session?.samplePads]);

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
      if (!typing && event.key >= '1' && event.key <= '6')
        switchWorkspace(workspaces[Number(event.key) - 1].id);
      if (event.key === 'Escape') setCommandOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [redo, switchWorkspace, toggleMute, undo]);

  const persistedPlugin = session?.rack.find((device) => device.kind === 'plugin') ?? null;
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
    recordings,
    setRecordings,
    separations,
    setSeparations,
    separationBusy,
    setSeparationBusy,
    separationMessage,
    setSeparationMessage,
    separationPreviewingPath,
    setSeparationPreviewingPath,
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
    setScanMessage,
    api,
  };
}
