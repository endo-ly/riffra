import type { ArrangementMutationResult } from '@/model/domain';
import { dispatchControl } from '../invoke';

export async function applyInstrument(
  trackId: string,
  instrumentId: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'instrument.apply', params: { trackId, instrumentId } });
}

export async function setTrackVst3Instrument(
  trackId: string,
  pluginPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'instrument.vst3.set', params: { trackId, pluginPath } });
}

export async function clearTrackInstrument(trackId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'instrument.clear', params: { trackId } });
}

export async function addTrackEffect(
  trackId: string,
  pluginPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'effect.add', params: { trackId, pluginPath } });
}

export async function removeTrackEffect(
  trackId: string,
  deviceId: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'effect.remove', params: { trackId, deviceId } });
}

export async function reorderTrackEffects(
  trackId: string,
  orderedDeviceIds: string[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'effect.reorder',
    params: { trackId, deviceIds: orderedDeviceIds },
  });
}

export async function setTrackDeviceBypassed(
  trackId: string,
  deviceId: string,
  bypassed: boolean,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'device.bypass', params: { trackId, deviceId, bypassed } });
}

export async function setTrackDeviceParameter(
  trackId: string,
  deviceId: string,
  parameterIndex: number,
  value: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'device.parameter.set',
    params: { trackId, deviceId, parameterIndex, value },
  });
}

export async function openTrackPluginEditor(trackId: string, deviceId: string): Promise<void> {
  await dispatchControl({ command: 'plugin.editor.open', params: { trackId, deviceId } });
}

export function inspectTrackDevice(trackId: string, deviceId: string) {
  return dispatchControl({ command: 'device.inspect', params: { trackId, deviceId } });
}

export function listTrackDeviceParameters(trackId: string, deviceId: string) {
  return dispatchControl({ command: 'device.parameter.list', params: { trackId, deviceId } });
}

export function getTrackDeviceParameter(trackId: string, deviceId: string, parameterIndex: number) {
  return dispatchControl({
    command: 'device.parameter.get',
    params: { trackId, deviceId, parameterIndex },
  });
}

export function listTrackPluginPresets(trackId: string, deviceId: string) {
  return dispatchControl({ command: 'plugin.preset.list', params: { trackId, deviceId } });
}

export function getTrackPluginPreset(trackId: string, deviceId: string) {
  return dispatchControl({ command: 'plugin.preset.get', params: { trackId, deviceId } });
}

export function setTrackPluginPreset(trackId: string, deviceId: string, presetIndex: number) {
  return dispatchControl({
    command: 'plugin.preset.set',
    params: { trackId, deviceId, presetIndex },
  });
}
