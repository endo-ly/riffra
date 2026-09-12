import { useEffect } from 'react';
import type { RuntimeProjectionStatus } from '@/model/domain';
import { clearToast, showToast } from '@/shared/toasts';

interface UseArrangeStatusToastOptions {
  runtimeProjectionStatus: RuntimeProjectionStatus;
  runtimeProjectionFailure: string | null;
  runtimeProjectionRetrying: boolean;
  onRetryRuntimeProjection: () => Promise<void>;
  editorMessage: string;
  unavailableClipIds: string[];
  missingDeviceIds: string[];
}

export function useArrangeStatusToast({
  runtimeProjectionStatus,
  runtimeProjectionFailure,
  runtimeProjectionRetrying,
  onRetryRuntimeProjection,
  editorMessage,
  unavailableClipIds,
  missingDeviceIds,
}: UseArrangeStatusToastOptions) {
  // Runtime projection status, rather than Arrangement revision, is the source
  // of truth for playback health. Marker and other authoring-only edits still
  // advance the canonical revision without requiring a new audio graph.
  const projectionLoading =
    runtimeProjectionRetrying ||
    runtimeProjectionStatus.lastErrorCode === 'timelineBusy' ||
    runtimeProjectionStatus.state === 'queued' ||
    runtimeProjectionStatus.state === 'preparing';
  const playbackOutOfSync =
    runtimeProjectionStatus.state === 'failed' &&
    runtimeProjectionStatus.lastErrorCode !== 'timelineBusy';
  const unavailableClipCount = unavailableClipIds.length;
  const missingDeviceCount = missingDeviceIds.length;
  const statusMessage = projectionLoading
    ? runtimeProjectionRetrying
      ? 'Retrying audio preparation…'
      : 'Preparing audio…'
    : playbackOutOfSync
      ? (runtimeProjectionFailure ?? 'Audio preparation failed. Retry to prepare audio.')
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
      ...(playbackOutOfSync && !runtimeProjectionRetrying
        ? { action: { label: 'Retry', onClick: () => void onRetryRuntimeProjection() } }
        : {}),
    });
    return () => clearToast('arrange.status');
  }, [
    onRetryRuntimeProjection,
    playbackOutOfSync,
    runtimeProjectionRetrying,
    statusMessage,
    statusPersistent,
  ]);

  return { playbackOutOfSync };
}
