import { listen } from '@tauri-apps/api/event';
import type { BootstrapState, RuntimeStartupFinished } from '@/model/domain';
import { adoptHostConnection, invokeHost } from '../invoke';

export async function bootstrap(): Promise<BootstrapState> {
  const state = await invokeHost<BootstrapState>('get_bootstrap_state');
  adoptHostConnection(state.hostConnection);
  return state;
}

export async function onRuntimeStartupFinished(
  callback: (event: RuntimeStartupFinished) => void,
): Promise<() => void> {
  return listen<RuntimeStartupFinished>('runtime-startup-finished', ({ payload }) =>
    callback(payload),
  );
}
