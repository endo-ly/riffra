import { listen } from '@tauri-apps/api/event';
import type {
  AudioClipMove,
  AudioAnalysis,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AnalysisJobStatus,
  AssetId,
  AssetPreviewOptions,
  BackgroundJobStatus,
  BootstrapState,
  HistoryState,
  DeviceChannels,
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
  DesktopViewState,
  ProjectTimebase,
  RuntimeProjectionStatus,
  DesignTool,
  SeparationResult,
  SessionAudioPair,
  MonitoringState,
  RackInstance,
  TrackKind,
  Workspace,
  TransportStatus,
  AudioClipPatch,
  AudioTakeVariant,
  AutomationParameter,
  AutomationPoint,
  MidiClipMove,
  MidiClipPatch,
  MidiInputRoute,
} from '@/lib/domain';
import { defaultSession, defaultViewState } from '@/lib/domain';
import { offlineAudioStatus } from '@/lib/audio-defaults';
import { invoke, invokeLatest, invokeOrFallback, isNativeRuntime } from './invoke';
import type {
  NativeApi,
  RuntimeStartupFinishedEvent,
  TrackPluginParameterChange,
  TrackPluginStateChange,
} from './native-api';
import type { AudioMeters } from '@/lib/audio-meters';

const defaultVst3Root = 'C:\\Program Files\\Common Files\\VST3';

async function bootstrap(): Promise<BootstrapState> {
  return invokeOrFallback<BootstrapState>(
    'get_bootstrap_state',
    {},
    {
      session: defaultSession(),
      viewState: defaultViewState(),
      pluginCatalog: [],
      runtimeStarted: false,
      runtimeStartupFinished: false,
      recoveredFromGeneration: false,
      safeMode: false,
      nativeAvailable: false,
      recoveryCandidates: [] as RecoveryCandidate[],
      dataRoot: 'Browser preview \u2014 native persistence is unavailable',
      vst3Root: defaultVst3Root,
    },
  );
}

async function onRuntimeStartupFinished(
  callback: (event: RuntimeStartupFinishedEvent) => void,
): Promise<() => void> {
  if (!isNativeRuntime()) return () => undefined;
  return listen<RuntimeStartupFinishedEvent>('runtime-startup-finished', ({ payload }) => {
    callback(payload);
  });
}

async function undoSession(): Promise<CreativeSession> {
  return invokeOrFallback<CreativeSession>('undo_session', {}, defaultSession());
}

async function redoSession(): Promise<CreativeSession> {
  return invokeOrFallback<CreativeSession>('redo_session', {}, defaultSession());
}

async function getHistoryState(): Promise<HistoryState> {
  return invokeOrFallback<HistoryState>(
    'get_history_state',
    {},
    {
      canUndo: false,
      canRedo: false,
    },
  );
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

async function importMidiFile(path: string, name?: string): Promise<AssetId | null> {
  return invokeOrFallback<AssetId | null>('import_midi_file', { path, name: name ?? null }, null);
}

async function importMidiBytes(name: string, bytes: number[]): Promise<AssetId | null> {
  return invokeOrFallback<AssetId | null>('import_midi_bytes', { name, bytes }, null);
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
      refreshedAtMs: Date.now(),
      message: 'Audio device probe is unavailable in browser preview.',
    },
  );
}

async function probeDeviceChannels(
  driver: string,
  inputDevice: string,
  outputDevice: string,
): Promise<DeviceChannels> {
  return invokeOrFallback<DeviceChannels>(
    'probe_device_channels',
    { driver, inputDevice, outputDevice },
    {
      driver,
      inputDevice,
      inputChannels: [],
      outputDevice,
      outputChannels: [],
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

async function getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus> {
  return invokeOrFallback<RuntimeProjectionStatus>(
    'get_runtime_projection_status',
    {},
    {
      state: 'idle',
      operationId: 0,
      runningOperationId: null,
      targetProjectionSequence: null,
      targetSessionRevision: null,
      preparedSessionRevision: null,
      activeProjectionSequence: null,
      activeSessionRevision: null,
      runtimeGeneration: 0,
      queuedAtMs: null,
      startedAtMs: null,
      completedAtMs: null,
      lastNativeResponseAtMs: null,
      discardedPreparationCount: 0,
      lastError: null,
    },
  );
}

async function setEmergencyMute(muted: boolean): Promise<AudioStatus> {
  return await invoke<AudioStatus>('set_emergency_mute', { muted });
}

async function startArrangeRecording(recordingSessionId?: string): Promise<AudioStatus> {
  return await invoke<AudioStatus>('start_arrange_recording', {
    recordingSessionId: recordingSessionId ?? null,
  });
}

async function recordAnotherTake(recordingSessionId: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('record_another_take', { recordingSessionId });
  } catch (error) {
    return await audioCommandError('Start another take', error);
  }
}

async function stopArrangeRecording(): Promise<AudioStatus> {
  return await invoke<AudioStatus>('stop_arrange_recording');
}

async function setMasterGainDb(gainDb: number): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('set_master_gain_db', {
    gainDb,
  });
}

async function previewMasterGainDb(gainDb: number): Promise<void> {
  await invoke<void>('preview_master_gain_db', { gainDb });
}

async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('recover_audio_device');
  } catch (error) {
    return await audioCommandError('Recover audio device', error);
  }
}

async function retryStartupRuntime(): Promise<AudioStatus> {
  return await invoke<AudioStatus>('retry_startup_runtime');
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

async function sendMidiToTrack(trackId: string, bytes: number[]): Promise<AudioStatus | null> {
  try {
    await invoke<void>('send_midi_to_track', { trackId, bytes });
    return null;
  } catch (error) {
    return await audioCommandError('Send MIDI to Track', error);
  }
}

async function panicMidiTrack(trackId: string): Promise<AudioStatus | null> {
  try {
    await invoke<void>('panic_midi_track', { trackId });
    return null;
  } catch (error) {
    return await audioCommandError('Panic MIDI Track', error);
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

async function replaceMissingTrackPlugin(
  deviceId: string,
  newPath: string,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('replace_missing_track_plugin', { deviceId, newPath });
}

async function addAudioClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'add_audio_clip_to_arrangement',
    { assetId, name, startTick: startTick ?? null, trackId: trackId ?? null },
    null,
  );
}

async function addMidiClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'add_midi_clip_to_arrangement',
    { assetId, name, startTick: startTick ?? null, trackId: trackId ?? null },
    null,
  );
}

async function updateAudioClip(
  clipId: string,
  patch: AudioClipPatch,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('update_audio_clip', { clipId, patch }, null);
}

async function updateMidiClip(
  clipId: string,
  patch: MidiClipPatch,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('update_midi_clip', { clipId, patch }, null);
}

async function removeTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'remove_timeline_clips',
    { audioClipIds, midiClipIds },
    null,
  );
}

async function trimAudioClip(
  clipId: string,
  startTick: number,
  sourceRange: { start: number; end: number },
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'trim_audio_clip',
    { clipId, startTick, sourceRange },
    null,
  );
}

async function splitAudioClip(clipId: string, splitTick: number): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('split_audio_clip', { clipId, splitTick }, null);
}

async function duplicateAudioClip(clipId: string): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('duplicate_audio_clip', { clipId }, null);
}

async function moveAudioClips(moves: AudioClipMove[]): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('move_audio_clips', { moves }, null);
}

async function moveMidiClips(moves: MidiClipMove[]): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('move_midi_clips', { moves }, null);
}

async function trimMidiClip(
  clipId: string,
  startTick: number,
  durationTicks: number,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'trim_midi_clip',
    { clipId, startTick, durationTicks },
    null,
  );
}

async function splitMidiClip(clipId: string, splitTick: number): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('split_midi_clip', { clipId, splitTick }, null);
}

async function duplicateMidiClip(clipId: string): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('duplicate_midi_clip', { clipId }, null);
}

async function pasteTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
  startTick: number,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'paste_timeline_clips',
    { audioClipIds, midiClipIds, startTick },
    null,
  );
}

async function crossfadeAudioClips(
  firstId: string,
  secondId: string,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'crossfade_audio_clips',
    { firstId, secondId },
    null,
  );
}

async function addTrack(name: string, kind: TrackKind): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_track', { name, kind });
}

async function updateTrack(
  trackId: string,
  patch: {
    name?: string;
    gainDb?: number;
    pan?: number;
    muted?: boolean;
    solo?: boolean;
    armed?: boolean;
    monitoring?: MonitoringState;
    rack?: RackInstance;
  },
): Promise<CreativeSession> {
  const fields = Object.keys(patch);
  const latestField =
    fields.length === 1 && ['muted', 'solo', 'armed', 'monitoring'].includes(fields[0] ?? '')
      ? fields[0]
      : null;
  if (latestField) {
    return await invokeLatest<CreativeSession>(
      'update_track',
      { trackId, patch },
      `update_track:${trackId}:${latestField}`,
    );
  }
  return await invoke<CreativeSession>('update_track', { trackId, patch });
}

async function setTrackAutomation(
  trackId: string,
  parameter: AutomationParameter,
  points: AutomationPoint[],
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_automation', { trackId, parameter, points });
}

async function setTrackAudioInput(
  trackId: string,
  channelIndex: number | null,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_audio_input', { trackId, channelIndex });
}

async function setTrackMidiInput(trackId: string, route: MidiInputRoute): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_midi_input', { trackId, route });
}

async function setTrackInstrument(trackId: string, pluginPath: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_instrument', { trackId, pluginPath });
}

async function clearTrackInstrument(trackId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('clear_track_instrument', { trackId });
}

async function addTrackEffect(trackId: string, pluginPath: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_track_effect', { trackId, pluginPath });
}

async function removeTrackEffect(trackId: string, deviceId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_track_effect', { trackId, deviceId });
}

async function reorderTrackEffects(
  trackId: string,
  orderedDeviceIds: string[],
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('reorder_track_effects', { trackId, orderedDeviceIds });
}

async function setTrackDeviceBypassed(
  trackId: string,
  deviceId: string,
  bypassed: boolean,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_device_bypassed', {
    trackId,
    deviceId,
    bypassed,
  });
}

async function setTrackDeviceParameter(
  trackId: string,
  deviceId: string,
  parameterIndex: number,
  value: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_device_parameter', {
    trackId,
    deviceId,
    parameterIndex,
    value,
  });
}

async function openTrackPluginEditor(trackId: string, deviceId: string): Promise<void> {
  await invoke<void>('open_track_plugin_editor', { trackId, deviceId });
}

async function persistTrackPluginState(change: TrackPluginStateChange): Promise<CreativeSession> {
  return await invoke<CreativeSession>('persist_track_plugin_state', {
    trackId: change.trackId,
    deviceId: change.deviceId,
    parameterValues: change.parameterValues,
    stateData: change.stateData ?? null,
    bypassed: change.bypassed,
  });
}

async function persistTrackPluginParameter(
  change: TrackPluginParameterChange,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('persist_track_plugin_parameter', {
    trackId: change.trackId,
    deviceId: change.deviceId,
    parameterIndex: change.parameterIndex,
    value: change.value,
  });
}

async function removeTrack(trackId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_track', { trackId });
}

async function duplicateTrack(trackId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('duplicate_track', { trackId });
}

async function reorderTrack(trackId: string, targetIndex: number): Promise<CreativeSession> {
  return await invoke<CreativeSession>('reorder_track', { trackId, targetIndex });
}

async function addMarker(tick: number, name: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_marker', { tick, name });
}

async function updateMarker(
  markerId: string,
  patch: { name?: string; tick?: number },
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_marker', { markerId, ...patch });
}

async function removeMarker(markerId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_marker', { markerId });
}

async function addMidiNote(
  clipId: string,
  startTick: number,
  pitch: number,
  durationTicks: number,
  velocity: number,
  channel: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_midi_note', {
    clipId,
    startTick,
    pitch,
    durationTicks,
    velocity,
    channel,
  });
}

async function updateMidiNote(
  clipId: string,
  noteId: string,
  patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number },
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_midi_note', { clipId, noteId, patch });
}

async function updateMidiNotes(
  clipId: string,
  updates: {
    noteId: string;
    patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number };
  }[],
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_midi_notes', { clipId, updates });
}

async function removeMidiNote(clipId: string, noteId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_midi_note', { clipId, noteId });
}

async function quantizeMidiNotes(
  clipId: string,
  noteIds: string[],
  gridTicks: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('quantize_midi_notes', { clipId, noteIds, gridTicks });
}

async function duplicateMidiNotes(
  clipId: string,
  noteIds: string[],
  offsetTicks: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('duplicate_midi_notes', {
    clipId,
    noteIds,
    offsetTicks,
  });
}

async function setAudioClipTakeVariant(
  clipId: string,
  variant: AudioTakeVariant,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_audio_clip_take_variant', { clipId, variant });
}

async function startTakeComparison(takeId: string): Promise<AudioStatus> {
  return await invoke<AudioStatus>('start_take_comparison', { takeId });
}

async function switchTakeComparisonVariant(variant: AudioTakeVariant): Promise<AudioStatus> {
  return await invoke<AudioStatus>('switch_take_comparison_variant', { variant });
}

async function stopTakeComparison(): Promise<AudioStatus> {
  return await invoke<AudioStatus>('stop_take_comparison');
}

async function activateTake(sessionId: string, takeId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('activate_take', { sessionId, takeId });
}

async function placeTakeAsSeparateClip(takeId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('place_take_as_separate_clip', { takeId });
}

async function retryRuntimeProjection(): Promise<RuntimeProjectionStatus> {
  return await invoke<RuntimeProjectionStatus>('retry_runtime_projection');
}

async function playTimeline(transportSequence: number): Promise<void> {
  await invoke<void>('play_timeline', { transportSequence });
}

async function stopTimeline(transportSequence: number): Promise<void> {
  await invoke<void>('stop_timeline', { transportSequence });
}

async function goToStartTimeline(transportSequence: number): Promise<void> {
  await invoke<void>('go_to_start_timeline', { transportSequence });
}

async function seekTimeline(tick: number): Promise<void> {
  await invoke<void>('seek_timeline', { tick });
}

async function updateArrangementTimebase(timebase: ProjectTimebase): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_arrangement_timebase', { timebase });
}

async function updateTimelineLoopRange(
  enabled: boolean,
  startTick: number,
  endTick: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_timeline_loop_range', {
    enabled,
    startTick,
    endTick,
  });
}

async function updateTimelinePunchRange(
  enabled: boolean,
  startTick: number,
  endTick: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_timeline_punch_range', {
    enabled,
    startTick,
    endTick,
  });
}

async function openAssetInDesign(
  assetId: AssetId,
  tool: DesignTool,
): Promise<DesktopViewState | null> {
  return invokeOrFallback<DesktopViewState | null>('open_asset_in_design', { assetId, tool }, null);
}

async function switchWorkspace(
  workspace: Workspace,
  transportSequence: number,
): Promise<DesktopViewState | null> {
  return invokeOrFallback<DesktopViewState | null>(
    'switch_workspace',
    { workspace, transportSequence },
    null,
  );
}

async function updateSessionSettings(patch: {
  projectName?: string | null;
  loopEnabled?: boolean;
  countInBeats?: number;
  metronomeEnabled?: boolean;
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
    onRuntimeStartupFinished,
    undoSession,
    redoSession,
    getHistoryState,
    restoreRecoveryGeneration,
    exportSession,
    importSession,
    importMidiFile,
    importMidiBytes,
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
    probeDeviceChannels,
    listSeparations,
    renderTimeline,
    previewAsset,
    stopSamplePreview,
    stopSamplePreviewKey,
    getAudioStatus,
    getRuntimeProjectionStatus,
    setEmergencyMute,
    startArrangeRecording,
    recordAnotherTake,
    stopArrangeRecording,
    setMasterGainDb,
    previewMasterGainDb,
    recoverAudioDevice,
    retryStartupRuntime,
    setAudioDriver,
    enableMidiListening,
    disableMidiListening,
    sendMidiToTrack,
    panicMidiTrack,
    createSamplePad,
    updateSamplePad,
    removeSamplePad,
    getMissingDependencies,
    relinkMissingDependency,
    disableMissingPlugin,
    replaceMissingTrackPlugin,
    addAudioClipToArrangement,
    addMidiClipToArrangement,
    updateAudioClip,
    updateMidiClip,
    removeTimelineClips,
    trimAudioClip,
    splitAudioClip,
    duplicateAudioClip,
    moveAudioClips,
    moveMidiClips,
    trimMidiClip,
    splitMidiClip,
    duplicateMidiClip,
    pasteTimelineClips,
    crossfadeAudioClips,
    addTrack,
    updateTrack,
    setTrackAutomation,
    setTrackAudioInput,
    setTrackMidiInput,
    setTrackInstrument,
    clearTrackInstrument,
    addTrackEffect,
    removeTrackEffect,
    reorderTrackEffects,
    setTrackDeviceBypassed,
    setTrackDeviceParameter,
    openTrackPluginEditor,
    persistTrackPluginState,
    persistTrackPluginParameter,
    removeTrack,
    duplicateTrack,
    reorderTrack,
    addMarker,
    updateMarker,
    removeMarker,
    addMidiNote,
    updateMidiNote,
    updateMidiNotes,
    removeMidiNote,
    quantizeMidiNotes,
    duplicateMidiNotes,
    setAudioClipTakeVariant,
    startTakeComparison,
    switchTakeComparisonVariant,
    stopTakeComparison,
    activateTake,
    placeTakeAsSeparateClip,
    retryRuntimeProjection,
    playTimeline,
    stopTimeline,
    goToStartTimeline,
    seekTimeline,
    updateArrangementTimebase,
    updateTimelineLoopRange,
    updateTimelinePunchRange,
    openAssetInDesign,
    switchWorkspace,
    updateSessionSettings,
    applyAiSuggestion,
    renameRecording,
    deleteRecording,
    archiveRecording,
    promoteRecording,
    tagRecording,
    detectDuplicateRecordings,
    onAudioStatus: (callback: (status: AudioStatus) => void) => {
      if (!isNativeRuntime()) {
        return () => undefined;
      }
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      void listen<AudioStatus>('audio-status', (event) => {
        callback(event.payload);
      }).then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    onAudioMeters: (callback: (meters: AudioMeters) => void) => {
      if (!isNativeRuntime()) return () => undefined;
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      void listen<AudioMeters>('audio-meters', (event) => callback(event.payload)).then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    onTransportStatus: (callback: (status: TransportStatus) => void) => {
      if (!isNativeRuntime()) return () => undefined;
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      void listen<TransportStatus>('transport-status', (event) => callback(event.payload)).then(
        (fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        },
      );
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    onRuntimeRestarted: (callback: (generation: number) => void) => {
      if (!isNativeRuntime()) return () => undefined;
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      void listen<{ generation: number }>('runtime-restarted', (event) => {
        callback(event.payload.generation);
      }).then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    onTrackPluginStateChanged: (callback: (change: TrackPluginStateChange) => void) => {
      if (!isNativeRuntime()) return () => undefined;
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      void listen<TrackPluginStateChange>('track-plugin-state-changed', (event) =>
        callback(event.payload),
      )
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        })
        .catch(() => undefined);
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    onTrackPluginParameterChanged: (callback: (change: TrackPluginParameterChange) => void) => {
      if (!isNativeRuntime()) return () => undefined;
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      void listen<TrackPluginParameterChange>('track-plugin-parameter-changed', (event) =>
        callback(event.payload),
      )
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        })
        .catch(() => undefined);
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
  };
}

/** defaultNativeApi is the shared production instance used when no api is injected. */
export const defaultNativeApi: NativeApi = createNativeApi();
