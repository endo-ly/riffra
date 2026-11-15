import { useEffect } from 'react';
import { useSession } from '@/shared/session/session-context';
import { clearToast, showToast } from '@/shared/toasts';

export function useRuntimeProjectionToast() {
  const runtimeProjectionStatus = useSession((state) => state.runtime.projection);
  const tracks = useSession((state) => state.project.boot?.canonical.session.arrangement.tracks);
  const preparation = runtimeProjectionStatus.loadProgress;
  const stage = preparation?.stage;
  const ownerName = tracks?.find((track) => track.id === stage?.ownerId)?.name;
  const pluginName = stage?.pluginPath
    .split(/[\\/]/)
    .pop()
    ?.replace(/\.vst3$/i, '');
  const deviceName = [ownerName, pluginName].filter(Boolean).join(' · ');
  const phaseLabels = {
    dispatching: 'Waiting for the audio engine',
    queued: 'Waiting to prepare audio',
    creating: 'Loading',
    preparing: 'Preparing',
    restoring: 'Restoring',
    deviceReady: 'Loaded',
    routing: 'Preparing routing',
    prepared: 'Activating audio',
  } as const;
  const preparationMessage =
    preparation && stage
      ? `${preparation.stalled ? 'Still waiting · ' : ''}${phaseLabels[stage.phase]}${deviceName ? ` · ${deviceName}` : ''}${stage.totalDevices ? ` · ${stage.completedDevices}/${stage.totalDevices}` : ''} · ${Math.floor(preparation.elapsedMs / 1000)}s`
      : null;
  const runtimeProjectionFailure = useSession((state) => state.runtime.projectionFailure);
  const runtimeProjectionRetrying = useSession((state) => state.runtime.projectionRetrying);
  const onRetryRuntimeProjection = useSession((state) => state.runtime.retryProjection);
  // Runtime projection status, rather than Arrangement revision, is the source
  // of truth for playback health. Marker and other authoring-only edits still
  // advance the canonical revision without requiring a new audio graph.
  const projectionLoading =
    preparation != null ||
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
    ? (preparationMessage ??
      (runtimeProjectionRetrying ? 'Retrying audio preparation…' : 'Preparing audio…'))
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
