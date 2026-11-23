import type { RigState } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { notify } from '@/shared/notify';

export interface RigSlice {
  state: RigState;
  load(): Promise<void>;
  accept(state: RigState): void;
  /** Creates a rig and resolves to its id, or `null` when the Host refused it. */
  create(name: string, channelIndex: number): Promise<string | null>;
  /** Resolves to whether the Host saved the patch. */
  update(
    rigId: string,
    patch: { name?: string; channelIndex?: number; gainDb?: number },
  ): Promise<boolean>;
  /** Duplicates a rig and resolves to the copy's id, or `null` when the Host refused it. */
  duplicate(rigId: string): Promise<string | null>;
  remove(rigId: string): Promise<void>;
  addEffect(rigId: string, pluginPath: string): Promise<void>;
  removeEffect(rigId: string, deviceId: string): Promise<void>;
  moveEffect(rigId: string, orderedDeviceIds: string[]): Promise<void>;
  setEffectBypassed(rigId: string, deviceId: string, bypassed: boolean): Promise<void>;
  start(rigId: string): Promise<void>;
  stop(): Promise<void>;
  openEditor(rigId: string, deviceId: string): Promise<void>;
  reset(): void;
}

const EMPTY: RigState = { rigs: [], activeRigId: null, lastPlayedRigId: null };

export function createRigSlice(
  api: NativeApi,
  get: () => RigSlice,
  update: (patch: Partial<RigSlice>) => void,
): RigSlice {
  let generation = 0;
  // Rig state arrives with the initial load and with `rig-state-changed`, so a
  // command result only reports success and never overwrites a newer event.
  const run = async <T>(failure: string, operation: () => Promise<T>): Promise<T | null> => {
    try {
      return await operation();
    } catch (cause) {
      notify.error(failure, { cause });
      return null;
    }
  };
  // The rig the result lists that the state before the command did not.
  const added = async (failure: string, operation: () => Promise<RigState>) => {
    const before = new Set(get().state.rigs.map((rig) => rig.id));
    const state = await run(failure, operation);
    return state?.rigs.find((rig) => !before.has(rig.id))?.id ?? null;
  };
  return {
    state: EMPTY,
    async load() {
      const current = generation;
      const state = await run('Rigs could not be loaded', () => api.getRigState());
      if (state && current === generation) update({ state });
    },
    accept: (state) => update({ state }),
    create: (name, channelIndex) =>
      added('Rig creation failed', () => api.createRig(name, channelIndex)),
    update: async (rigId, patch) =>
      (await run('Rig could not be saved', () => api.updateRig(rigId, patch))) !== null,
    duplicate: (rigId) => added('Rig duplication failed', () => api.duplicateRig(rigId)),
    async remove(rigId) {
      await run('Rig deletion failed', () => api.deleteRig(rigId));
    },
    async addEffect(rigId, pluginPath) {
      await run('Effect could not be added', () => api.addRigEffect(rigId, pluginPath));
    },
    async removeEffect(rigId, deviceId) {
      await run('Effect could not be removed', () => api.removeRigEffect(rigId, deviceId));
    },
    async moveEffect(rigId, orderedDeviceIds) {
      await run('Effects could not be reordered', () => api.moveRigEffect(rigId, orderedDeviceIds));
    },
    async setEffectBypassed(rigId, deviceId, bypassed) {
      await run('Bypass could not be changed', () =>
        api.setRigEffectBypassed(rigId, deviceId, bypassed),
      );
    },
    async start(rigId) {
      await run('Rig could not start', () => api.startRig(rigId));
    },
    async stop() {
      await run('Rig could not stop', () => api.stopRig());
    },
    async openEditor(rigId, deviceId) {
      await run('Plug-in editor could not open', () => api.openRigEditor(rigId, deviceId));
    },
    reset() {
      generation += 1;
      update({ state: EMPTY });
    },
  };
}
