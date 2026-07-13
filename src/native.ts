import { invoke } from '@tauri-apps/api/core';
import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  BootstrapState,
  LibraryAsset,
  MidiEvent,
  MidiExportResult,
  MidiProbe,
  ProjectExport,
  RecordingAsset,
  RecoveryCandidate,
  RenderOptions,
  RenderResult,
  SamplePad,
  ScanReport,
  ScratchSession,
  SeparationResult,
} from './domain';
import { defaultSession } from './domain';
import type { NativeApi } from './native-api';

const defaultVst3Root = 'C:\\Program Files\\Common Files\\VST3';

export async function bootstrap(): Promise<BootstrapState> {
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

export async function saveScratch(session: ScratchSession): Promise<string | null> {
  try {
    await invoke('save_scratch_session', { session });
    return null;
  } catch (error) {
    if (!('__TAURI_INTERNALS__' in window)) {
      localStorage.setItem('riffra.preview.scratch', JSON.stringify(session));
    }
    return nativeErrorText(error);
  }
}

export async function restoreRecoveryGeneration(fileName: string): Promise<ScratchSession | null> {
  try {
    return await invoke<ScratchSession>('restore_recovery_generation', { fileName });
  } catch {
    return null;
  }
}

export async function exportScratchSession(): Promise<ProjectExport | null> {
  try {
    return await invoke<ProjectExport>('export_scratch_session');
  } catch {
    return null;
  }
}

export async function importScratchSession(path: string): Promise<ScratchSession | null> {
  try {
    return await invoke<ScratchSession>('import_scratch_session', { path });
  } catch {
    return null;
  }
}

export async function scanVst3Folder(path?: string): Promise<ScanReport> {
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

export async function listRecordings(query?: string): Promise<RecordingAsset[]> {
  try {
    return await invoke<RecordingAsset[]>('list_recordings', { query: query ?? null });
  } catch {
    return [];
  }
}

export async function searchLibrary(query: string): Promise<LibraryAsset[]> {
  if (!query.trim()) return [];
  try {
    return await invoke<LibraryAsset[]>('search_library', { query });
  } catch {
    return [];
  }
}

export async function updateLibraryAsset(
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

export async function relatedLibraryAssets(id: string): Promise<LibraryAsset[]> {
  try {
    return await invoke<LibraryAsset[]>('related_library_assets', { id });
  } catch {
    return [];
  }
}

export async function analyzeAudio(path: string): Promise<AudioAnalysis | null> {
  try {
    return await invoke<AudioAnalysis>('analyze_audio', { path });
  } catch {
    return null;
  }
}

export async function probeMidiDevices(): Promise<MidiProbe> {
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

export async function readMidiEvents(path: string): Promise<MidiEvent[]> {
  try {
    return await invoke<MidiEvent[]>('read_midi_events', { path });
  } catch {
    return [];
  }
}

export async function probeAudioDevices(): Promise<AudioDeviceProbe> {
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

export async function listSeparations(): Promise<SeparationResult[]> {
  try {
    return await invoke<SeparationResult[]>('list_separations');
  } catch {
    return [];
  }
}

export async function separateChannels(path: string): Promise<SeparationResult | null> {
  try {
    return await invoke<SeparationResult>('separate_channels', { path });
  } catch {
    return null;
  }
}

export async function renderTimeline(options: RenderOptions): Promise<RenderResult | null> {
  try {
    return await invoke<RenderResult>('render_timeline', { options });
  } catch {
    return null;
  }
}

export async function renderTimelineStems(options: RenderOptions): Promise<RenderResult[]> {
  try {
    return await invoke<RenderResult[]>('render_timeline_stems', { options });
  } catch {
    return [];
  }
}

export async function exportMidi(): Promise<MidiExportResult | null> {
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

export async function loadPlugin(path: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('load_plugin', { path });
  } catch (error) {
    return await audioCommandError('Load plugin', error);
  }
}

export async function clearPlugin(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('clear_plugin');
  } catch (error) {
    return await audioCommandError('Clear plugin', error);
  }
}

export async function previewSample(
  path: string,
  startMs: number,
  endMs: number,
  looped = false,
  gain = 1,
  voiceKey?: number,
): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('preview_sample', {
      path,
      startMs,
      endMs,
      looped,
      gain,
      voiceKey: voiceKey ?? null,
    });
  } catch (error) {
    return await audioCommandError('Preview sample', error);
  }
}

export async function stopSamplePreview(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('stop_preview');
  } catch (error) {
    return await audioCommandError('Stop preview', error);
  }
}

export async function stopSamplePreviewKey(voiceKey: number): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('stop_preview_for_key', { voiceKey });
  } catch (error) {
    return await audioCommandError('Stop mapped preview', error);
  }
}

export async function getAudioStatus(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('get_audio_status');
  } catch {
    return {
      state: 'offline',
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
      message: 'Native audio sidecar is not connected.',
    };
  }
}

export async function setEmergencyMute(muted: boolean): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('set_emergency_mute', { muted });
  } catch (error) {
    return await audioCommandError(
      muted ? 'Engage emergency mute' : 'Release emergency mute',
      error,
      true,
    );
  }
}

export async function startRecording(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('start_recording');
  } catch (error) {
    return await audioCommandError('Start recording', error);
  }
}

export async function stopRecording(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('stop_recording');
  } catch (error) {
    return await audioCommandError('Stop recording', error);
  }
}

export async function setPluginBypassed(bypassed: boolean): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('set_plugin_bypassed', { bypassed });
  } catch (error) {
    return await audioCommandError('Change plugin bypass', error);
  }
}

export async function setPluginParameter(index: number, value: number): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('set_plugin_parameter', { index, value });
  } catch (error) {
    return await audioCommandError('Set plugin parameter', error);
  }
}

export async function setPluginState(stateData: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('set_plugin_state', { stateData });
  } catch (error) {
    return await audioCommandError('Set plugin state', error);
  }
}

export async function setMasterGainDb(gainDb: number): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('set_master_gain_db', { gainDb });
  } catch (error) {
    return await audioCommandError('Set master gain', error);
  }
}

export async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('recover_audio_device');
  } catch (error) {
    return await audioCommandError('Recover audio device', error);
  }
}

export async function setAudioDriver(
  driver: string,
  sampleRate: number | null = null,
  bufferSize: number | null = null,
): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('set_audio_driver', { driver, sampleRate, bufferSize });
  } catch (error) {
    return await audioCommandError('Set audio driver', error);
  }
}

export async function openMidiInput(name: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('open_midi_input', { name });
  } catch (error) {
    return await audioCommandError('Open MIDI input', error);
  }
}

export async function closeMidiInput(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('close_midi_input');
  } catch (error) {
    return await audioCommandError('Close MIDI input', error);
  }
}

export async function configureSamplePads(pads: SamplePad[]): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('configure_sample_pads', { pads });
  } catch (error) {
    return await audioCommandError('Configure sample pads', error);
  }
}

/**
 * createNativeApi returns the production NativeApi that delegates to the
 * invoke-backed helpers in this module. Behavior is identical to calling the
 * named functions directly; this wrapper exists so callers can depend on the
 * NativeApi seam and tests can substitute a FakeNativeApi.
 */
export function createNativeApi(): NativeApi {
  return {
    bootstrap,
    saveScratch,
    restoreRecoveryGeneration,
    exportScratchSession,
    importScratchSession,
    scanVst3Folder,
    listRecordings,
    searchLibrary,
    updateLibraryAsset,
    relatedLibraryAssets,
    analyzeAudio,
    readMidiEvents,
    probeMidiDevices,
    probeAudioDevices,
    listSeparations,
    separateChannels,
    renderTimeline,
    renderTimelineStems,
    exportMidi,
    loadPlugin,
    clearPlugin,
    previewSample,
    stopSamplePreview,
    stopSamplePreviewKey,
    getAudioStatus,
    setEmergencyMute,
    startRecording,
    stopRecording,
    setPluginBypassed,
    setPluginParameter,
    setPluginState,
    setMasterGainDb,
    recoverAudioDevice,
    setAudioDriver,
    openMidiInput,
    closeMidiInput,
    configureSamplePads,
  };
}

/** defaultNativeApi is the shared production instance used when no api is injected. */
export const defaultNativeApi: NativeApi = createNativeApi();
