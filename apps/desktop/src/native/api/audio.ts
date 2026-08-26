import type {
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AssetId,
  DeviceChannels,
  SessionAudioPair,
} from '@/model/domain';
import type { AssetPreviewOptions } from '../contracts';
import { offlineAudioStatus } from '@/shared/audio/audio-defaults';
import { invokeHostOrFallback, invokeHost } from '../invoke';
import { audioCommandError } from './audio-error';

export async function probeAudioDevices(): Promise<AudioDeviceProbe> {
  return invokeHostOrFallback<AudioDeviceProbe>(
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
  return invokeHostOrFallback<DeviceChannels>(
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
    return await invokeHost<AudioStatus>('preview_asset', {
      assetId,
      options: {
        startMs: options.startMs ?? 0,
        endMs: options.endMs ?? null,
        looped: options.looped ?? false,
        gain: options.gain ?? 1,
      },
    });
  } catch (error) {
    return await audioCommandError('Preview asset', error);
  }
}

export async function stopPreview(): Promise<AudioStatus> {
  try {
    return await invokeHost<AudioStatus>('stop_preview');
  } catch (error) {
    return await audioCommandError('Stop preview', error);
  }
}

export async function getAudioStatus(): Promise<AudioStatus> {
  return invokeHostOrFallback<AudioStatus>('get_audio_status', {}, offlineAudioStatus());
}

export async function setEmergencyMute(muted: boolean): Promise<AudioStatus> {
  return await invokeHost<AudioStatus>('set_emergency_mute', { muted });
}

export async function setMasterGainDb(gainDb: number): Promise<SessionAudioPair> {
  return invokeHost<SessionAudioPair>('set_master_gain_db', {
    gainDb,
  });
}

export async function previewMasterGainDb(gainDb: number): Promise<void> {
  await invokeHost<void>('preview_master_gain_db', { gainDb });
}

export async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await invokeHost<AudioStatus>('recover_audio_device');
  } catch (error) {
    return await audioCommandError('Recover audio device', error);
  }
}

export async function retryStartupRuntime(): Promise<AudioStatus> {
  return await invokeHost<AudioStatus>('retry_startup_runtime');
}

export async function setAudioDriver(config: AudioDriverConfig): Promise<AudioStatus> {
  return await invokeHost<AudioStatus>('set_audio_driver', { config });
}

export async function enableMidiListening(): Promise<AudioStatus> {
  try {
    return await invokeHost<AudioStatus>('enable_midi_listening');
  } catch (error) {
    return await audioCommandError('Enable MIDI listening', error);
  }
}

export async function disableMidiListening(): Promise<AudioStatus> {
  try {
    return await invokeHost<AudioStatus>('disable_midi_listening');
  } catch (error) {
    return await audioCommandError('Disable MIDI listening', error);
  }
}

export async function sendMidiToTrack(
  trackId: string,
  bytes: number[],
): Promise<AudioStatus | null> {
  try {
    await invokeHost<void>('send_midi_to_track', { trackId, bytes });
    return null;
  } catch (error) {
    return await audioCommandError('Send MIDI to Track', error);
  }
}

export async function panicMidiTrack(trackId: string): Promise<AudioStatus | null> {
  try {
    await invokeHost<void>('panic_midi_track', { trackId });
    return null;
  } catch (error) {
    return await audioCommandError('Panic MIDI Track', error);
  }
}
