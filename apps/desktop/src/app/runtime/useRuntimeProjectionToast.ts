import { useEffect } from 'react';
import { useSession } from '@/shared/session/session-context';
import { clearToast, showToast } from '@/shared/toasts';

export function useRuntimeProjectionToast() {
  const runtime = useSession((state) => state.runtime);
  const {
    projection: runtimeProjectionStatus,
    projectionFailure: runtimeProjectionFailure,
    projectionRetrying: runtimeProjectionRetrying,
    retryProjection: onRetryRuntimeProjection,
  } = runtime;
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
  const unavailableClipCount =
    runtimeProjectionStatus.activeDiagnostics?.unavailableClipIds.length ?? 0;
  const missingDeviceCount =
    runtimeProjectionStatus.activeDiagnostics?.missingDeviceIds.length ?? 0;
  const statusMessage = projectionLoading
    ? runtimeProjectionRetrying
      ? 'Retrying audio preparation…'
      : 'Preparing audio…'
    : playbackOutOfSync
      ? (runtimeProjectionFailure ?? 'Audio preparation failed. Retry to prepare audio.')
      : unavailableClipCount || missingDeviceCount
        ? `Playback skipped ${unavailableClipCount} missing source${unavailableClipCount === 1 ? '' : 's'} and ${missingDeviceCount} missing device${missingDeviceCount === 1 ? '' : 's'}.`
        : null;
  const statusPersistent =
    projectionLoading || playbackOutOfSync || unavailableClipCount > 0 || missingDeviceCount > 0;

  useEffect(() => {
    if (!statusMessage) {
      clearToast('runtime.projection');
      return;
    }
    showToast('runtime.projection', statusMessage, {
      kind: playbackOutOfSync ? 'error' : 'info',
      persistent: statusPersistent,
      ...(playbackOutOfSync && !runtimeProjectionRetrying
        ? { action: { label: 'Retry', onClick: () => void onRetryRuntimeProjection() } }
        : {}),
    });
    return () => clearToast('runtime.projection');
  }, [
    onRetryRuntimeProjection,
    playbackOutOfSync,
    runtimeProjectionRetrying,
    statusMessage,
    statusPersistent,
  ]);

  return { playbackOutOfSync };
}
