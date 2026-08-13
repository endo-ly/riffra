import { useCallback, useEffect, useRef } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, BackgroundJobStatus, BootstrapState } from '@/model/domain';
import { showToast } from '@/shared/toasts';

interface UseStartupRuntimeRecoveryOptions {
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
export function useStartupRuntimeRecovery({
  boot,
  runtimeStarted,
  runtimeStartupFinished,
  activeJobId,
  backgroundJob,
  scanPlugins,
  retryStartupRuntime,
  setAudio,
}: UseStartupRuntimeRecoveryOptions) {
  const startupScanStarted = useRef(false);
  const startupRuntimeRecoveryAttempted = useRef(false);

  const retryRuntimeAfterScan = useCallback(async () => {
    if (startupRuntimeRecoveryAttempted.current || runtimeStarted) return;
    startupRuntimeRecoveryAttempted.current = true;
    try {
      setAudio(await retryStartupRuntime());
    } catch (error) {
      showToast(
        'vst3-scan',
        `Startup runtime restore failed after the catalog scan: ${
          error instanceof Error ? error.message : String(error)
        }`,
        { kind: 'error' },
      );
    }
  }, [retryStartupRuntime, runtimeStarted, setAudio]);

  useEffect(() => {
    if (
      startupScanStarted.current ||
      activeJobId.current ||
      backgroundJob != null ||
      !boot?.nativeAvailable ||
      boot.safeMode ||
      !runtimeStartupFinished
    ) {
      return;
    }
    startupScanStarted.current = true;
    void (async () => {
      if (await scanPlugins()) await retryRuntimeAfterScan();
    })();
  }, [
    activeJobId,
    backgroundJob,
    boot,
    retryRuntimeAfterScan,
    runtimeStartupFinished,
    scanPlugins,
  ]);
}
