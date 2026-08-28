import { useEffect, useRef, useState } from 'react';
import { isEditableTypingTarget } from '@/shared/input';
import { getHostGeneration, logNativeError } from '@/native/invoke';
import { defaultNativeApi } from '@/native/native';
import type { NativeApi } from '@/native/native-api';
import { useAppRuntime } from '@/app/runtime/useAppRuntime';
import { useHostConnection } from '@/app/runtime/useHostConnection';
import { useStartupRuntimeRecovery } from '@/app/runtime/useStartupRuntimeRecovery';
import { useRuntimeRestartNotification } from '@/app/runtime/useRuntimeRestartNotification';
import { useRuntimeProjectionStatus } from '@/app/runtime/useRuntimeProjectionStatus';
import { useBackgroundJobs } from '@/app/runtime/useBackgroundJobs';
import { useTransportController } from '@/features/transport/hooks/useTransportController';
import { useLibrary } from '@/features/library/hooks/useLibrary';
import { useInbox } from '@/features/library/hooks/useInbox';
import { useAudioSettings } from '@/features/audio/hooks/useAudioSettings';
import { useMissingDependencies } from '@/features/project/hooks/useMissingDependencies';
import { useRecording } from '@/features/recording/hooks/useRecording';
import { usePluginCatalog } from '@/features/plugins/hooks/usePluginCatalog';
import { toast } from '@/shared/toasts';

export function useAppController(api: NativeApi = defaultNativeApi) {
  const { getAudioStatus } = api;
  const [commandOpen, setCommandOpen] = useState(false);
  const hostConnection = useHostConnection(api);
  const hostReady = hostConnection.connected && !hostConnection.switching;
  const runtime = useAppRuntime(api, hostConnection.state.generation);
  const runtimeProjection = useRuntimeProjectionStatus(api, hostConnection.state.generation);
  const { activeJobId, backgroundJob, runBackgroundJob, cancelActiveJob } = useBackgroundJobs(
    api,
    hostConnection.state.generation,
  );
  const {
    boot,
    audio,
    setAudio,
    runtimeStarted,
    runtimeStartupFinished,
    sessionRef,
    applyCanonicalState,
    session: canonicalSession,
    historyState,
    autosaveError,
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
  const pluginCatalog = usePluginCatalog({
    api,
    boot,
    runBackgroundJob,
  });
  const { plugins, scanPlugins } = pluginCatalog;
  useStartupRuntimeRecovery({
    hostGeneration: hostConnection.state.generation,
    hostReady,
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
    hostGeneration: hostConnection.state.generation,
    applyCanonicalState,
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
  useRuntimeRestartNotification({
    api,
    hostGeneration: hostConnection.state.generation,
  });

  const { transportPlaying, playTransport, stopTransport, goToStart } = useTransportController({
    api,
    sessionRef,
    hostGeneration: hostConnection.state.generation,
  });

  const audioHook = useAudioSettings(api, {
    audio,
    hostGeneration: hostConnection.state.generation,
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
  const recording = useRecording(api, {
    hostGeneration: hostConnection.state.generation,
    audio,
    setAudio,
    applyCanonicalState,
    onCommandFailure: (message) => toast(`Recording failed: ${message}`, { kind: 'error' }),
    onProjectionFailure: (message) => toast(message, { kind: 'error' }),
    onFinalizationFailure: (message) =>
      toast(`Recording files were preserved in Inbox: ${message}`, { kind: 'error' }),
  });
  const {
    recordings,
    reloadRecordings,
    recordingCommandPending,
    startRecordingNow,
    toggleRecording,
  } = recording;

  const library = useLibrary(api, {
    setAudio,
    hostGeneration: hostConnection.state.generation,
  });
  const {
    libraryQuery,
    setLibraryQuery,
    libraryResults,
    selectedLibraryAsset,
    relatedAssets,
    query,
    selectLibraryAsset,
    previewSelectedLibraryAsset,
    updateSelectedLibraryAsset,
    importMidi,
  } = library;

  const inbox = useInbox(api, recordings, {
    hostGeneration: hostConnection.state.generation,
    reload: async () => {
      await reloadRecordings();
    },
    onRelocate: clearRelocatedMissingDependencies,
  });

  const initialDataLoadStarted = useRef(false);
  useEffect(() => {
    initialDataLoadStarted.current = false;
  }, [hostConnection.state.generation]);
  useEffect(() => {
    if (!boot || !hostReady || initialDataLoadStarted.current) return;
    initialDataLoadStarted.current = true;
    const requestGeneration = hostConnection.state.generation;
    const timer = setTimeout(() => {
      void reloadRecordings().catch(logNativeError('listRecordings'));
      void refreshAudioDevices().catch(logNativeError('probeAudioDevices'));
      void enableMidi().catch(logNativeError('enableMidi'));
      void getAudioStatus()
        .then((nextAudio) => {
          if (getHostGeneration() === requestGeneration) setAudio(nextAudio);
        })
        .catch(logNativeError('getAudioStatus'));
    }, 150);
    return () => clearTimeout(timer);
  }, [
    enableMidi,
    getAudioStatus,
    hostReady,
    refreshAudioDevices,
    reloadRecordings,
    boot,
    hostConnection.state.generation,
    setAudio,
  ]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const typing = isEditableTypingTarget(event.target);
      if (event.ctrlKey && event.key.toLowerCase() === 'k') {
        if (!hostReady) return;
        event.preventDefault();
        setCommandOpen((open) => !open);
        return;
      }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === 'z') {
        if (!hostReady) return;
        event.preventDefault();
        if (event.shiftKey) {
          void redo();
        } else {
          void undo();
        }
        return;
      }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === 'y') {
        if (!hostReady) return;
        event.preventDefault();
        void redo();
        return;
      }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'm') {
        if (!hostReady) return;
        event.preventDefault();
        void toggleMute();
        return;
      }
      if (event.key === 'Escape') setCommandOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [hostReady, redo, toggleMute, undo]);

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
    hostConnectionState: hostConnection.state,
    localHosts: hostConnection.hosts,
    hostSwitching: hostConnection.switching,
    hostConnectionError: hostConnection.error,
    refreshLocalHosts: hostConnection.refresh,
    switchHost: hostConnection.switchHost,
    reconnectHost: hostConnection.reconnect,
    hostConnected: hostReady,
    boot,
    session,
    applyCanonicalState,
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
    updateSelectedLibraryAsset,
    previewSelectedLibraryAsset,
    toggleMute,
    toggleRecording,
    query,
    visiblePlugins,
    visibleRecordings,
    inbox,
    api,
    startRecordingNow,
    runtimeProjectionStatus: runtimeProjection.status,
    runtimeProjectionFailure: runtimeProjection.failure,
    retryRuntimeProjection: runtimeProjection.retry,
  };
}
