import { listen } from '@tauri-apps/api/event';
import type { BootstrapState, RecoveryCandidate } from '@/model/domain';
import { defaultSession } from '../browser-defaults';
import {
  getHostGeneration,
  invokeHostOrFallback,
  isNativeRuntime,
  setHostConnectionAvailability,
  setHostGeneration,
} from '../invoke';
import type { RuntimeStartupFinishedEvent } from '../native-api';
import { defaultVst3Root } from './constants';

export async function bootstrap(): Promise<BootstrapState> {
  const session = defaultSession();
  const state = await invokeHostOrFallback<BootstrapState>(
    'get_bootstrap_state',
    {},
    {
      canonical: {
        session,
        sequence: 0,
        history: { canUndo: false, canRedo: false },
      },
      pluginCatalog: [],
      runtimeStarted: false,
      runtimeStartupFinished: false,
      recoveredFromGeneration: false,
      safeMode: false,
      nativeAvailable: false,
      recoveryCandidates: [] as RecoveryCandidate[],
      dataRoot: 'Browser preview \u2014 native persistence is unavailable',
      vst3Root: defaultVst3Root,
      hostConnection: {
        mode: 'disconnected',
        generation: 0,
        dataRoot: null,
        instanceId: null,
        pid: null,
        reason: 'Native Host is unavailable in browser preview',
      },
    },
  );
  if (state.hostConnection.generation >= getHostGeneration()) {
    setHostGeneration(state.hostConnection.generation);
    setHostConnectionAvailability(state.hostConnection.mode !== 'disconnected');
  }
  return state;
}

export async function onRuntimeStartupFinished(
  callback: (event: RuntimeStartupFinishedEvent) => void,
): Promise<() => void> {
  if (!isNativeRuntime()) return () => undefined;
  return listen<RuntimeStartupFinishedEvent>('runtime-startup-finished', ({ payload }) => {
    callback(payload);
  });
}
