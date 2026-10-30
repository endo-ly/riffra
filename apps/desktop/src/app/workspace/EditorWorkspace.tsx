import { useEffect, type CSSProperties } from 'react';
import { useSession } from '@/shared/session/session-context';
import {
  selectHostReady,
  selectRuntimeReady,
  selectTrackInstrumentPlayable,
} from '@/shared/session/selectors';
import { isEditableTypingTarget } from '@/shared/input';
import { midiNoteName } from '@/shared/midi';
import { useLivePlay } from '@/features/live-input/useLivePlay';
import { TimelineView } from '@/features/timeline/TimelineView';
import { TRACK_HEADER_WIDTH } from '@/features/timeline/model/timeline-layout';
import { WorkspaceLowerArea } from './WorkspaceLowerArea';
import styles from './EditorWorkspace.module.css';

export function EditorWorkspace() {
  const session = useSession((state) => state.project.boot!.canonical.session);
  const workspace = useSession((state) => state.workspace);
  const hostReady = useSession(selectHostReady);
  const runtimeReady = useSession(selectRuntimeReady);
  const togglePlayback = useSession((state) => state.transport.togglePlayback);
  const track =
    session.arrangement.tracks.find((item) => item.id === workspace.focusedTrackId) ?? null;
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
  const { toggleComputerKeyboard } = livePlay;
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        if (!event.repeat) toggleComputerKeyboard();
        return;
      }
      if (event.key === ' ' && !isEditableTypingTarget(event.target)) {
        event.preventDefault();
        void togglePlayback();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [toggleComputerKeyboard, togglePlayback]);
  return (
    <section
      className={styles.workspace}
      aria-label="Arrange timeline"
      data-arrange-workspace
      style={{ '--header-width': `${TRACK_HEADER_WIDTH}px` } as CSSProperties}
    >
      <TimelineView
        liveInput={{
          on: livePlay.computerKeyboard,
          rangeLabel:
            livePlay.settings.mode === 'pads'
              ? 'Pads'
              : midiNoteName(livePlay.settings.octave * 12 + 12),
          blockedReason: livePlay.blockedReason,
          onToggle: toggleComputerKeyboard,
        }}
      />
      <WorkspaceLowerArea play={livePlay} />
    </section>
  );
}
