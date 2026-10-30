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
} from '@/model/generated';

import { dispatchControl } from '../invoke';

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
  sendMidiToTrack(trackId: string, bytes: number[]): Promise<void>;
  setLiveMidiTarget(trackId: string | null): Promise<void>;
  panicMidiTrack(trackId: string): Promise<void>;
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

function previewAsset(assetId: AssetId, options: AssetPreviewOptions): Promise<AudioStatus> {
  return dispatchControl({
    command: 'asset.preview',
    params: {
      assetId,
      startMs: options.startMs ?? 0,
      endMs: options.endMs ?? null,
      looped: options.looped ?? false,
      gain: options.gain ?? 1,
    },
  });
}

function previewInstrument(instrumentId: string): Promise<AudioStatus> {
  return dispatchControl({ command: 'instrument.preview', params: { instrumentId } });
}

function stopInstrumentPreview(): Promise<AudioStatus> {
  return dispatchControl({ command: 'instrument.preview.stop', params: {} });
}

function openPluginAudition(pluginPath: string): Promise<AudioStatus> {
  return dispatchControl({ command: 'plugin.audition.open', params: { pluginPath } });
}

function stopPreview(): Promise<AudioStatus> {
  return dispatchControl({ command: 'asset.preview.stop', params: {} });
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

function recoverAudioDevice(): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.recover', params: {} });
}

async function retryStartupRuntime(): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.startup.retry', params: {} });
}

async function setAudioDriver(config: AudioDriverConfig): Promise<AudioStatus> {
  return dispatchControl({ command: 'audio.driver.set', params: config });
}

function enableMidiListening(): Promise<AudioStatus> {
  return dispatchControl({ command: 'midi.listening.enable', params: {} });
}

function disableMidiListening(): Promise<AudioStatus> {
  return dispatchControl({ command: 'midi.listening.disable', params: {} });
}

async function sendMidiToTrack(trackId: string, bytes: number[]): Promise<void> {
  await dispatchControl({ command: 'midi.send', params: { trackId, bytes } });
}

async function setLiveMidiTarget(trackId: string | null): Promise<void> {
  await dispatchControl({ command: 'midi.target.set', params: { trackId } });
}

async function panicMidiTrack(trackId: string): Promise<void> {
  await dispatchControl({ command: 'midi.panic', params: { trackId } });
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
