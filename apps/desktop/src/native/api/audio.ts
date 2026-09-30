import type {
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AssetId,
  DeviceChannels,
  ArrangementMutationResult,
} from '@/model/domain';
import type { AssetPreviewOptions } from '../contracts';
import { offlineAudioStatus } from '@/shared/audio/audio-defaults';
import { dispatchControl, dispatchControlOrFallback } from '../invoke';
import { audioCommandError } from './audio-error';

export async function probeAudioDevices(): Promise<AudioDeviceProbe> {
  return dispatchControlOrFallback(
    { command: 'audio.probe', params: {} },
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
  return dispatchControlOrFallback(
    { command: 'audio.channels.probe', params: { driver, inputDevice, outputDevice } },
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
    return await dispatchControl({
      command: 'asset.preview',
      params: {
        assetId,
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

export async function previewInstrument(instrumentId: string): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'instrument.preview', params: { instrumentId } });
  } catch (error) {
    return await audioCommandError('Preview instrument', error);
  }
}

export async function stopInstrumentPreview(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'instrument.preview.stop', params: {} });
  } catch (error) {
    return await audioCommandError('Stop instrument preview', error);
  }
}

export async function stopPreview(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'asset.preview.stop', params: {} });
  } catch (error) {
    return await audioCommandError('Stop preview', error);
  }
}

export async function getAudioStatus(): Promise<AudioStatus> {
  return dispatchControlOrFallback({ command: 'audio.status', params: {} }, offlineAudioStatus());
}

export async function setEmergencyMute(muted: boolean): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.emergency-mute', params: { muted } });
}

export async function resetFeedbackProtection(): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.feedback-protection.reset', params: {} });
}

export async function setMasterGainDb(gainDb: number): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'audio.master-gain.set', params: { gainDb } });
}

export async function previewMasterGainDb(gainDb: number): Promise<void> {
  await dispatchControl({ command: 'audio.master-gain.preview', params: { gainDb } });
}

export async function previewTrackMix(
  trackId: string,
  patch: { gainDb?: number; pan?: number },
): Promise<void> {
  await dispatchControl({
    command: 'track.mix.preview',
    params: { trackId, gainDb: patch.gainDb, pan: patch.pan },
  });
}

export async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'audio.recover', params: {} });
  } catch (error) {
    return await audioCommandError('Recover audio device', error);
  }
}

export async function retryStartupRuntime(): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.startup.retry', params: {} });
}

export async function setAudioDriver(config: AudioDriverConfig): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.driver.set', params: config });
}

export async function enableMidiListening(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'midi.listening.enable', params: {} });
  } catch (error) {
    return await audioCommandError('Enable MIDI listening', error);
  }
}

export async function disableMidiListening(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'midi.listening.disable', params: {} });
  } catch (error) {
    return await audioCommandError('Disable MIDI listening', error);
  }
}

export async function sendMidiToTrack(
  trackId: string,
  bytes: number[],
): Promise<AudioStatus | null> {
  try {
    await dispatchControl({ command: 'midi.send', params: { trackId, bytes } });
    return null;
  } catch (error) {
    return await audioCommandError('Send MIDI to Track', error);
  }
}

export async function setLiveMidiTarget(trackId: string | null): Promise<AudioStatus | null> {
  try {
    await dispatchControl({ command: 'midi.target.set', params: { trackId } });
    return null;
  } catch (error) {
    return await audioCommandError('Set live MIDI target', error);
  }
}

export async function panicMidiTrack(trackId: string): Promise<AudioStatus | null> {
  try {
    await dispatchControl({ command: 'midi.panic', params: { trackId } });
    return null;
  } catch (error) {
    return await audioCommandError('Panic MIDI Track', error);
  }
}
