import { useCallback, useEffect, useRef, useState } from 'react';
import type { AssetId, DesignTool } from '@/model/domain';
import { isUsableRecording } from '@/shared/recordings';
import { isEditableTypingTarget } from '@/features/arrange/play-surface/musical-typing';
import { logNativeError } from '@/native/invoke';
import { defaultNativeApi } from '@/native/native';
import type { NativeApi } from '@/native/native-api';
import { workspaces } from '@/app/workspaces';
import { useAppRuntime } from '@/app/runtime/useAppRuntime';
import { useRuntimeRestartNotification } from '@/app/runtime/useRuntimeRestartNotification';
import { useBackgroundJobs } from '@/app/runtime/useBackgroundJobs';
import { useTransportController } from '@/features/transport/useTransportController';
import { useWorkspaceNavigation } from '@/app/navigation/useWorkspaceNavigation';
import { useLibrary } from '@/features/library/useLibrary';
import { useInbox } from '@/features/library/useInbox';
import { useAudioSettings } from '@/features/settings/useAudioSettings';
import { useRecording } from '@/features/recording/useRecording';
import { useDesign } from '@/features/design/useDesign';
import { usePluginCatalog } from '@/features/plugins/usePluginCatalog';
import { usePluginStatePersistence } from '@/app/runtime/usePluginStatePersistence';

export function useAppController(api: NativeApi = defaultNativeApi) {
  const {
    probeMidiDevices,
    probeAudioDevices,
    stopSamplePreviewKey,
    getAudioStatus,
    openAssetInDesign: openAssetInDesignApi,
  } = api;
  const [commandOpen, setCommandOpen] = useState(false);
  const [focusMode, setFocusMode] = useState(false);
  const runtime = useAppRuntime(api);
  const { activeJobId, backgroundJob, runBackgroundJob, cancelActiveJob } = useBackgroundJobs(api);
  const {
    boot,
    viewState,
    setViewState,
    audio,
    setAudio,
    renderResult,
    setRenderResult,
    midi,
    setMidi,
    deviceProbe,
    setDeviceProbe,
    runtimeStarted,
    runtimeStartupFinished,
    sessionRef,
    viewStateRef,
    setSession,
    setNavigationWorkspace,
    session: canonicalSession,
    historyState,
    autosaveError,
    setAutosaveError,
    exportMessage,
    undo,
    redo,
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
  } = runtime;
  const session = canonicalSession;
  usePluginStatePersistence({ api, setSession, setAutosaveError });
  const pluginCatalog = usePluginCatalog({
    api,
    boot,
    runtimeStarted,
    runtimeStartupFinished,
    activeJobId,
    backgroundJob,
    runBackgroundJob,
    setAudio,
    setSession,
  });
  const {
    plugins,
    missingDependencies,
    clearRelocatedMissingDependencies,
    relinkMissing,
    disableMissingPluginDevice,
    replaceMissingPluginDevice,
    rescanMissingPlugins,
    ignoreMissing,
  } = pluginCatalog;
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
  useRuntimeRestartNotification({ api });

  const { transportPlaying, playTransport, stopTransport, goToStart } = useTransportController({
    api,
    sessionRef,
    playbackMode: viewState.workspace === 'arrange' ? 'timeline' : 'preview',
    renderResult,
    setRenderResult,
    setAudio,
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
      }
    },
    [openAssetInDesignApi, runSessionOp, setViewState],
  );
  const audioHook = useAudioSettings(api, {
    audio,
    setAudio,
  });
  const {
    audioPreferenceMessage,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
    disableMidi,
    toggleMute,
  } = audioHook;
  const recording = useRecording(api, { audio, setAudio, setSession });
  const {
    recordings,
    reloadRecordings,
    recordingCommandPending,
    startRecordingNow,
    toggleRecording,
  } = recording;

  const design = useDesign({
    api,
    recordings,
    session,
    targetAssetId: viewState.designContext.targetAssetId,
    setAudio,
    setSession,
    openAssetInDesign,
    runBackgroundJob,
    runSessionOp,
  });
  const {
    separations,
    separationBusy,
    separationMessage,
    separationPreviewingAssetId,
    previewPadId,
    setPreviewPadId,
    reloadSeparations,
    analysis,
    referenceId,
    referencePreviewingId,
    referenceSyncPreviewing,
    referenceLoopPreview,
    setReferenceLoopPreview,
    referenceAnalyses,
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
    previewSamplePad,
    stopPreview,
    createSamplePad,
    updateSamplePad,
    removeSamplePad,
  } = design;

  const library = useLibrary(api, { setAudio, setPreviewPadId });
  const {
    librarySection,
    setLibrarySection,
    libraryQuery,
    setLibraryQuery,
    libraryResults,
    selectedLibraryAsset,
    relatedAssets,
    query,
    selectLibraryAsset,
    previewSelectedLibraryAsset,
    editSelectedLibraryAsset,
    importMidi,
  } = library;

  const inbox = useInbox(api, recordings, {
    reload: async () => {
      await reloadRecordings();
    },
    onRelocate: clearRelocatedMissingDependencies,
  });

  const initialDataLoadStarted = useRef(false);
  useEffect(() => {
    if (!boot || initialDataLoadStarted.current) return;
    initialDataLoadStarted.current = true;
    const timer = setTimeout(() => {
      void reloadRecordings().catch(logNativeError('listRecordings'));
      void reloadSeparations().catch(logNativeError('listSeparations'));
      void probeMidiDevices().then(setMidi).catch(logNativeError('probeMidiDevices'));
      void probeAudioDevices().then(setDeviceProbe).catch(logNativeError('probeAudioDevices'));
      void enableMidi().catch(logNativeError('enableMidi'));
      void getAudioStatus().then(setAudio).catch(logNativeError('getAudioStatus'));
    }, 150);
    return () => clearTimeout(timer);
  }, [
    enableMidi,
    getAudioStatus,
    reloadSeparations,
    probeAudioDevices,
    probeMidiDevices,
    reloadRecordings,
    boot,
    setAudio,
    setDeviceProbe,
    setMidi,
  ]);

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
  }, [previewSamplePad, session?.playState.sampleInstrument.pads, setAudio, stopSamplePreviewKey]);

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
    viewState,
    session,
    setSession,
    audio,
    setAudio,
    audioPreferenceMessage,
    autosaveError,
    plugins,
    missingDependencies,
    relinkMissing,
    disableMissingPluginDevice,
    replaceMissingPluginDevice,
    rescanMissingPlugins,
    ignoreMissing,
    recordings,
    separations,
    separationBusy,
    separationMessage,
    separationPreviewingAssetId,
    transportPlaying,
    recordingCommandPending,
    previewPadId,
    exportMessage,
    midi,
    setMidi,
    deviceProbe,
    setDeviceProbe,
    analysis,
    referenceId,
    referencePreviewingId,
    referenceSyncPreviewing,
    referenceLoopPreview,
    setReferenceLoopPreview,
    referenceAnalyses,
    librarySection,
    setLibrarySection,
    libraryQuery,
    setLibraryQuery,
    libraryResults,
    selectedLibraryAsset,
    relatedAssets,
    importMidi,
    commandOpen,
    setCommandOpen,
    focusMode,
    setFocusMode,
    backgroundJob,
    cancelActiveJob,
    historyState,
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
    api,
  };
}
