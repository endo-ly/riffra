import type { RigState } from '@/model/generated';
import { dispatchControl } from '@/native/invoke';

export interface RigApi {
  getRigState(): Promise<RigState>;
  createRig(name: string, channelIndex: number): Promise<RigState>;
  updateRig(
    rigId: string,
    patch: { name?: string; channelIndex?: number; gainDb?: number },
  ): Promise<RigState>;
  duplicateRig(rigId: string): Promise<RigState>;
  deleteRig(rigId: string): Promise<RigState>;
  addRigEffect(rigId: string, pluginPath: string): Promise<RigState>;
  removeRigEffect(rigId: string, deviceId: string): Promise<RigState>;
  moveRigEffect(rigId: string, orderedDeviceIds: string[]): Promise<RigState>;
  setRigEffectBypassed(rigId: string, deviceId: string, bypassed: boolean): Promise<RigState>;
  startRig(rigId: string): Promise<RigState>;
  stopRig(): Promise<RigState>;
  openRigEditor(rigId: string, deviceId: string): Promise<void>;
}

function getRigState() {
  return dispatchControl({ command: 'rig.list', params: {} });
}

function createRig(name: string, channelIndex: number) {
  return dispatchControl({ command: 'rig.create', params: { name, channelIndex } });
}

function updateRig(
  rigId: string,
  patch: { name?: string; channelIndex?: number; gainDb?: number },
) {
  return dispatchControl({ command: 'rig.update', params: { rigId, ...patch } });
}

function duplicateRig(rigId: string) {
  return dispatchControl({ command: 'rig.duplicate', params: { rigId } });
}

function deleteRig(rigId: string) {
  return dispatchControl({ command: 'rig.delete', params: { rigId } });
}

function addRigEffect(rigId: string, pluginPath: string) {
  return dispatchControl({ command: 'rig.effect.add', params: { rigId, pluginPath } });
}

function removeRigEffect(rigId: string, deviceId: string) {
  return dispatchControl({ command: 'rig.effect.remove', params: { rigId, deviceId } });
}

function moveRigEffect(rigId: string, orderedDeviceIds: string[]) {
  return dispatchControl({
    command: 'rig.effect.move',
    params: { rigId, deviceIds: orderedDeviceIds },
  });
}

function setRigEffectBypassed(rigId: string, deviceId: string, bypassed: boolean) {
  return dispatchControl({
    command: 'rig.effect.update',
    params: { rigId, deviceId, bypassed },
  });
}

function startRig(rigId: string) {
  return dispatchControl({ command: 'rig.start', params: { rigId } });
}

function stopRig() {
  return dispatchControl({ command: 'rig.stop', params: {} });
}

async function openRigEditor(rigId: string, deviceId: string): Promise<void> {
  await dispatchControl({ command: 'rig.editor.open', params: { rigId, deviceId } });
}

export const rigApi: RigApi = {
  getRigState,
  createRig,
  updateRig,
  duplicateRig,
  deleteRig,
  addRigEffect,
  removeRigEffect,
  moveRigEffect,
  setRigEffectBypassed,
  startRig,
  stopRig,
  openRigEditor,
};
