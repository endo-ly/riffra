import { useEffect, useRef, useState } from 'react';
import { isEditableTypingTarget } from '@/shared/input';
import { logNativeError } from '@/native/invoke';
import { defaultNativeApi } from '@/native/native';
import type { NativeApi } from '@/native/native-api';
import { useAppRuntime } from '@/app/runtime/useAppRuntime';
import { useStartupRuntimeRecovery } from '@/app/runtime/useStartupRuntimeRecovery';
import { useRuntimeRestartNotification } from '@/app/runtime/useRuntimeRestartNotification';
import { useBackgroundJobs } from '@/app/runtime/useBackgroundJobs';
import { useTransportController } from '@/features/transport/hooks/useTransportController';
import { useLibrary } from '@/features/library/hooks/useLibrary';
import { useInbox } from '@/features/library/hooks/useInbox';
import { useAudioSettings } from '@/features/audio/hooks/useAudioSettings';
import { useMissingDependencies } from '@/features/project/hooks/useMissingDependencies';
import { useRecording } from '@/features/recording/hooks/useRecording';
import { usePluginCatalog } from '@/features/plugins/hooks/usePluginCatalog';
import { usePluginStatePersistence } from '@/features/plugins/hooks/usePluginStatePersistence';

export function useAppController(api: NativeApi = defaultNativeApi) {
  const { getAudioStatus } = api;
  const [commandOpen, setCommandOpen] = useState(false);
  const runtime = useAppRuntime(api);
  const { activeJobId, backgroundJob, runBackgroundJob, cancelActiveJob } = useBackgroundJobs(api);
  const {
    boot,
    audio,
    setAudio,
    runtimeStarted,
    runtimeStartupFinished,
    sessionRef,
    setSession,
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
  useRuntimeRestartNotification({ api });

  const { transportPlaying, playTransport, stopTransport, goToStart } = useTransportController({
    api,
    sessionRef,
  });

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

  const library = useLibrary(api, { setAudio });
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
      void refreshAudioDevices().catch(logNativeError('probeAudioDevices'));
      void enableMidi().catch(logNativeError('enableMidi'));
      void getAudioStatus().then(setAudio).catch(logNativeError('getAudioStatus'));
    }, 150);
    return () => clearTimeout(timer);
  }, [enableMidi, getAudioStatus, refreshAudioDevices, reloadRecordings, boot, setAudio]);

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
      if (event.key === 'Escape') setCommandOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [redo, toggleMute, undo]);

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
  return {
    boot,
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
    transportPlaying,
    recordingCommandPending,
    exportMessage,
    deviceProbe,
    refreshAudioDevices,
    probeAudioChannels,
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
    backgroundJob,
    cancelActiveJob,
    historyState,
    recoverAudio,
    selectAudioDriver,
    undo,
    redo,
    playTransport,
    stopTransport,
    goToStart,
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
    selectLibraryAsset,
    editSelectedLibraryAsset,
    previewSelectedLibraryAsset,
    toggleMute,
    toggleRecording,
    query,
    visiblePlugins,
    visibleRecordings,
    inbox,
    api,
    startRecordingNow,
  };
}
