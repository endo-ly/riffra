import type { BackgroundJobStatus, JobState } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { errorText } from '@/shared/errors';
import { logNativeError } from '@/native/invoke';

const terminal: readonly JobState[] = ['completed', 'failed', 'cancelled'];

export interface JobSlice {
  active: BackgroundJobStatus | null;
  activeJobId: { readonly current: string | null };
  run<J extends BackgroundJobStatus>(
    start: () => Promise<J>,
    onCompleted: (result: NonNullable<J['result']>) => void,
    onFailed: (message: string) => void,
  ): Promise<boolean>;
  cancel(): Promise<void>;
  reset(): void;
}

export function createJobSlice(
  api: Pick<NativeApi, 'getBackgroundJob' | 'cancelBackgroundJob'>,
  update: (patch: Partial<JobSlice>) => void,
): JobSlice {
  let activeId: string | null = null;
  let starting = false;
  let generation = 0;
  let clearTimer: ReturnType<typeof setTimeout> | null = null;
  return {
    active: null,
    activeJobId: {
      get current() {
        return activeId;
      },
    },
    reset() {
      generation += 1;
      activeId = null;
      starting = false;
      if (clearTimer !== null) clearTimeout(clearTimer);
      clearTimer = null;
      update({ active: null });
    },
    async run<J extends BackgroundJobStatus>(
      start: () => Promise<J>,
      onCompleted: (result: NonNullable<J['result']>) => void,
      onFailed: (message: string) => void,
    ) {
      if (activeId || starting) return false;
      const currentGeneration = generation;
      starting = true;
      let started: J;
      try {
        started = await start();
      } catch (error) {
        if (generation === currentGeneration) onFailed(errorText(error));
        return false;
      } finally {
        if (generation === currentGeneration) starting = false;
      }
      if (generation !== currentGeneration) return false;
      if (clearTimer !== null) clearTimeout(clearTimer);
      clearTimer = null;
      activeId = started.id;
      update({ active: started });
      let latest = started;
      try {
        while (!terminal.includes(latest.state)) {
          await new Promise((resolve) => setTimeout(resolve, 75));
          if (generation !== currentGeneration) return false;
          const next = await api.getBackgroundJob(started.id);
          if (generation !== currentGeneration) return false;
          if (!next) {
            onFailed('Background job disappeared before it reported a result.');
            return false;
          }
          latest = next as J;
          update({ active: next });
        }
        if (latest.state !== 'completed' || latest.result == null) {
          onFailed(
            latest.state === 'completed'
              ? 'Background job completed without a result.'
              : latest.message,
          );
          return false;
        }
        onCompleted(latest.result as NonNullable<J['result']>);
        return true;
      } catch (error) {
        if (generation === currentGeneration) onFailed(errorText(error));
        return false;
      } finally {
        if (generation === currentGeneration) {
          if (activeId === started.id) activeId = null;
          if (clearTimer !== null) clearTimeout(clearTimer);
          clearTimer = setTimeout(() => {
            clearTimer = null;
            update({ active: null });
          }, 500);
        }
      }
    },
    async cancel() {
      if (!activeId) return;
      const currentGeneration = generation;
      try {
        const status = await api.cancelBackgroundJob(activeId);
        if (generation === currentGeneration && status) update({ active: status });
      } catch (error) {
        logNativeError('cancelBackgroundJob')(error);
      }
    },
  };
}
