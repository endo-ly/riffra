import { useCallback, useEffect, useRef, useState } from 'react';
import type { BackgroundJobStatus, JobState } from '@/model/domain';
import type { JobApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';

const terminalJobStates: readonly JobState[] = ['completed', 'failed', 'cancelled'];

export function useBackgroundJobs(
  api: Pick<JobApi, 'getBackgroundJob' | 'cancelBackgroundJob'>,
  hostGeneration = 0,
) {
  const [backgroundJob, setBackgroundJob] = useState<BackgroundJobStatus | null>(null);
  const activeJobId = useRef<string | null>(null);
  const mounted = useRef(false);
  const clearStatusTimer = useRef<number | null>(null);
  useEffect(() => {
    activeJobId.current = null;
    setBackgroundJob(null);
    if (clearStatusTimer.current !== null) {
      window.clearTimeout(clearStatusTimer.current);
      clearStatusTimer.current = null;
    }
  }, [hostGeneration]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (clearStatusTimer.current !== null) {
        window.clearTimeout(clearStatusTimer.current);
        clearStatusTimer.current = null;
      }
    };
  }, []);

  const runBackgroundJob = useCallback(
    async <J extends BackgroundJobStatus>(
      start: () => Promise<J>,
      onCompleted: (result: NonNullable<J['result']>) => void,
      onFailed: (message: string) => void,
    ): Promise<boolean> => {
      if (activeJobId.current) return false;
      let started: J;
      try {
        started = await start();
      } catch (error) {
        onFailed(error instanceof Error ? error.message : String(error));
        return false;
      }
      activeJobId.current = started.id;
      if (mounted.current) setBackgroundJob(started);
      let latest: J = started;
      try {
        while (!terminalJobStates.includes(latest.state)) {
          await new Promise((resolve) => window.setTimeout(resolve, 75));
          const next = await api.getBackgroundJob(started.id);
          if (!next) {
            onFailed('Background job disappeared before it reported a result.');
            return false;
          }
          latest = next as J;
          if (mounted.current) setBackgroundJob(next);
        }
        if (latest.state !== 'completed' || latest.result == null) {
          onFailed(
            latest.state === 'completed'
              ? 'Background job completed without a result.'
              : latest.message,
          );
          return false;
        }
        onCompleted(latest.result);
        return true;
      } catch (error) {
        onFailed(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        if (activeJobId.current === started.id) {
          activeJobId.current = null;
        }
        if (mounted.current) {
          if (clearStatusTimer.current !== null) {
            window.clearTimeout(clearStatusTimer.current);
          }
          clearStatusTimer.current = window.setTimeout(() => {
            clearStatusTimer.current = null;
            if (mounted.current) {
              setBackgroundJob((current) => (current?.id === started.id ? null : current));
            }
          }, 500);
        }
      }
    },
    [api],
  );

  const cancelActiveJob = useCallback(async () => {
    const id = activeJobId.current;
    if (!id) return;
    try {
      const status = await api.cancelBackgroundJob(id);
      if (status && mounted.current) {
        setBackgroundJob(status);
      }
    } catch (error) {
      logNativeError('cancelBackgroundJob')(error);
    }
  }, [api]);
  return { activeJobId, backgroundJob, runBackgroundJob, cancelActiveJob };
}
