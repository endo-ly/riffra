import type { BootstrapState, RuntimeStartupFinished } from '@/model/generated';
import { listen } from '@tauri-apps/api/event';
import { adoptHostConnection, invokeHost } from '../invoke';

export interface BootstrapApi {
  bootstrap(): Promise<BootstrapState>;
  onRuntimeStartupFinished(callback: (event: RuntimeStartupFinished) => void): Promise<() => void>;
}

async function bootstrap(): Promise<BootstrapState> {
  const state = await invokeHost<BootstrapState>('get_bootstrap_state');
  adoptHostConnection(state.hostConnection);
  return state;
}

async function onRuntimeStartupFinished(
  callback: (event: RuntimeStartupFinished) => void,
): Promise<() => void> {
  return listen<RuntimeStartupFinished>('runtime-startup-finished', ({ payload }) =>
    callback(payload),
  );
}

export const bootstrapApi: BootstrapApi = {
  bootstrap,
  onRuntimeStartupFinished,
};
