import { listen } from '@tauri-apps/api/event';
import type { BootstrapState, RecoveryCandidate } from '@/lib/domain';
import { defaultSession, defaultViewState } from '@/lib/domain';
import { invokeOrFallback, isNativeRuntime } from '../invoke';
import type { RuntimeStartupFinishedEvent } from '../native-api';
import { defaultVst3Root } from './constants';

export async function bootstrap(): Promise<BootstrapState> {
  return invokeOrFallback<BootstrapState>(
    'get_bootstrap_state',
    {},
    {
      session: defaultSession(),
      viewState: defaultViewState(),
      pluginCatalog: [],
      runtimeStarted: false,
      runtimeStartupFinished: false,
      recoveredFromGeneration: false,
      safeMode: false,
      nativeAvailable: false,
      recoveryCandidates: [] as RecoveryCandidate[],
      dataRoot: 'Browser preview \u2014 native persistence is unavailable',
      vst3Root: defaultVst3Root,
    },
  );
}

export async function onRuntimeStartupFinished(
  callback: (event: RuntimeStartupFinishedEvent) => void,
): Promise<() => void> {
  if (!isNativeRuntime()) return () => undefined;
  return listen<RuntimeStartupFinishedEvent>('runtime-startup-finished', ({ payload }) => {
    callback(payload);
  });
}
