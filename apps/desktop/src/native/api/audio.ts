export interface AssetPreviewOptions {
  startMs?: number;
  endMs?: number | null;
  looped?: boolean;
  gain?: number;
}

import type {
  AssetId,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  DeviceChannels,
} from '@/model/domain';

import { dispatchControl } from '../invoke';
import { audioCommandError } from './audio-error';

export interface AudioApi {
  probeAudioDevices(): Promise<AudioDeviceProbe>;
  probeDeviceChannels(
    driver: string,
    inputDevice: string,
    outputDevice: string,
  ): Promise<DeviceChannels>;
  previewAsset(assetId: AssetId, options: AssetPreviewOptions): Promise<AudioStatus>;
  previewInstrument(instrumentId: string): Promise<AudioStatus>;
  stopInstrumentPreview(): Promise<AudioStatus>;
  openPluginAudition(pluginPath: string): Promise<AudioStatus>;
  stopPreview(): Promise<AudioStatus>;
  getAudioStatus(): Promise<AudioStatus>;
  setEmergencyMute(muted: boolean): Promise<AudioStatus>;
  resetFeedbackProtection(): Promise<AudioStatus>;
  recoverAudioDevice(): Promise<AudioStatus>;
  retryStartupRuntime(): Promise<AudioStatus>;
  setAudioDriver(config: AudioDriverConfig): Promise<AudioStatus>;
  enableMidiListening(): Promise<AudioStatus>;
  disableMidiListening(): Promise<AudioStatus>;
  sendMidiToTrack(trackId: string, bytes: number[]): Promise<AudioStatus | null>;
  setLiveMidiTarget(trackId: string | null): Promise<AudioStatus | null>;
  panicMidiTrack(trackId: string): Promise<AudioStatus | null>;
}

async function probeAudioDevices(): Promise<AudioDeviceProbe> {
  return dispatchControl({ command: 'audio.probe', params: {} });
}

async function probeDeviceChannels(
  driver: string,
  inputDevice: string,
  outputDevice: string,
): Promise<DeviceChannels> {
  return dispatchControl({
    command: 'audio.channels.probe',
    params: { driver, inputDevice, outputDevice },
  });
}

async function previewAsset(assetId: AssetId, options: AssetPreviewOptions): Promise<AudioStatus> {
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

async function previewInstrument(instrumentId: string): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'instrument.preview', params: { instrumentId } });
  } catch (error) {
    return await audioCommandError('Preview instrument', error);
  }
}

async function stopInstrumentPreview(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'instrument.preview.stop', params: {} });
  } catch (error) {
    return await audioCommandError('Stop instrument preview', error);
  }
}

async function openPluginAudition(pluginPath: string): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'plugin.audition.open', params: { pluginPath } });
  } catch (error) {
    return await audioCommandError('Open plug-in', error);
  }
}

async function stopPreview(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'asset.preview.stop', params: {} });
  } catch (error) {
    return await audioCommandError('Stop preview', error);
  }
}

async function getAudioStatus(): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.status', params: {} });
}

async function setEmergencyMute(muted: boolean): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.emergency-mute', params: { muted } });
}

async function resetFeedbackProtection(): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.feedback-protection.reset', params: {} });
}

async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'audio.recover', params: {} });
  } catch (error) {
    return await audioCommandError('Recover audio device', error);
  }
}

async function retryStartupRuntime(): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.startup.retry', params: {} });
}

async function setAudioDriver(config: AudioDriverConfig): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.driver.set', params: config });
}

async function enableMidiListening(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'midi.listening.enable', params: {} });
  } catch (error) {
    return await audioCommandError('Enable MIDI listening', error);
  }
}

async function disableMidiListening(): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'midi.listening.disable', params: {} });
  } catch (error) {
    return await audioCommandError('Disable MIDI listening', error);
  }
}

async function sendMidiToTrack(trackId: string, bytes: number[]): Promise<AudioStatus | null> {
  try {
    await dispatchControl({ command: 'midi.send', params: { trackId, bytes } });
    return null;
  } catch (error) {
    return await audioCommandError('Send MIDI to Track', error);
  }
}

async function setLiveMidiTarget(trackId: string | null): Promise<AudioStatus | null> {
  try {
    await dispatchControl({ command: 'midi.target.set', params: { trackId } });
    return null;
  } catch (error) {
    return await audioCommandError('Set live MIDI target', error);
  }
}

async function panicMidiTrack(trackId: string): Promise<AudioStatus | null> {
  try {
    await dispatchControl({ command: 'midi.panic', params: { trackId } });
    return null;
  } catch (error) {
    return await audioCommandError('Panic MIDI Track', error);
  }
}

export const audioApi: AudioApi = {
  probeAudioDevices,
  probeDeviceChannels,
  previewAsset,
  previewInstrument,
  stopInstrumentPreview,
  openPluginAudition,
  stopPreview,
  getAudioStatus,
  setEmergencyMute,
  resetFeedbackProtection,
  recoverAudioDevice,
  retryStartupRuntime,
  setAudioDriver,
  enableMidiListening,
  disableMidiListening,
  sendMidiToTrack,
  setLiveMidiTarget,
  panicMidiTrack,
};
