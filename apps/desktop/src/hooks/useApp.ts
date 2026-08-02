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
  JobState,
  LibraryAsset,
  MissingDependency,
  MidiProbe,
  PluginEntry,
  RecordingAsset,
  RenderResult,
  SeparationResult,
  Workspace,
} from '@/lib/domain';
import { toAssetId } from '@/lib/domain';
import { isUsableRecording } from '@/lib/recordings';
import { audioCommandSucceeded } from '@/lib/audio-safety';
import { startingAudioStatus } from '@/lib/audio-defaults';
import type { AudioMeters } from '@/lib/audio-meters';
import { publishAudioMeters } from '@/lib/audio-meters';
import { logNativeError } from '@/native/invoke';
import { defaultNativeApi } from '@/native/native';
import type { NativeApi } from '@/native/native-api';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { workspaces } from '@/constants';
import { useLibrary } from './useLibrary';
import { useInbox } from './useInbox';
import { useSession } from './useSession';
import { useAudio } from './useAudio';

const terminalJobStates: readonly JobState[] = ['completed', 'failed', 'cancelled'];

export function useApp(api: NativeApi = defaultNativeApi) {
  const {
    bootstrap,
    startAnalysisJob,
    startSeparationJob,
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
    restoreCurrentRackStrict,
    restoreSamplePadsStrict,
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
    replaceMissingTrackPlugin,
    addAudioClipToArrangement,
    openAssetInDesign: openAssetInDesignApi,
    switchWorkspace: switchWorkspaceApi,
    saveRackDefinition,
    listRackDefinitions,
    loadRackDefinitionAsset,
    sendMidiToPlugin,
    onAudioStatus,
    onAudioMeters,
    onTransportStatus,
    onRuntimeRestarted,
    onTrackPluginStateChanged,
    onTrackPluginParameterChanged,
    persistTrackPluginState,
    persistTrackPluginParameter,
    playTimeline,
    stopTimeline,
    goToStartTimeline,
    syncArrangementRuntime,
  } = api;
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [audio, setAudio] = useState<AudioStatus>(startingAudioStatus());
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
  const [renderPreviewing, setRenderPreviewing] = useState(false);
  const [transportPlaying, setTransportPlaying] = useState(false);
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
  const bootstrapPromise = useRef<Promise<BootstrapState> | null>(null);
  const sessionRef = useRef<CreativeSession | null>(null);
  const audioRef = useRef(audio);
  audioRef.current = audio;
  const workspaceSwitchPromise = useRef<Promise<void> | null>(null);
  const workspaceSwitchTarget = useRef<{
    workspace: Workspace;
    transportSequence: number;
  } | null>(null);
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
  const playRackRestorePromise = useRef<Promise<AudioStatus> | null>(null);
  const arrangeRuntimeSyncPromise = useRef<Promise<void> | null>(null);
  const runtimeReconciliationTail = useRef<Promise<void>>(Promise.resolve());
  const transportOperationPromise = useRef<Promise<void> | null>(null);
  const transportIntentVersion = useRef(0);
  const runtimeRecoveryPromise = useRef<Promise<void> | null>(null);
  const runtimeRecoveryTargetGeneration = useRef(0);
  const runtimeRecoveryCompletedGeneration = useRef(0);

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

  const sessionHook = useSession(api, { setBoot, setAudio, setMissingPluginPaths });
  const {
    session,
    setSession: setSessionState,
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
  sessionRef.current = session;
  // Arrangement/Inspector operations can spend seconds in native VST
  // construction. If the user navigates away while such an operation is in
  // flight, its older session response must not move the UI back to the
  // workspace that started the operation. Keep the newest visible workspace
  // and merge the operation's canonical data into it.
  const setSessionFromChildOperation = useCallback(
    (nextSession: CreativeSession) => {
      const current = sessionRef.current;
      // `updatedAtMs` is a strictly increasing canonical commit token. Keep
      // the visible workspace guard, but also reject an older full-session
      // response so a slow VST/rack operation cannot overwrite newer edits.
      if (current != null && nextSession.updatedAtMs < current.updatedAtMs) return;
      const guarded =
        current != null && current.workspace !== nextSession.workspace
          ? { ...nextSession, workspace: current.workspace }
          : nextSession;
      sessionRef.current = guarded;
      setSessionState(guarded);
    },
    [setSessionState],
  );
  // Every session response that crosses an async native boundary goes through
  // the same workspace guard. This includes internal App work (startup,
  // plugin-state flushes, and rack operations), not only setters passed to
  // child components. A slow Arrange/VST response must not overwrite a newer
  // optimistic Play/Home navigation.
  const setSession = setSessionFromChildOperation;
  const setNavigationSession = useCallback(
    (nextSession: CreativeSession) => {
      // Workspace navigation is view state, not an undoable production edit.
      // Mark both optimistic and canonical navigation snapshots so rapid tab
      // switching cannot retain full CreativeSession copies in undo history.
      historySkip.current = true;
      setSession(nextSession);
    },
    [historySkip, setSession],
  );
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
  const enqueueRuntimeReconciliation = useCallback(
    <T>(
      expectedWorkspace: Workspace,
      operation: () => Promise<T>,
      staleResult: () => T,
    ): Promise<T> => {
      const current = runtimeReconciliationTail.current
        .catch(() => undefined)
        .then(() => {
          // A queued VST operation may outlive the workspace that requested
          // it. Do not start stale work after a rapid navigation burst; an
          // operation already inside third-party code remains bounded by the
          // native lifecycle watchdog and cannot be safely cancelled here.
          if (sessionRef.current?.workspace !== expectedWorkspace) return staleResult();
          return operation();
        });
      runtimeReconciliationTail.current = current.then(
        () => undefined,
        () => undefined,
      );
      return current;
    },
    [],
  );
  const restorePlayRack = useCallback((): Promise<AudioStatus> => {
    const pending = playRackRestorePromise.current;
    if (pending) return pending;

    const operation = enqueueRuntimeReconciliation(
      'play',
      () => restoreCurrentRackStrict(),
      () => audioRef.current,
    )
      .then((nextAudio) => {
        setAudio(nextAudio);
        if (!audioCommandSucceeded(nextAudio)) {
          throw new Error(nextAudio.message || 'Rack restoration returned a faulted state.');
        }
        return nextAudio;
      })
      .catch((error: unknown) => {
        setScanMessage(
          `Rack restore failed: ${error instanceof Error ? error.message : String(error)}`,
        );
        throw error;
      })
      .finally(() => {
        if (playRackRestorePromise.current === operation) playRackRestorePromise.current = null;
      });
    playRackRestorePromise.current = operation;
    return operation;
  }, [enqueueRuntimeReconciliation, restoreCurrentRackStrict, setAudio]);
  const syncArrangeRuntime = useCallback((): Promise<void> => {
    const pending = arrangeRuntimeSyncPromise.current;
    if (pending) return pending;

    const operation = enqueueRuntimeReconciliation(
      'arrange',
      () => syncArrangementRuntime().then(() => undefined),
      () => undefined,
    ).finally(() => {
      if (arrangeRuntimeSyncPromise.current === operation) {
        arrangeRuntimeSyncPromise.current = null;
      }
    });
    arrangeRuntimeSyncPromise.current = operation;
    return operation;
  }, [enqueueRuntimeReconciliation, syncArrangementRuntime]);
  const recoverCurrentRuntime = useCallback(
    (generation: number): Promise<void> => {
      runtimeRecoveryTargetGeneration.current = Math.max(
        runtimeRecoveryTargetGeneration.current,
        generation,
      );
      const pending = runtimeRecoveryPromise.current;
      if (pending) return pending;
      if (boot?.safeMode) {
        runtimeRecoveryCompletedGeneration.current = Math.max(
          runtimeRecoveryCompletedGeneration.current,
          generation,
        );
        return Promise.resolve();
      }
      if (runtimeRecoveryTargetGeneration.current <= runtimeRecoveryCompletedGeneration.current) {
        return Promise.resolve();
      }

      const operation = (async () => {
        const maxRecoveryAttempts = 3;
        let attempts = 0;
        while (
          runtimeRecoveryTargetGeneration.current > runtimeRecoveryCompletedGeneration.current
        ) {
          const targetGeneration = runtimeRecoveryTargetGeneration.current;
          attempts += 1;
          if (attempts > maxRecoveryAttempts) {
            throw new Error(
              `Audio Runtime recovery exceeded ${maxRecoveryAttempts} attempts while restoring generation ${targetGeneration}.`,
            );
          }
          try {
            const nextAudio = await restoreSamplePadsStrict();
            setAudio(nextAudio);
            if (!audioCommandSucceeded(nextAudio)) {
              throw new Error(
                nextAudio.message || 'Sample Pad restoration returned a faulted state.',
              );
            }
            if (runtimeRecoveryTargetGeneration.current !== targetGeneration) continue;

            const workspace = sessionRef.current?.workspace;
            if (workspace === 'play') {
              await restorePlayRack();
            } else if (workspace === 'arrange') {
              await syncArrangeRuntime();
            }
            runtimeRecoveryCompletedGeneration.current = Math.max(
              runtimeRecoveryCompletedGeneration.current,
              targetGeneration,
            );
          } catch (error) {
            // A failed restore may itself have caused a fresh sidecar restart.
            // Let the next generation supersede this failure; a failure without
            // a newer generation remains visible to the caller.
            if (runtimeRecoveryTargetGeneration.current > targetGeneration) continue;
            throw error;
          }
        }
      })()
        .catch((error: unknown) => {
          setScanMessage(
            `Runtime recovery failed: ${error instanceof Error ? error.message : String(error)}`,
          );
          throw error;
        })
        .finally(() => {
          if (runtimeRecoveryPromise.current === operation) {
            runtimeRecoveryPromise.current = null;
          }
        });
      runtimeRecoveryPromise.current = operation;
      return operation;
    },
    [boot?.safeMode, restorePlayRack, restoreSamplePadsStrict, setAudio, syncArrangeRuntime],
  );
  const switchWorkspace = useCallback(
    async (workspace: Workspace) => {
      const transportSequence = transportIntentVersion.current + 1;
      transportIntentVersion.current = transportSequence;
      transportOperationPromise.current = null;
      const previous = sessionRef.current;
      const initialWorkspace = previous?.workspace ?? workspace;
      // Paint every navigation intent before looking at the runtime loop.
      // If an earlier native/session operation is stalled, a later click must
      // still update the visible workspace immediately.
      if (previous && previous.workspace !== workspace) {
        const optimistic = { ...previous, workspace };
        sessionRef.current = optimistic;
        setNavigationSession(optimistic);
      }
      workspaceSwitchTarget.current = { workspace, transportSequence };
      const pending = workspaceSwitchPromise.current;
      // Painting the navigation intent is synchronous. A pending runtime loop
      // may continue in the background, but callers must not await the
      // previous native/session operation before the new workspace can render.
      if (pending) return;

      // Start on the next microtask so the promise is installed before a
      // no-op target can finish synchronously; otherwise clicking the already
      // active tab could leave a resolved promise marked as permanently
      // pending and interfere with a later navigation intent.
      const operation = Promise.resolve().then(async () => {
        let lastCommittedWorkspace = initialWorkspace;
        try {
          while (workspaceSwitchTarget.current != null) {
            const targetRequest = workspaceSwitchTarget.current;
            workspaceSwitchTarget.current = null;
            const target = targetRequest.workspace;
            if (target === lastCommittedWorkspace) continue;

            const next = await runSessionOp(
              () => switchWorkspaceApi(target, targetRequest.transportSequence),
              'Workspace switch',
            );
            if (!next) {
              if (
                workspaceSwitchTarget.current == null &&
                sessionRef.current?.workspace === target
              ) {
                const current = sessionRef.current;
                if (current) {
                  const rollback = { ...current, workspace: lastCommittedWorkspace };
                  sessionRef.current = rollback;
                  setNavigationSession(rollback);
                }
              }
              continue;
            }
            lastCommittedWorkspace = target;
            if (sessionRef.current?.workspace !== target) continue;
            sessionRef.current = next;
            setNavigationSession(next);
            if (boot?.safeMode) continue;
            if (target === 'play') {
              // Rack construction may execute third-party VST code for an
              // unbounded amount of time. It is a runtime reconciliation, not
              // a prerequisite for painting the new workspace, so navigation
              // must not wait for it. A later workspace request can proceed
              // while the isolated audio process finishes the restore.
              void restorePlayRack()
                .then(async (nextAudio) => {
                  if (
                    sessionRef.current?.workspace === 'play' &&
                    audioCommandSucceeded(nextAudio) &&
                    !nextAudio.feedbackSuspected
                  ) {
                    setAudio(await setEmergencyMute(false));
                  }
                })
                .catch(logNativeError('Play rack restore'));
            } else if (target === 'arrange') {
              // Arrangement VST construction is likewise deferred. The Play
              // command performs its own synchronized runtime load, and the
              // editor can render while this background reconciliation runs.
              void syncArrangeRuntime().catch(logNativeError('Arrange runtime sync'));
            }
          }
        } catch (error) {
          setAutosaveError(
            `Workspace switch failed: ${error instanceof Error ? error.message : String(error)}`,
          );
        } finally {
          workspaceSwitchPromise.current = null;
        }
      });
      workspaceSwitchPromise.current = operation;
      // The operation is intentionally detached from the navigation event.
      // The loop coalesces rapid targets without making the React event
      // handler wait for VST/native work. Workspace persistence is deferred to
      // the next production Session edit on the Rust side.
      void operation;
    },
    [
      boot?.safeMode,
      restorePlayRack,
      runSessionOp,
      setAutosaveError,
      setAudio,
      setEmergencyMute,
      setNavigationSession,
      switchWorkspaceApi,
      syncArrangeRuntime,
    ],
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
      const nextAudio = await sendMidiToPlugin(bytes);
      // Successful keyboard notes are a high-rate realtime path. Their
      // acknowledgement must not rebuild the whole App tree or carry a full
      // plugin state payload; safety/error states still surface immediately.
      if (
        nextAudio != null &&
        (!audioCommandSucceeded(nextAudio) ||
          /failed|could not|unavailable|blocked/i.test(nextAudio.message))
      ) {
        setAudio(nextAudio);
      }
    },
    [sendMidiToPlugin, setAudio],
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
    async <J extends BackgroundJobStatus>(
      start: () => Promise<J>,
      onCompleted: (result: NonNullable<J['result']>) => void,
      onFailed: (message: string) => void,
    ): Promise<boolean> => {
      if (activeJobId.current) return false;
      let started: J;
      try {
        started = await start();
      } catch (error) {
        onFailed(error instanceof Error ? error.message : String(error));
        return false;
      }
      activeJobId.current = started.id;
      setBackgroundJob(started);
      let latest: J = started;
      try {
        while (!terminalJobStates.includes(latest.state)) {
          await new Promise((resolve) => window.setTimeout(resolve, 75));
          const next = await getBackgroundJob(started.id);
          if (!next) {
            onFailed('Background job disappeared before it reported a result.');
            return false;
          }
          // The job id encodes its kind, so the polled status is the same
          // variant as `started`. Cast once here so callers receive a typed
          // result without re-asserting at every use site.
          latest = next as J;
          setBackgroundJob(next);
        }
        if (latest.state !== 'completed') {
          onFailed(latest.message);
          return false;
        }
        if (latest.result == null) {
          onFailed('Background job completed without a result.');
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
    void listRackDefinitions()
      .then(setRackDefinitions)
      .catch(logNativeError('listRackDefinitions'));
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

  const runTransportOperation = useCallback((operation: () => Promise<void>): Promise<void> => {
    const pending = transportOperationPromise.current;
    if (pending) return pending;

    const current = operation()
      .catch((error: unknown) => {
        logNativeError('Transport operation')(error);
        setTransportPlaying(false);
      })
      .finally(() => {
        if (transportOperationPromise.current === current) {
          transportOperationPromise.current = null;
        }
      });
    transportOperationPromise.current = current;
    return current;
  }, []);
  const runImmediateTransportOperation = useCallback((operation: () => Promise<void>) => {
    return operation().catch((error: unknown) => {
      logNativeError('Immediate transport operation')(error);
      setTransportPlaying(false);
    });
  }, []);

  const playTransport = useCallback(() => {
    if (transportOperationPromise.current) return transportOperationPromise.current;
    const transportSequence = transportIntentVersion.current + 1;
    transportIntentVersion.current = transportSequence;
    const isCurrentIntent = () => transportIntentVersion.current === transportSequence;
    return runTransportOperation(async () => {
      const currentSession = sessionRef.current;
      if (!currentSession) return;
      const requestedWorkspace = currentSession.workspace;
      if (requestedWorkspace === 'arrange') {
        if (!isCurrentIntent()) return;
        await playTimeline(transportSequence);
        return;
      }
      let result = renderResult;
      if (!result) {
        result = await renderTimeline({
          range: { kind: 'entireArrangement' },
          normalize: false,
          trackId: null,
        });
        if (!result || !isCurrentIntent()) return;
        setRenderResult(result);
      }
      if (!isCurrentIntent()) return;
      const nextAudio = await previewAssetApi(result.assetId, {
        looped: currentSession.settings.loopEnabled,
      });
      if (!isCurrentIntent()) {
        await stopSamplePreview();
        return;
      }
      if (sessionRef.current?.workspace !== requestedWorkspace) return;
      setAudio(nextAudio);
      setTransportPlaying(true);
    });
  }, [
    playTimeline,
    previewAssetApi,
    renderResult,
    renderTimeline,
    runTransportOperation,
    stopSamplePreview,
  ]);

  const stopTransport = useCallback(() => {
    const transportSequence = transportIntentVersion.current + 1;
    transportIntentVersion.current = transportSequence;
    // Stop is an explicit cancellation boundary. Detach the old Play
    // promise so a subsequent Play can be submitted immediately; its finally
    // handler cannot clear a newer promise because it checks identity.
    transportOperationPromise.current = null;
    setTransportPlaying(false);
    return runImmediateTransportOperation(async () => {
      if (sessionRef.current?.workspace === 'arrange') {
        await stopTimeline(transportSequence);
        return;
      }
      setAudio(await stopSamplePreview());
      setRenderPreviewing(false);
    });
  }, [runImmediateTransportOperation, setAudio, stopSamplePreview, stopTimeline]);

  const goToStart = useCallback(() => {
    const transportSequence = transportIntentVersion.current + 1;
    transportIntentVersion.current = transportSequence;
    transportOperationPromise.current = null;
    setTransportPlaying(false);
    return runImmediateTransportOperation(async () => {
      if (sessionRef.current?.workspace === 'arrange') {
        await goToStartTimeline(transportSequence);
        return;
      }
      setAudio(await stopSamplePreview());
      setRenderPreviewing(false);
    });
  }, [goToStartTimeline, runImmediateTransportOperation, setAudio, stopSamplePreview]);

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
      (report) => {
        setPlugins(report.plugins);
        setScanMessage(
          report.issues.length
            ? `${report.plugins.length}件 · ${report.issues.length}件の注意`
            : `${report.plugins.length}件を検出`,
        );
      },
      (message) => setScanMessage(`VST3 scan failed: ${message}`),
    );
    if (!completed) return;
    try {
      await syncArrangeRuntime();
    } finally {
      setMissingDependencies(await getMissingDependencies());
    }
  }, [boot?.vst3Root, getMissingDependencies, runBackgroundJob, startScanJob, syncArrangeRuntime]);

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
    const bootstrapOperation = bootstrapPromise.current ?? (bootstrapPromise.current = bootstrap());
    void bootstrapOperation
      .then((state) => {
        if (disposed) return;
        setBoot(state);
        setSession(state.session);
        void getMissingDependencies()
          .then(setMissingDependencies)
          .catch(logNativeError('getMissingDependencies'));
        void (async () => {
          let startupAudio: AudioStatus | null = null;
          let runtimeReady = state.safeMode;
          if (!state.safeMode) {
            try {
              const result = await setMasterGainDb(state.session.settings.masterDb);
              startupAudio = result.audio;
              setAudio(result.audio);
              setSession(result.session);
              startupAudio = await restoreSamplePadsStrict();
              setAudio(startupAudio);
              if (!audioCommandSucceeded(startupAudio)) {
                throw new Error(
                  startupAudio.message || 'Sample Pad restoration returned a faulted state.',
                );
              }
              if (state.session.workspace === 'play') {
                startupAudio = await restorePlayRack();
              }
              runtimeReady = true;
            } catch (error) {
              setScanMessage(
                `Startup audio initialization failed: ${error instanceof Error ? error.message : String(error)}`,
              );
            }
            if (
              runtimeReady &&
              startupAudio &&
              audioCommandSucceeded(startupAudio) &&
              !startupAudio.feedbackSuspected
            ) {
              setAudio(await setEmergencyMute(false));
            }
          }
          if (disposed) return;

          // Let the first workspace paint before starting filesystem/device
          // discovery. These jobs are useful, but none is required to render
          // the initial shell and they can otherwise compete with VST setup.
          deferredStartupTimer = setTimeout(() => {
            if (disposed) return;
            void listRecordings().then(setRecordings).catch(logNativeError('listRecordings'));
            void listSeparations().then(setSeparations).catch(logNativeError('listSeparations'));
            void probeMidiDevices().then(setMidi).catch(logNativeError('probeMidiDevices'));
            void probeAudioDevices()
              .then(setDeviceProbe)
              .catch(logNativeError('probeAudioDevices'));
            void enableMidi().catch(logNativeError('enableMidi'));
            void getAudioStatus().then(setAudio).catch(logNativeError('getAudioStatus'));
            void runBackgroundJob(
              () => startScanJob(state.vst3Root),
              (report) => {
                setPlugins(report.plugins);
                setMissingPluginPaths(
                  state.session.rack.devices
                    .filter((device) => device.kind === 'plugin' && device.path)
                    .filter(
                      (device) =>
                        !report.plugins.some(
                          (plugin) =>
                            plugin.path === device.path && plugin.scanState === 'validated',
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
          }, 150);
        })().catch(logNativeError('startup runtime initialization'));
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
    const unlistenTransport = onTransportStatus((status) =>
      setTransportPlaying(status.state === 'playing'),
    );
    const unlistenRuntimeRestarted = onRuntimeRestarted((generation) => {
      if (disposed) return;
      void recoverCurrentRuntime(generation).catch(logNativeError('Runtime recovery'));
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
      unlistenMeters();
      unlistenTransport();
      unlistenRuntimeRestarted();
      unlistenTrackPluginParameter();
      unlistenTrackPluginState();
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
    setSession: setSessionFromChildOperation,
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
    setTransportPlaying,
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
    playTransport,
    stopTransport,
    goToStart,
    previewSamplePad,
    stopPreview,
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
    // PluginStateSummary: the full plugin status includes a binary stateData
    // blob and the parameter list; stringifying those on every status frame
    // would cost more than the re-render this signature is meant to avoid.
    status.plugin == null
      ? null
      : [
          status.plugin.loaded,
          status.plugin.bypassed,
          status.plugin.path,
          status.plugin.name,
          status.plugin.sampleRate,
          status.plugin.blockSize,
          status.plugin.inputChannels,
          status.plugin.outputChannels,
          status.plugin.bypassedBlocks,
          status.plugin.processedBlocks,
          status.plugin.contentionBlocks,
          status.plugin.transitionBlocks,
        ],
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
