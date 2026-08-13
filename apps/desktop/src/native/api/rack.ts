import type { CreativeSession } from '@/model/domain';
import { invoke } from '../invoke';
import type { TrackPluginParameterChange, TrackPluginStateChange } from '../native-api';

export async function setTrackInstrument(
  trackId: string,
  pluginPath: string,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_instrument', { trackId, pluginPath });
}

export async function clearTrackInstrument(trackId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('clear_track_instrument', { trackId });
}

export async function addTrackEffect(
  trackId: string,
  pluginPath: string,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_track_effect', { trackId, pluginPath });
}

export async function removeTrackEffect(
  trackId: string,
  deviceId: string,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_track_effect', { trackId, deviceId });
}

export async function reorderTrackEffects(
  trackId: string,
  orderedDeviceIds: string[],
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('reorder_track_effects', {
    trackId,
    orderedDeviceIds,
  });
}

export async function setTrackDeviceBypassed(
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

export async function setTrackDeviceParameter(
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

export async function openTrackPluginEditor(trackId: string, deviceId: string): Promise<void> {
  await invoke<void>('open_track_plugin_editor', { trackId, deviceId });
}

export async function persistTrackPluginState(
  change: TrackPluginStateChange,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('persist_track_plugin_state', {
    trackId: change.trackId,
    deviceId: change.deviceId,
    parameterValues: change.parameterValues,
    stateData: change.stateData ?? null,
    bypassed: change.bypassed,
  });
}

export async function persistTrackPluginParameter(
  change: TrackPluginParameterChange,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('persist_track_plugin_parameter', {
    trackId: change.trackId,
    deviceId: change.deviceId,
    parameterIndex: change.parameterIndex,
    value: change.value,
  });
}
