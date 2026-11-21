import { useRef, type CSSProperties } from 'react';
import { useSession } from '@/session/session-context';
import {
  selectHostReady,
  selectRuntimeReady,
  selectTrackInstrumentPlayable,
  selectTrack,
} from '@/session/selectors';
import { useLivePlay } from '@/features/live-input/useLivePlay';
import { LiveInputToggle } from '@/features/live-input/LiveInputToggle';
import { TimelineView } from '@/features/timeline/TimelineView';
import { TRACK_HEADER_WIDTH } from '@/features/timeline/model/timeline-layout';
import { useWorkspaceShortcuts } from './hooks/useWorkspaceShortcuts';
import { WorkspaceLowerArea } from './WorkspaceLowerArea';
import styles from './EditorWorkspace.module.css';

export function EditorWorkspace() {
  const workspaceRef = useRef<HTMLElement>(null);
  const workspace = useSession((state) => state.workspace);
  const hostReady = useSession(selectHostReady);
  const runtimeReady = useSession(selectRuntimeReady);
  const track = useSession((state) => selectTrack(state, workspace.focusedTrackId));
  const playable = useSession((state) =>
    selectTrackInstrumentPlayable(state, workspace.focusedTrackId),
  );
  const livePlay = useLivePlay({
    track,
    runtimeReady,
    hostConnected: hostReady,
    playable,
    surfaceOpen: workspace.lower.view === 'playSurface' && !workspace.lower.collapsed,
  });
  useWorkspaceShortcuts(livePlay.toggleComputerKeyboard);
  return (
    <section
      ref={workspaceRef}
      className={styles.workspace}
      aria-label="Arrange timeline"
      data-arrange-workspace
      style={{ '--header-width': `${TRACK_HEADER_WIDTH}px` } as CSSProperties}
    >
      <TimelineView toolbarAccessory={<LiveInputToggle play={livePlay} />} />
      <WorkspaceLowerArea play={livePlay} workspaceRef={workspaceRef} />
    </section>
  );
}
