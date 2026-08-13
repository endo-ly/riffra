import type {
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AssetId,
  AssetPreviewOptions,
  DeviceChannels,
  MidiProbe,
  RuntimeProjectionStatus,
  SessionAudioPair,
} from '@/lib/domain';
import { offlineAudioStatus } from '@/lib/audio-defaults';
import { invokeOrFallback, invoke } from '../invoke';
import { audioCommandError } from './audio-error';

export async function probeMidiDevices(): Promise<MidiProbe> {
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

export async function probeAudioDevices(): Promise<AudioDeviceProbe> {
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

export async function probeDeviceChannels(
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

export async function previewAsset(
  assetId: AssetId,
  options: AssetPreviewOptions,
): Promise<AudioStatus> {
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
  return invokeOrFallback<AudioStatus>('get_audio_status', {}, offlineAudioStatus());
}

export async function getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus> {
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

export async function setEmergencyMute(muted: boolean): Promise<AudioStatus> {
  return await invoke<AudioStatus>('set_emergency_mute', { muted });
}

export async function setMasterGainDb(gainDb: number): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('set_master_gain_db', {
    gainDb,
  });
}

export async function previewMasterGainDb(gainDb: number): Promise<void> {
  await invoke<void>('preview_master_gain_db', { gainDb });
}

export async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('recover_audio_device');
  } catch (error) {
    return await audioCommandError('Recover audio device', error);
  }
}

export async function retryStartupRuntime(): Promise<AudioStatus> {
  return await invoke<AudioStatus>('retry_startup_runtime');
}

export async function setAudioDriver(config: AudioDriverConfig): Promise<AudioStatus> {
  return await invoke<AudioStatus>('set_audio_driver', { config });
}

export async function enableMidiListening(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('enable_midi_listening');
  } catch (error) {
    return await audioCommandError('Enable MIDI listening', error);
  }
}

export async function disableMidiListening(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('disable_midi_listening');
  } catch (error) {
    return await audioCommandError('Disable MIDI listening', error);
  }
}

export async function sendMidiToTrack(
  trackId: string,
  bytes: number[],
): Promise<AudioStatus | null> {
  try {
    await invoke<void>('send_midi_to_track', { trackId, bytes });
    return null;
  } catch (error) {
    return await audioCommandError('Send MIDI to Track', error);
  }
}

export async function panicMidiTrack(trackId: string): Promise<AudioStatus | null> {
  try {
    await invoke<void>('panic_midi_track', { trackId });
    return null;
  } catch (error) {
    return await audioCommandError('Panic MIDI Track', error);
  }
}
