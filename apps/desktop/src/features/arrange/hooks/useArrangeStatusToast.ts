import { useEffect } from 'react';
import type { RuntimeProjectionStatus } from '@/model/domain';
import { clearToast, showToast } from '@/shared/toasts';

interface UseArrangeStatusToastOptions {
  runtimeProjectionStatus: RuntimeProjectionStatus;
  runtimeProjectionFailure: string | null;
  onRetryRuntimeProjection: () => Promise<void>;
  editorMessage: string;
  unavailableClipIds: string[];
  missingDeviceIds: string[];
}

export function useArrangeStatusToast({
  runtimeProjectionStatus,
  runtimeProjectionFailure,
  onRetryRuntimeProjection,
  editorMessage,
  unavailableClipIds,
  missingDeviceIds,
}: UseArrangeStatusToastOptions) {
  // Runtime projection status, rather than Arrangement revision, is the source
  // of truth for playback health. Marker and other authoring-only edits still
  // advance the canonical revision without requiring a new audio graph.
  const playbackOutOfSync =
    runtimeProjectionStatus.state === 'failed' || runtimeProjectionFailure !== null;
  const unavailableClipCount = unavailableClipIds.length;
  const missingDeviceCount = missingDeviceIds.length;
  const statusMessage = playbackOutOfSync
    ? (runtimeProjectionFailure ??
      runtimeProjectionStatus.lastError ??
      'Playback runtime is out of sync')
    : unavailableClipCount || missingDeviceCount
      ? `Playback skipped ${unavailableClipCount} missing source${unavailableClipCount === 1 ? '' : 's'} and ${missingDeviceCount} missing device${missingDeviceCount === 1 ? '' : 's'}.`
      : editorMessage;
  const statusPersistent = playbackOutOfSync || unavailableClipCount > 0 || missingDeviceCount > 0;

  useEffect(() => {
    if (!statusMessage) {
      clearToast('arrange.status');
      return;
    }
    showToast('arrange.status', statusMessage, {
      kind: playbackOutOfSync ? 'error' : 'info',
      persistent: statusPersistent,
      ...(playbackOutOfSync
        ? { action: { label: 'Retry', onClick: () => void onRetryRuntimeProjection() } }
        : {}),
    });
    return () => clearToast('arrange.status');
  }, [playbackOutOfSync, onRetryRuntimeProjection, statusMessage, statusPersistent]);

  return { playbackOutOfSync };
}
