import type { ArrangementMutationResult, BuiltInInstrumentSummary } from '@/model/domain';
import { invokeHost } from '../invoke';

async function invokeRack(command: string, args: Record<string, unknown>) {
  return invokeHost<ArrangementMutationResult>(command, args);
}

export async function listBuiltInInstruments(): Promise<BuiltInInstrumentSummary[]> {
  return await invokeHost<BuiltInInstrumentSummary[]>('list_built_in_instruments', {});
}

export async function setTrackBuiltInInstrument(
  trackId: string,
  presetId: string,
): Promise<ArrangementMutationResult> {
  return await invokeRack('set_track_built_in_instrument', { trackId, presetId });
}

export async function setTrackVst3Instrument(
  trackId: string,
  pluginPath: string,
): Promise<ArrangementMutationResult> {
  return await invokeRack('set_track_vst3_instrument', { trackId, pluginPath });
}

export async function clearTrackInstrument(trackId: string): Promise<ArrangementMutationResult> {
  return await invokeRack('clear_track_instrument', { trackId });
}

export async function addTrackEffect(
  trackId: string,
  pluginPath: string,
): Promise<ArrangementMutationResult> {
  return await invokeRack('add_track_effect', { trackId, pluginPath });
}

export async function removeTrackEffect(
  trackId: string,
  deviceId: string,
): Promise<ArrangementMutationResult> {
  return await invokeRack('remove_track_effect', { trackId, deviceId });
}

export async function reorderTrackEffects(
  trackId: string,
  orderedDeviceIds: string[],
): Promise<ArrangementMutationResult> {
  return await invokeRack('reorder_track_effects', { trackId, orderedDeviceIds });
}

export async function setTrackDeviceBypassed(
  trackId: string,
  deviceId: string,
  bypassed: boolean,
): Promise<ArrangementMutationResult> {
  return await invokeRack('set_track_device_bypassed', { trackId, deviceId, bypassed });
}

export async function setTrackDeviceParameter(
  trackId: string,
  deviceId: string,
  parameterIndex: number,
  value: number,
): Promise<ArrangementMutationResult> {
  return await invokeRack('set_track_device_parameter', {
    trackId,
    deviceId,
    parameterIndex,
    value,
  });
}

export async function openTrackPluginEditor(trackId: string, deviceId: string): Promise<void> {
  await invokeHost<void>('open_track_plugin_editor', { trackId, deviceId });
}
