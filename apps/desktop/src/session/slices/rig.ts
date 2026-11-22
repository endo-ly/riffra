import type { RigState } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { errorText } from '@/shared/errors';

export interface RigSlice {
  state: RigState;
  error: string | null;
  load(): Promise<void>;
  accept(state: RigState): void;
  create(name: string, channelIndex: number): Promise<boolean>;
  update(
    rigId: string,
    patch: { name?: string; channelIndex?: number; gainDb?: number },
  ): Promise<boolean>;
  duplicate(rigId: string): Promise<boolean>;
  remove(rigId: string): Promise<boolean>;
  addEffect(rigId: string, pluginPath: string): Promise<boolean>;
  removeEffect(rigId: string, deviceId: string): Promise<boolean>;
  moveEffect(rigId: string, orderedDeviceIds: string[]): Promise<boolean>;
  setEffectBypassed(rigId: string, deviceId: string, bypassed: boolean): Promise<boolean>;
  start(rigId: string): Promise<boolean>;
  stop(): Promise<boolean>;
  openEditor(rigId: string, deviceId: string): Promise<void>;
  reset(): void;
}

const EMPTY: RigState = { rigs: [], activeRigId: null };

export function createRigSlice(
  api: NativeApi,
  update: (patch: Partial<RigSlice>) => void,
): RigSlice {
  let generation = 0;
  // Rig state arrives with the initial load and with `rig-state-changed`.
  // A command result is only its success, so an in-flight command can never
  // overwrite a newer state event.
  const run = async (operation: () => Promise<RigState>): Promise<boolean> => {
    const current = generation;
    update({ error: null });
    try {
      await operation();
      return true;
    } catch (error) {
      if (current === generation) update({ error: errorText(error) });
      return false;
    }
  };
  return {
    state: EMPTY,
    error: null,
    async load() {
      const current = generation;
      update({ error: null });
      try {
        const state = await api.getRigState();
        if (current === generation) update({ state });
      } catch (error) {
        if (current === generation) update({ error: errorText(error) });
      }
    },
    accept: (state) => update({ state }),
    create: (name, channelIndex) => run(() => api.createRig(name, channelIndex)),
    update: (rigId, patch) => run(() => api.updateRig(rigId, patch)),
    duplicate: (rigId) => run(() => api.duplicateRig(rigId)),
    remove: (rigId) => run(() => api.deleteRig(rigId)),
    addEffect: (rigId, pluginPath) => run(() => api.addRigEffect(rigId, pluginPath)),
    removeEffect: (rigId, deviceId) => run(() => api.removeRigEffect(rigId, deviceId)),
    moveEffect: (rigId, orderedDeviceIds) => run(() => api.moveRigEffect(rigId, orderedDeviceIds)),
    setEffectBypassed: (rigId, deviceId, bypassed) =>
      run(() => api.setRigEffectBypassed(rigId, deviceId, bypassed)),
    start: (rigId) => run(() => api.startRig(rigId)),
    stop: () => run(() => api.stopRig()),
    async openEditor(rigId, deviceId) {
      const current = generation;
      update({ error: null });
      try {
        await api.openRigEditor(rigId, deviceId);
      } catch (error) {
        if (current === generation) update({ error: errorText(error) });
      }
    },
    reset() {
      generation += 1;
      update({ state: EMPTY, error: null });
    },
  };
}
