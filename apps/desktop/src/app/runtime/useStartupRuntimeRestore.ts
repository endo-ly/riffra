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
  activeJobId: {
    current: string | null;
  };
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
  useEffect(() => {
    startupScanStarted.current = false;
    startupRuntimeRestoreAttempted.current = false;
  }, [hostGeneration]);

  const restoreRuntimeAfterScan = useCallback(async () => {
    if (startupRuntimeRestoreAttempted.current || runtimeStarted) return;
    startupRuntimeRestoreAttempted.current = true;
    try {
      const nextAudio = await retryStartupRuntime();
      setAudio(nextAudio);
    } catch {
      showToast(
        'vst3-scan',
        'Audio preparation after the plugin scan failed. Check the Arrange status and retry.',
        { kind: 'error' },
      );
    }
  }, [retryStartupRuntime, runtimeStarted, setAudio]);
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
