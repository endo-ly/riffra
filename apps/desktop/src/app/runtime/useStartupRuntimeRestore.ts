import { useCallback, useEffect, useRef } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, BackgroundJobStatus, BootstrapState } from '@/model/domain';
import { showToast } from '@/shared/toasts';

interface UseStartupRuntimeRestoreOptions {
  hostGeneration?: number;
  hostReady?: boolean;
  boot: BootstrapState | null;
  runtimeStarted: boolean;
  runtimeStartupFinished: boolean;
  activeJobId: { current: string | null };
  backgroundJob: BackgroundJobStatus | null;
  scanPlugins: () => Promise<boolean>;
  retryStartupRuntime: () => Promise<AudioStatus>;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
}

/** Restores the native runtime once after the startup plugin scan. */
export function useStartupRuntimeRestore({
  hostGeneration = 0,
  hostReady = true,
  boot,
  runtimeStarted,
  runtimeStartupFinished,
  activeJobId,
  backgroundJob,
  scanPlugins,
  retryStartupRuntime,
  setAudio,
}: UseStartupRuntimeRestoreOptions) {
  const startupScanStarted = useRef(false);
  const startupRuntimeRestoreAttempted = useRef(false);
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  useEffect(() => {
    startupScanStarted.current = false;
    startupRuntimeRestoreAttempted.current = false;
  }, [hostGeneration]);

  const restoreRuntimeAfterScan = useCallback(async () => {
    if (startupRuntimeRestoreAttempted.current || runtimeStarted) return;
    const requestGeneration = hostGeneration;
    startupRuntimeRestoreAttempted.current = true;
    try {
      const nextAudio = await retryStartupRuntime();
      if (currentHostGeneration.current === requestGeneration) setAudio(nextAudio);
    } catch (error) {
      if (currentHostGeneration.current !== requestGeneration) return;
      showToast(
        'vst3-scan',
        `Startup runtime restore failed after the catalog scan: ${
          error instanceof Error ? error.message : String(error)
        }`,
        { kind: 'error' },
      );
    }
  }, [hostGeneration, retryStartupRuntime, runtimeStarted, setAudio]);

  useEffect(() => {
    if (
      startupScanStarted.current ||
      activeJobId.current ||
      backgroundJob != null ||
      !hostReady ||
      !boot?.nativeAvailable ||
      boot.safeMode ||
      !runtimeStartupFinished
    ) {
      return;
    }
    startupScanStarted.current = true;
    void (async () => {
      if (await scanPlugins()) await restoreRuntimeAfterScan();
    })();
  }, [
    activeJobId,
    backgroundJob,
    boot,
    hostReady,
    restoreRuntimeAfterScan,
    runtimeStartupFinished,
    scanPlugins,
  ]);
}
