import { invoke } from '@tauri-apps/api/core';
import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  AssetId,
  AssetPreviewOptions,
  AudioClipPatch,
  BackgroundJobStatus,
  BootstrapState,
  LibraryAsset,
  MissingDependency,
  MidiExportResult,
  MidiProbe,
  ProjectExport,
  RecordingAsset,
  RecoveryCandidate,
  RenderOptions,
  RenderResult,
  ScanReport,
  CreativeSession,
  DesignTool,
  SeparationResult,
  Workspace,
} from '@/lib/domain';
import { defaultSession } from '@/lib/domain';
import type { NativeApi } from './native-api';

const defaultVst3Root = 'C:\\Program Files\\Common Files\\VST3';

async function bootstrap(): Promise<BootstrapState> {
  try {
    return await invoke<BootstrapState>('get_bootstrap_state');
  } catch {
    return {
      session: defaultSession(),
      recoveredFromGeneration: false,
      safeMode: false,
      nativeAvailable: false,
      recoveryCandidates: [] as RecoveryCandidate[],
      dataRoot: 'Browser preview — native persistence is unavailable',
      vst3Root: defaultVst3Root,
    };
  }
}

async function saveSession(session: CreativeSession): Promise<CreativeSession> {
  return await invoke<CreativeSession>('save_scratch_session', { session });
}

async function restoreRecoveryGeneration(fileName: string): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('restore_recovery_generation', { fileName });
  } catch {
    return null;
  }
}

async function exportSession(): Promise<ProjectExport | null> {
  try {
    return await invoke<ProjectExport>('export_scratch_session');
  } catch {
    return null;
  }
}

async function importSession(path: string): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('import_scratch_session', { path });
  } catch {
    return null;
  }
}

async function scanVst3Folder(path?: string): Promise<ScanReport> {
  try {
    return await invoke<ScanReport>('scan_vst3_folder', { path: path ?? null });
  } catch {
    return {
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
    };
  }
}

async function startAnalysisJob(assetId: AssetId): Promise<BackgroundJobStatus> {
  return await invoke<BackgroundJobStatus>('start_analysis_job', { assetId });
}

async function startSeparationJob(assetId: AssetId): Promise<BackgroundJobStatus> {
  return await invoke<BackgroundJobStatus>('start_separation_job', { assetId });
}

async function startRenderJob(options: RenderOptions): Promise<BackgroundJobStatus> {
  return await invoke<BackgroundJobStatus>('start_render_job', { options });
}

async function startRenderStemsJob(options: RenderOptions): Promise<BackgroundJobStatus> {
  return await invoke<BackgroundJobStatus>('start_render_stems_job', { options });
}

async function startScanJob(path?: string): Promise<BackgroundJobStatus> {
  return await invoke<BackgroundJobStatus>('start_scan_job', { path: path ?? null });
}

async function getBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return await invoke<BackgroundJobStatus | null>('get_background_job', { id });
}

async function cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return await invoke<BackgroundJobStatus | null>('cancel_background_job', { id });
}

async function listRecordings(query?: string): Promise<RecordingAsset[]> {
  try {
    return await invoke<RecordingAsset[]>('list_recordings', { query: query ?? null });
  } catch {
    return [];
  }
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
  try {
    return await invoke<LibraryAsset[]>('search_library', { query });
  } catch {
    return [];
  }
}

async function updateLibraryAsset(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  try {
    return await invoke<LibraryAsset>('update_library_asset', { id, tag, note });
  } catch {
    return null;
  }
}

async function relatedLibraryAssets(id: string): Promise<LibraryAsset[]> {
  try {
    return await invoke<LibraryAsset[]>('related_library_assets', { id });
  } catch {
    return [];
  }
}

async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null> {
  try {
    return await invoke<AudioAnalysis>('analyze_asset', { assetId });
  } catch {
    return null;
  }
}

async function probeMidiDevices(): Promise<MidiProbe> {
  try {
    return await invoke<MidiProbe>('probe_midi_devices');
  } catch {
    return {
      inputs: [],
      outputs: [],
      refreshedAtMs: Date.now(),
      message: 'MIDI probe is unavailable in browser preview.',
    };
  }
}

async function probeAudioDevices(): Promise<AudioDeviceProbe> {
  try {
    return await invoke<AudioDeviceProbe>('probe_audio_devices');
  } catch {
    return {
      drivers: [],
      midiInputs: [],
      midiOutputs: [],
      refreshedAtMs: Date.now(),
      message: 'Audio device probe is unavailable in browser preview.',
    };
  }
}

async function listSeparations(): Promise<SeparationResult[]> {
  try {
    return await invoke<SeparationResult[]>('list_separations');
  } catch {
    return [];
  }
}

async function renderTimeline(options: RenderOptions): Promise<RenderResult | null> {
  try {
    return await invoke<RenderResult>('render_timeline', { options });
  } catch {
    return null;
  }
}

async function renderTimelineStems(options: RenderOptions): Promise<RenderResult[]> {
  try {
    return await invoke<RenderResult[]>('render_timeline_stems', { options });
  } catch {
    return [];
  }
}

async function exportMidi(): Promise<MidiExportResult | null> {
  try {
    return await invoke<MidiExportResult>('export_midi');
  } catch {
    return null;
  }
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
  name: string,
  parameterValues: number[],
  bypassed: boolean,
  stateData: string | null,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('load_plugin_into_rack', {
    path,
    name,
    parameterValues,
    bypassed,
    stateData,
  });
  return { session: result[0], audio: result[1] };
}

async function clearPluginFromRack(): Promise<{
  session: CreativeSession;
  audio: AudioStatus;
}> {
  const result = await invoke<[CreativeSession, AudioStatus]>('clear_plugin_from_rack');
  return { session: result[0], audio: result[1] };
}

async function setRackPluginBypassed(
  bypassed: boolean,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('set_rack_plugin_bypassed', {
    bypassed,
  });
  return { session: result[0], audio: result[1] };
}

async function setRackPluginParameter(
  index: number,
  value: number,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('set_rack_plugin_parameter', {
    index,
    value,
  });
  return { session: result[0], audio: result[1] };
}

async function setRackMacroValue(
  macroId: string,
  value: number,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('set_rack_macro_value', {
    macroId,
    value,
  });
  return { session: result[0], audio: result[1] };
}

async function mapRackMacro(
  macroId: string,
  parameterIndex: number | null,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('map_rack_macro', {
    macroId,
    parameterIndex,
  });
  return { session: result[0], audio: result[1] };
}

async function restoreCurrentRack(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('restore_current_rack');
  } catch (error) {
    return await audioCommandError('Restore rack', error);
  }
}

async function recallSnapshot(
  slot: 'A' | 'B',
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('recall_snapshot', { slot });
  return { session: result[0], audio: result[1] };
}

async function captureSnapshot(
  slot: 'A' | 'B',
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('capture_snapshot', { slot });
  return { session: result[0], audio: result[1] };
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
  try {
    return await invoke<AudioStatus>('get_audio_status');
  } catch {
    return {
      state: 'offline',
      driver: null,
      inputDevice: null,
      outputDevice: null,
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
      message: 'Native audio sidecar is not connected.',
    };
  }
}

async function setEmergencyMute(
  muted: boolean,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('set_emergency_mute', { muted });
  return { session: result[0], audio: result[1] };
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

async function setMasterGainDb(
  gainDb: number,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('set_master_gain_db', {
    gainDb,
  });
  return { session: result[0], audio: result[1] };
}

async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('recover_audio_device');
  } catch (error) {
    return await audioCommandError('Recover audio device', error);
  }
}

async function setAudioDriver(
  driver: string,
  inputDevice: string | null = null,
  outputDevice: string | null = null,
  sampleRate: number | null = null,
  bufferSize: number | null = null,
): Promise<AudioStatus> {
  return await invoke<AudioStatus>('set_audio_driver', {
    driver,
    inputDevice,
    outputDevice,
    sampleRate,
    bufferSize,
  });
}

async function openMidiInput(name: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('open_midi_input', { name });
  } catch (error) {
    return await audioCommandError('Open MIDI input', error);
  }
}

async function closeMidiInput(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('close_midi_input');
  } catch (error) {
    return await audioCommandError('Close MIDI input', error);
  }
}

async function createSamplePad(
  assetId: AssetId,
  name: string,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('create_sample_pad', {
    assetId,
    name,
  });
  return { session: result[0], audio: result[1] };
}

async function updateSamplePad(
  padId: string,
  patch: { startMs?: number; endMs?: number; gainDb?: number; loopEnabled?: boolean },
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('update_sample_pad', {
    padId,
    patch,
  });
  return { session: result[0], audio: result[1] };
}

async function removeSamplePad(
  padId: string,
): Promise<{ session: CreativeSession; audio: AudioStatus }> {
  const result = await invoke<[CreativeSession, AudioStatus]>('remove_sample_pad', {
    padId,
  });
  return { session: result[0], audio: result[1] };
}

async function getMissingDependencies(): Promise<MissingDependency[]> {
  try {
    return await invoke<MissingDependency[]>('get_missing_dependencies');
  } catch {
    return [];
  }
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
  try {
    return await invoke<CreativeSession>('add_audio_clip_to_arrangement', {
      assetId,
      name,
      durationMs,
      trackId: trackId ?? null,
    });
  } catch {
    return null;
  }
}

async function updateAudioClip(
  clipId: string,
  patch: AudioClipPatch,
): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('update_audio_clip', { clipId, patch });
  } catch {
    return null;
  }
}

async function moveAudioClipToTrack(
  clipId: string,
  trackId: string,
): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('move_audio_clip_to_track', { clipId, trackId });
  } catch {
    return null;
  }
}

async function setAudioClipMuted(clipId: string, muted: boolean): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('set_audio_clip_muted', { clipId, muted });
  } catch {
    return null;
  }
}

async function setAudioClipLoop(
  clipId: string,
  loopEnabled: boolean,
): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('set_audio_clip_loop', { clipId, loopEnabled });
  } catch {
    return null;
  }
}

async function duplicateAudioClip(clipId: string): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('duplicate_audio_clip', { clipId });
  } catch {
    return null;
  }
}

async function splitAudioClip(
  clipId: string,
  atOffsetMs?: number,
): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('split_audio_clip', {
      clipId,
      atOffsetMs: atOffsetMs ?? null,
    });
  } catch {
    return null;
  }
}

async function removeAudioClip(clipId: string): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('remove_audio_clip', { clipId });
  } catch {
    return null;
  }
}

async function saveRackDefinition(name: string, path: string): Promise<AssetId | null> {
  try {
    return await invoke<AssetId>('save_rack_definition', { name, path });
  } catch {
    return null;
  }
}

async function listRackDefinitions(): Promise<LibraryAsset[]> {
  try {
    return await invoke<LibraryAsset[]>('list_rack_definitions');
  } catch {
    return [];
  }
}

async function loadRackDefinitionAsset(
  assetId: AssetId,
): Promise<{ session: CreativeSession; audio: AudioStatus } | null> {
  try {
    const result = await invoke<[CreativeSession, AudioStatus]>('load_rack_definition_asset', {
      assetId,
    });
    return { session: result[0], audio: result[1] };
  } catch {
    return null;
  }
}

async function openAssetInDesign(
  assetId: AssetId,
  tool: DesignTool,
): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('open_asset_in_design', { assetId, tool });
  } catch {
    return null;
  }
}

async function switchWorkspace(workspace: Workspace): Promise<CreativeSession | null> {
  try {
    return await invoke<CreativeSession>('switch_workspace', { workspace });
  } catch {
    return null;
  }
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

async function addTrack(name: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_track', { name });
}

async function updateTrack(
  trackId: string,
  patch: { gainDb?: number; pan?: number; muted?: boolean; solo?: boolean },
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_track', { trackId, patch });
}

async function importMidiClip(assetId: AssetId, name: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('import_midi_clip', { assetId, name });
}

async function updateMidiNote(
  clipId: string,
  noteId: string,
  patch: {
    note?: number;
    startMs?: number;
    durationMs?: number;
    velocity?: number;
    channel?: number;
  },
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_midi_note', { clipId, noteId, patch });
}

async function removeMidiNote(clipId: string, noteId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_midi_note', { clipId, noteId });
}

async function removeMidiClip(clipId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_midi_clip', { clipId });
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
    startRenderJob,
    startRenderStemsJob,
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
    renderTimelineStems,
    exportMidi,
    loadPluginIntoRack,
    clearPluginFromRack,
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
    recoverAudioDevice,
    setAudioDriver,
    openMidiInput,
    closeMidiInput,
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
    addTrack,
    updateTrack,
    importMidiClip,
    updateMidiNote,
    removeMidiNote,
    removeMidiClip,
    applyAiSuggestion,
    updateAudioClip,
    moveAudioClipToTrack,
    setAudioClipMuted,
    setAudioClipLoop,
    duplicateAudioClip,
    splitAudioClip,
    removeAudioClip,
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
