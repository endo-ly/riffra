import { invoke } from '@tauri-apps/api/core';
import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AnalysisJobStatus,
  AssetId,
  AssetPreviewOptions,
  BackgroundJobStatus,
  BootstrapState,
  LibraryAsset,
  MissingDependency,
  MidiProbe,
  ProjectExport,
  RecordingAsset,
  RecoveryCandidate,
  RenderOptions,
  RenderResult,
  ScanJobStatus,
  ScanReport,
  SeparationJobStatus,
  CreativeSession,
  DesignTool,
  SeparationResult,
  SessionAudioPair,
  Workspace,
} from '@/lib/domain';
import { defaultSession } from '@/lib/domain';
import { offlineAudioStatus } from '@/lib/audio-defaults';
import { invokeOrFallback } from './invoke';
import type { NativeApi } from './native-api';

const defaultVst3Root = 'C:\\Program Files\\Common Files\\VST3';

async function bootstrap(): Promise<BootstrapState> {
  return invokeOrFallback<BootstrapState>(
    'get_bootstrap_state',
    {},
    {
      session: defaultSession(),
      recoveredFromGeneration: false,
      safeMode: false,
      nativeAvailable: false,
      recoveryCandidates: [] as RecoveryCandidate[],
      dataRoot: 'Browser preview \u2014 native persistence is unavailable',
      vst3Root: defaultVst3Root,
    },
  );
}

async function saveSession(session: CreativeSession): Promise<CreativeSession> {
  return await invoke<CreativeSession>('save_scratch_session', { session });
}

async function restoreRecoveryGeneration(fileName: string): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'restore_recovery_generation',
    { fileName },
    null,
  );
}

async function exportSession(): Promise<ProjectExport | null> {
  return invokeOrFallback<ProjectExport | null>('export_scratch_session', {}, null);
}

async function importSession(path: string): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('import_scratch_session', { path }, null);
}

async function scanVst3Folder(path?: string): Promise<ScanReport> {
  return invokeOrFallback<ScanReport>(
    'scan_vst3_folder',
    { path: path ?? null },
    {
      root: path ?? defaultVst3Root,
      startedAtMs: Date.now(),
      finishedAtMs: Date.now(),
      plugins: [],
      issues: [
        {
          path: path ?? defaultVst3Root,
          message: 'Native scanner is unavailable in browser preview.',
        },
      ],
    },
  );
}

async function startAnalysisJob(assetId: AssetId): Promise<AnalysisJobStatus> {
  return await invoke<AnalysisJobStatus>('start_analysis_job', { assetId });
}

async function startSeparationJob(assetId: AssetId): Promise<SeparationJobStatus> {
  return await invoke<SeparationJobStatus>('start_separation_job', { assetId });
}

async function startScanJob(path?: string): Promise<ScanJobStatus> {
  return await invoke<ScanJobStatus>('start_scan_job', { path: path ?? null });
}

async function getBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return await invoke<BackgroundJobStatus | null>('get_background_job', { id });
}

async function cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return await invoke<BackgroundJobStatus | null>('cancel_background_job', { id });
}

async function listRecordings(query?: string): Promise<RecordingAsset[]> {
  return invokeOrFallback<RecordingAsset[]>('list_recordings', { query: query ?? null }, []);
}

async function renameRecording(id: string, name: string): Promise<string> {
  return invoke<string>('rename_recording', { id, newName: name });
}

async function deleteRecording(id: string): Promise<void> {
  await invoke('delete_recording', { id });
}

async function archiveRecording(id: string): Promise<string> {
  return await invoke<string>('archive_recording', { id });
}

async function promoteRecording(id: string): Promise<string> {
  return await invoke<string>('promote_recording', { id });
}

async function tagRecording(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  return await invoke<LibraryAsset>('tag_recording', { id, tag, note });
}

async function detectDuplicateRecordings(): Promise<string[][]> {
  return await invoke<string[][]>('detect_duplicate_recordings');
}

async function searchLibrary(query: string): Promise<LibraryAsset[]> {
  if (!query.trim()) return [];
  return invokeOrFallback<LibraryAsset[]>('search_library', { query }, []);
}

async function updateLibraryAsset(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  return invokeOrFallback<LibraryAsset | null>('update_library_asset', { id, tag, note }, null);
}

async function relatedLibraryAssets(id: string): Promise<LibraryAsset[]> {
  return invokeOrFallback<LibraryAsset[]>('related_library_assets', { id }, []);
}

async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null> {
  return invokeOrFallback<AudioAnalysis | null>('analyze_asset', { assetId }, null);
}

async function probeMidiDevices(): Promise<MidiProbe> {
  return invokeOrFallback<MidiProbe>(
    'probe_midi_devices',
    {},
    {
      inputs: [],
      outputs: [],
      refreshedAtMs: Date.now(),
      message: 'MIDI probe is unavailable in browser preview.',
    },
  );
}

async function probeAudioDevices(): Promise<AudioDeviceProbe> {
  return invokeOrFallback<AudioDeviceProbe>(
    'probe_audio_devices',
    {},
    {
      drivers: [],
      midiInputs: [],
      midiOutputs: [],
      refreshedAtMs: Date.now(),
      message: 'Audio device probe is unavailable in browser preview.',
    },
  );
}

async function listSeparations(): Promise<SeparationResult[]> {
  return invokeOrFallback<SeparationResult[]>('list_separations', {}, []);
}

async function renderTimeline(options: RenderOptions): Promise<RenderResult | null> {
  return invokeOrFallback<RenderResult | null>('render_timeline', { options }, null);
}

function nativeErrorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  try {
    return JSON.stringify(error);
  } catch {
    return 'Unknown native error';
  }
}

async function audioCommandError(
  operation: string,
  error: unknown,
  safetyCritical = false,
): Promise<AudioStatus> {
  const status = await getAudioStatus();
  return {
    ...status,
    state: safetyCritical || status.state === 'offline' ? 'faulted' : status.state,
    message: `${operation} failed: ${nativeErrorText(error)}. ${safetyCritical ? 'Audio output could not be confirmed; keep emergency mute engaged.' : 'Audio state was not changed.'} Saved data is safe.`,
  };
}

async function loadPluginIntoRack(
  path: string,
  parameterValues: number[],
  bypassed: boolean,
  stateData: string | null,
): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('load_plugin_into_rack', {
    path,
    parameterValues,
    bypassed,
    stateData,
  });
}

async function clearPluginFromRack(): Promise<{
  session: CreativeSession;
  audio: AudioStatus;
}> {
  return invoke<SessionAudioPair>('clear_plugin_from_rack');
}

async function openPluginEditor(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('open_plugin_editor');
  } catch (error) {
    return await audioCommandError('Open plugin editor', error);
  }
}

async function setRackPluginBypassed(bypassed: boolean): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('set_rack_plugin_bypassed', {
    bypassed,
  });
}

async function setRackPluginParameter(index: number, value: number): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('set_rack_plugin_parameter', {
    index,
    value,
  });
}

async function setRackMacroValue(macroId: string, value: number): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('set_rack_macro_value', {
    macroId,
    value,
  });
}

async function mapRackMacro(
  macroId: string,
  parameterIndex: number | null,
): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('map_rack_macro', {
    macroId,
    parameterIndex,
  });
}

async function restoreCurrentRack(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('restore_current_rack');
  } catch (error) {
    return await audioCommandError('Restore rack', error);
  }
}

async function recallSnapshot(slot: 'A' | 'B'): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('recall_snapshot', { slot });
}

async function captureSnapshot(slot: 'A' | 'B'): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('capture_snapshot', { slot });
}

async function previewAsset(assetId: AssetId, options: AssetPreviewOptions): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('preview_asset', {
      assetId,
      options: {
        startMs: options.startMs ?? 0,
        endMs: options.endMs ?? null,
        looped: options.looped ?? false,
        gain: options.gain ?? 1,
        voiceKey: options.voiceKey ?? null,
      },
    });
  } catch (error) {
    return await audioCommandError('Preview asset', error);
  }
}

async function stopSamplePreview(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('stop_preview');
  } catch (error) {
    return await audioCommandError('Stop preview', error);
  }
}

async function stopSamplePreviewKey(voiceKey: number): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('stop_preview_for_key', { voiceKey });
  } catch (error) {
    return await audioCommandError('Stop mapped preview', error);
  }
}

async function getAudioStatus(): Promise<AudioStatus> {
  return invokeOrFallback<AudioStatus>('get_audio_status', {}, offlineAudioStatus());
}

async function setEmergencyMute(muted: boolean): Promise<AudioStatus> {
  return await invoke<AudioStatus>('set_emergency_mute', { muted });
}

async function startRecording(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('start_recording');
  } catch (error) {
    return await audioCommandError('Start recording', error);
  }
}

async function stopRecording(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('stop_recording');
  } catch (error) {
    return await audioCommandError('Stop recording', error);
  }
}

async function setMasterGainDb(gainDb: number): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('set_master_gain_db', {
    gainDb,
  });
}

async function previewMasterGainDb(gainDb: number): Promise<AudioStatus> {
  return await invoke<AudioStatus>('preview_master_gain_db', { gainDb });
}

async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('recover_audio_device');
  } catch (error) {
    return await audioCommandError('Recover audio device', error);
  }
}

async function setAudioDriver(config: AudioDriverConfig): Promise<AudioStatus> {
  return await invoke<AudioStatus>('set_audio_driver', { config });
}

async function enableMidiListening(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('enable_midi_listening');
  } catch (error) {
    return await audioCommandError('Enable MIDI listening', error);
  }
}

async function disableMidiListening(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('disable_midi_listening');
  } catch (error) {
    return await audioCommandError('Disable MIDI listening', error);
  }
}

async function sendMidiToPlugin(bytes: number[]): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('send_midi_to_plugin', { bytes });
  } catch (error) {
    return await audioCommandError('Send MIDI to plugin', error);
  }
}

async function createSamplePad(assetId: AssetId, name: string): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('create_sample_pad', {
    assetId,
    name,
  });
}

async function updateSamplePad(
  padId: string,
  patch: { startMs?: number; endMs?: number; gainDb?: number; loopEnabled?: boolean },
): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('update_sample_pad', {
    padId,
    patch,
  });
}

async function removeSamplePad(padId: string): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('remove_sample_pad', {
    padId,
  });
}

async function getMissingDependencies(): Promise<MissingDependency[]> {
  return invokeOrFallback<MissingDependency[]>('get_missing_dependencies', {}, []);
}

async function relinkMissingDependency(
  assetId: AssetId,
  newPath: string,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('relink_missing_dependency', { assetId, newPath });
}

async function disableMissingPlugin(deviceId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('disable_missing_plugin', { deviceId });
}

async function addAudioClipToArrangement(
  assetId: AssetId,
  name: string,
  durationMs: number,
  trackId?: string,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'add_audio_clip_to_arrangement',
    { assetId, name, durationMs, trackId: trackId ?? null },
    null,
  );
}

async function saveRackDefinition(name: string, path: string): Promise<AssetId | null> {
  return invokeOrFallback<AssetId | null>('save_rack_definition', { name, path }, null);
}

async function listRackDefinitions(): Promise<LibraryAsset[]> {
  return invokeOrFallback<LibraryAsset[]>('list_rack_definitions', {}, []);
}

async function loadRackDefinitionAsset(assetId: AssetId): Promise<SessionAudioPair | null> {
  return invokeOrFallback<SessionAudioPair | null>('load_rack_definition_asset', { assetId }, null);
}

async function openAssetInDesign(
  assetId: AssetId,
  tool: DesignTool,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('open_asset_in_design', { assetId, tool }, null);
}

async function switchWorkspace(workspace: Workspace): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('switch_workspace', { workspace }, null);
}

async function updateSessionSettings(patch: {
  projectName?: string | null;
  loopEnabled?: boolean;
  countInBeats?: number;
  note?: string;
  aiPermission?: string;
  aiContext?: string[];
}): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_session_settings', { patch });
}

async function applyAiSuggestion(clipId: string, proposedGainDb: number): Promise<CreativeSession> {
  return await invoke<CreativeSession>('apply_ai_suggestion', { clipId, proposedGainDb });
}

/**
 * createNativeApi returns the production NativeApi that delegates to the
 * invoke-backed helpers in this module. Behavior is identical to calling the
 * named functions directly; this wrapper exists so callers can depend on the
 * NativeApi seam and tests can substitute a FakeNativeApi.
 */
function createNativeApi(): NativeApi {
  return {
    bootstrap,
    saveSession,
    restoreRecoveryGeneration,
    exportSession,
    importSession,
    scanVst3Folder,
    startAnalysisJob,
    startSeparationJob,
    startScanJob,
    getBackgroundJob,
    cancelBackgroundJob,
    listRecordings,
    searchLibrary,
    updateLibraryAsset,
    relatedLibraryAssets,
    analyzeAsset,
    probeMidiDevices,
    probeAudioDevices,
    listSeparations,
    renderTimeline,
    loadPluginIntoRack,
    clearPluginFromRack,
    openPluginEditor,
    setRackPluginBypassed,
    setRackPluginParameter,
    setRackMacroValue,
    mapRackMacro,
    restoreCurrentRack,
    captureSnapshot,
    recallSnapshot,
    previewAsset,
    stopSamplePreview,
    stopSamplePreviewKey,
    getAudioStatus,
    setEmergencyMute,
    startRecording,
    stopRecording,
    setMasterGainDb,
    previewMasterGainDb,
    recoverAudioDevice,
    setAudioDriver,
    enableMidiListening,
    disableMidiListening,
    sendMidiToPlugin,
    createSamplePad,
    updateSamplePad,
    removeSamplePad,
    getMissingDependencies,
    relinkMissingDependency,
    disableMissingPlugin,
    addAudioClipToArrangement,
    openAssetInDesign,
    switchWorkspace,
    updateSessionSettings,
    applyAiSuggestion,
    saveRackDefinition,
    listRackDefinitions,
    loadRackDefinitionAsset,
    renameRecording,
    deleteRecording,
    archiveRecording,
    promoteRecording,
    tagRecording,
    detectDuplicateRecordings,
  };
}

/** defaultNativeApi is the shared production instance used when no api is injected. */
export const defaultNativeApi: NativeApi = createNativeApi();
