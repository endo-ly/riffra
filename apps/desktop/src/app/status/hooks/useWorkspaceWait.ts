import type { Track } from '@/model/generated';
import { useSession } from '@/session/session-context';
import type { WaitCardProps } from '@/shared/ui/message/WaitCard';
import { instrumentLoading, type InstrumentLoading } from '@/app/status/model/runtime-status';

const NO_TRACKS: readonly Track[] = [];

/** How far the device load has come, whichever wait it belongs to. */
function loadingProgress(
  loading: InstrumentLoading,
): Pick<WaitCardProps, 'progress' | 'detail' | 'stalled'> {
  return {
    progress: loading.total
      ? {
          ratio: loading.completed / loading.total,
          label: `${loading.completed} / ${loading.total}`,
        }
      : undefined,
    detail: `${loading.current} · ${loading.elapsedSeconds}s`,
    stalled: loading.stalledOn ? `${loading.stalledOn} is taking longer than usual.` : undefined,
  };
}

/**
 * Describes why editing waits. The reason comes from an update install, the
 * Host connection, an opening Project, or the device load, in that order; the
 * device load also reports its progress inside an opening Project, because
 * opening one returns only after its instruments are loaded.
 */
export function useWorkspaceWait(): WaitCardProps | null {
  const installingUpdate = useSession((state) => state.appUpdate.installing);
  const host = useSession((state) => state.host);
  const projectSwitching = useSession((state) => state.project.switching);
  const projection = useSession((state) => state.runtime.projection);
  const tracks = useSession(
    (state) => state.project.boot?.canonical.session.arrangement.tracks ?? NO_TRACKS,
  );
  // Installing shuts the Host down, so the Host states it causes are not
  // reasons of their own.
  if (installingUpdate) {
    return {
      title: 'Installing update',
      message: 'Riffra restarts when the update is installed.',
    };
  }
  if (host.switching) {
    return {
      title: 'Connecting to Host',
      message: 'Editing resumes when the Host is ready.',
    };
  }
  if (host.connection.mode === 'disconnected') {
    return {
      title: 'Host disconnected',
      message: 'Reconnect from the Host menu to continue.',
      waiting: false,
      detail: host.connection.reason ?? undefined,
    };
  }
  const loading = instrumentLoading(projection, tracks);
  const progress = loading ? loadingProgress(loading) : {};
  if (projectSwitching) {
    return {
      title: 'Opening project',
      message: 'Editing resumes when the project and its instruments are ready.',
      ...progress,
    };
  }
  if (!loading) return null;
  return {
    title: 'Loading instruments',
    message: 'Editing resumes when every instrument is ready.',
    ...progress,
  };
}
