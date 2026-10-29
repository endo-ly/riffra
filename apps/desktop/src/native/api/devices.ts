import type {
  ArrangementMutationResult,
  DeviceInspection,
  DeviceParameterInfo,
  EffectOwner,
  InstrumentMidiInspection,
  InstrumentMidiSettings,
  PluginPresetInfo,
} from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface DeviceApi {
  applyInstrument(trackId: string, instrumentId: string): Promise<ArrangementMutationResult>;
  getInstrumentMidiSettings(trackId: string): Promise<InstrumentMidiInspection>;
  setInstrumentMidiSettings(
    trackId: string,
    settings: InstrumentMidiSettings,
  ): Promise<ArrangementMutationResult>;
  setTrackVst3Instrument(trackId: string, pluginPath: string): Promise<ArrangementMutationResult>;
  clearTrackInstrument(trackId: string): Promise<ArrangementMutationResult>;
  addEffect(owner: EffectOwner, pluginPath: string): Promise<ArrangementMutationResult>;
  removeEffect(owner: EffectOwner, deviceId: string): Promise<ArrangementMutationResult>;
  reorderEffects(
    owner: EffectOwner,
    orderedDeviceIds: string[],
  ): Promise<ArrangementMutationResult>;
  setDeviceBypassed(
    owner: EffectOwner,
    deviceId: string,
    bypassed: boolean,
  ): Promise<ArrangementMutationResult>;
  setDeviceParameter(
    owner: EffectOwner,
    deviceId: string,
    parameterIndex: number,
    value: number,
  ): Promise<ArrangementMutationResult>;
  openPluginEditor(owner: EffectOwner, deviceId: string): Promise<void>;
  inspectDevice(owner: EffectOwner, deviceId: string): Promise<DeviceInspection>;
  listDeviceParameters(owner: EffectOwner, deviceId: string): Promise<DeviceParameterInfo[]>;
  getDeviceParameter(
    owner: EffectOwner,
    deviceId: string,
    parameterIndex: number,
  ): Promise<DeviceParameterInfo>;
  formatDeviceParameterValue(
    owner: EffectOwner,
    deviceId: string,
    parameterIndex: number,
    value: number,
  ): Promise<string>;
  listPluginPresets(owner: EffectOwner, deviceId: string): Promise<PluginPresetInfo[]>;
  getPluginPreset(owner: EffectOwner, deviceId: string): Promise<PluginPresetInfo>;
  setPluginPreset(
    owner: EffectOwner,
    deviceId: string,
    presetIndex: number,
  ): Promise<ArrangementMutationResult>;
}

async function applyInstrument(
  trackId: string,
  instrumentId: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'instrument.apply', params: { trackId, instrumentId } });
}

async function getInstrumentMidiSettings(trackId: string) {
  return dispatchControl({ command: 'instrument.midi.get', params: { trackId } });
}

async function setInstrumentMidiSettings(
  trackId: string,
  settings: InstrumentMidiSettings,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'instrument.midi.set', params: { trackId, settings } });
}

async function setTrackVst3Instrument(
  trackId: string,
  pluginPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'instrument.vst3.set', params: { trackId, pluginPath } });
}

async function clearTrackInstrument(trackId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'instrument.clear', params: { trackId } });
}

async function addEffect(
  owner: EffectOwner,
  pluginPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'effect.add', params: { owner, pluginPath } });
}

async function removeEffect(
  owner: EffectOwner,
  deviceId: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'effect.remove', params: { owner, deviceId } });
}

async function reorderEffects(
  owner: EffectOwner,
  orderedDeviceIds: string[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'effect.reorder',
    params: { owner, deviceIds: orderedDeviceIds },
  });
}

async function setDeviceBypassed(
  owner: EffectOwner,
  deviceId: string,
  bypassed: boolean,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'device.bypass', params: { owner, deviceId, bypassed } });
}

async function setDeviceParameter(
  owner: EffectOwner,
  deviceId: string,
  parameterIndex: number,
  value: number,
): Promise<ArrangementMutationResult> {
  const result = await dispatchControl({
    command: 'device.parameter.set',
    params: { owner, deviceId, parameterIndex, value },
  });
  return result.mutation;
}

async function openPluginEditor(owner: EffectOwner, deviceId: string): Promise<void> {
  await dispatchControl({ command: 'plugin.editor.open', params: { owner, deviceId } });
}

function inspectDevice(owner: EffectOwner, deviceId: string) {
  return dispatchControl({ command: 'device.inspect', params: { owner, deviceId } });
}

function listDeviceParameters(owner: EffectOwner, deviceId: string) {
  return dispatchControl({ command: 'device.parameter.list', params: { owner, deviceId } });
}

function getDeviceParameter(owner: EffectOwner, deviceId: string, parameterIndex: number) {
  return dispatchControl({
    command: 'device.parameter.get',
    params: { owner, deviceId, parameterIndex },
  });
}

async function formatDeviceParameterValue(
  owner: EffectOwner,
  deviceId: string,
  parameterIndex: number,
  value: number,
): Promise<string> {
  return dispatchControl({
    command: 'device.parameter.format',
    params: { owner, deviceId, parameterIndex, value },
  });
}

function listPluginPresets(owner: EffectOwner, deviceId: string) {
  return dispatchControl({ command: 'plugin.preset.list', params: { owner, deviceId } });
}

function getPluginPreset(owner: EffectOwner, deviceId: string) {
  return dispatchControl({ command: 'plugin.preset.get', params: { owner, deviceId } });
}

function setPluginPreset(owner: EffectOwner, deviceId: string, presetIndex: number) {
  return dispatchControl({
    command: 'plugin.preset.set',
    params: { owner, deviceId, presetIndex },
  });
}

export const deviceApi: DeviceApi = {
  applyInstrument,
  getInstrumentMidiSettings,
  setInstrumentMidiSettings,
  setTrackVst3Instrument,
  clearTrackInstrument,
  addEffect,
  removeEffect,
  reorderEffects,
  setDeviceBypassed,
  setDeviceParameter,
  openPluginEditor,
  inspectDevice,
  listDeviceParameters,
  getDeviceParameter,
  formatDeviceParameterValue,
  listPluginPresets,
  getPluginPreset,
  setPluginPreset,
};
