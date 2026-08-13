import { useCallback, useEffect, useRef, useState } from 'react';
import type { AssetId, DesignTool } from '@/model/domain';
import { isUsableRecording } from '@/shared/recordings';
import { isEditableTypingTarget } from '@/shared/input';
import { logNativeError } from '@/native/invoke';
import { defaultNativeApi } from '@/native/native';
import type { NativeApi } from '@/native/native-api';
import { workspaces } from '@/app/workspaces';
import { useAppRuntime } from '@/app/runtime/useAppRuntime';
import { useStartupRuntimeRecovery } from '@/app/runtime/useStartupRuntimeRecovery';
import { useRuntimeRestartNotification } from '@/app/runtime/useRuntimeRestartNotification';
import { useBackgroundJobs } from '@/app/runtime/useBackgroundJobs';
import { useTransportController } from '@/features/transport/hooks/useTransportController';
import { useWorkspaceNavigation } from '@/app/navigation/useWorkspaceNavigation';
import { useLibrary } from '@/features/library/hooks/useLibrary';
import { useInbox } from '@/features/library/hooks/useInbox';
import { useAudioSettings } from '@/features/audio/hooks/useAudioSettings';
import { useMissingDependencies } from '@/features/project/hooks/useMissingDependencies';
import { useRecording } from '@/features/recording/hooks/useRecording';
import { useDesign } from '@/features/design/hooks/useDesign';
import { usePluginCatalog } from '@/features/plugins/hooks/usePluginCatalog';
import { usePluginStatePersistence } from '@/features/plugins/hooks/usePluginStatePersistence';

export function useAppController(api: NativeApi = defaultNativeApi) {
  const { getAudioStatus, openAssetInDesign: openAssetInDesignApi } = api;
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
    runBackgroundJob,
  });
  const { plugins, scanPlugins } = pluginCatalog;
  useStartupRuntimeRecovery({
    boot,
    runtimeStarted,
    runtimeStartupFinished,
    activeJobId,
    backgroundJob,
    scanPlugins,
    retryStartupRuntime: api.retryStartupRuntime,
    setAudio,
  });
  const missingDependencyState = useMissingDependencies({
    api,
    boot,
    setSession,
    rescanPlugins: scanPlugins,
  });
  const {
    missingDependencies,
    clearRelocatedMissingDependencies,
    relinkMissing,
    disableMissingPluginDevice,
    replaceMissingPluginDevice,
    rescanMissingPlugins,
    ignoreMissing,
  } = missingDependencyState;
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
    deviceProbe,
    refreshAudioDevices,
    probeAudioChannels,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
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
      void refreshAudioDevices().catch(logNativeError('probeAudioDevices'));
      void enableMidi().catch(logNativeError('enableMidi'));
      void getAudioStatus().then(setAudio).catch(logNativeError('getAudioStatus'));
    }, 150);
    return () => clearTimeout(timer);
  }, [
    enableMidi,
    getAudioStatus,
    reloadSeparations,
    refreshAudioDevices,
    reloadRecordings,
    boot,
    setAudio,
  ]);

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
    deviceProbe,
    refreshAudioDevices,
    probeAudioChannels,
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
