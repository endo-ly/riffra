import type {
  ArrangementMutationResult,
  EffectOwner,
  MixBusKind,
  MixBusPatch,
  SendPatch,
  SendPosition,
  SidechainSendPatch,
} from '@/model/domain';
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

export async function addEffect(
  owner: EffectOwner,
  pluginPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'effect.add', params: { owner, pluginPath } });
}

export async function removeEffect(
  owner: EffectOwner,
  deviceId: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'effect.remove', params: { owner, deviceId } });
}

export async function reorderEffects(
  owner: EffectOwner,
  orderedDeviceIds: string[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'effect.reorder',
    params: { owner, deviceIds: orderedDeviceIds },
  });
}

export async function setDeviceBypassed(
  owner: EffectOwner,
  deviceId: string,
  bypassed: boolean,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'device.bypass', params: { owner, deviceId, bypassed } });
}

export async function setDeviceParameter(
  owner: EffectOwner,
  deviceId: string,
  parameterIndex: number,
  value: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'device.parameter.set',
    params: { owner, deviceId, parameterIndex, value },
  });
}

export async function openPluginEditor(owner: EffectOwner, deviceId: string): Promise<void> {
  await dispatchControl({ command: 'plugin.editor.open', params: { owner, deviceId } });
}

export function inspectDevice(owner: EffectOwner, deviceId: string) {
  return dispatchControl({ command: 'device.inspect', params: { owner, deviceId } });
}

export function listDeviceParameters(owner: EffectOwner, deviceId: string) {
  return dispatchControl({ command: 'device.parameter.list', params: { owner, deviceId } });
}

export function getDeviceParameter(owner: EffectOwner, deviceId: string, parameterIndex: number) {
  return dispatchControl({
    command: 'device.parameter.get',
    params: { owner, deviceId, parameterIndex },
  });
}

export async function formatDeviceParameterValue(
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

export function listPluginPresets(owner: EffectOwner, deviceId: string) {
  return dispatchControl({ command: 'plugin.preset.list', params: { owner, deviceId } });
}

export function getPluginPreset(owner: EffectOwner, deviceId: string) {
  return dispatchControl({ command: 'plugin.preset.get', params: { owner, deviceId } });
}

export function setPluginPreset(owner: EffectOwner, deviceId: string, presetIndex: number) {
  return dispatchControl({
    command: 'plugin.preset.set',
    params: { owner, deviceId, presetIndex },
  });
}

export function addMixBus(name: string, kind: MixBusKind) {
  return dispatchControl({ command: 'mixer.bus.add', params: { name, kind } });
}
export function updateMixBus(busId: string, patch: MixBusPatch) {
  return dispatchControl({ command: 'mixer.bus.update', params: { busId, patch } });
}
export function removeMixBus(busId: string) {
  return dispatchControl({ command: 'mixer.bus.remove', params: { busId } });
}
export function setTrackOutput(trackId: string, busId: string | null) {
  return dispatchControl({ command: 'mixer.output.set', params: { trackId, busId } });
}
export function addSend(
  trackId: string,
  returnBusId: string,
  levelDb: number,
  position: SendPosition,
) {
  return dispatchControl({
    command: 'mixer.send.add',
    params: { trackId, returnBusId, levelDb, position },
  });
}
export function updateSend(sendId: string, patch: SendPatch) {
  return dispatchControl({ command: 'mixer.send.update', params: { sendId, patch } });
}
export function removeSend(sendId: string) {
  return dispatchControl({ command: 'mixer.send.remove', params: { sendId } });
}
export function addSidechainSend(
  sourceTrackId: string,
  targetDeviceId: string,
  levelDb: number,
  position: SendPosition,
) {
  return dispatchControl({
    command: 'mixer.sidechain.add',
    params: { sourceTrackId, targetDeviceId, levelDb, position },
  });
}
export function updateSidechainSend(sidechainSendId: string, patch: SidechainSendPatch) {
  return dispatchControl({
    command: 'mixer.sidechain.update',
    params: { sidechainSendId, patch },
  });
}
export function removeSidechainSend(sidechainSendId: string) {
  return dispatchControl({
    command: 'mixer.sidechain.remove',
    params: { sidechainSendId },
  });
}
