import { useCallback, useEffect, useRef, useState } from 'react';
import {
  createStudioConsoleTheme,
  type ColorMode,
  type VisualSnapshot,
} from '@riffra/visual-renderer';
import { secondsToTicks } from '@/model/timebase';
import { useSession } from '@/session/session-context';
import { Toolbar, ToolbarSegmented } from '@/shared/ui/Toolbar';
import { useVisualSnapshot } from './useVisualSnapshot';
import styles from './ShowcaseWorkspace.module.css';

const COLOR_MODES: readonly { value: ColorMode; label: string }[] = [
  { value: 'harmony', label: 'Harmony' },
  { value: 'velocity', label: 'Velocity' },
  { value: 'track', label: 'Track' },
];

/** Fills the work area with the visualizer for the active Project. */
export function ShowcaseWorkspace() {
  const { snapshot, loading } = useVisualSnapshot();
  const colorMode = useSession((state) => state.workspace.showcaseColorMode);
  const setColorMode = useSession((state) => state.workspace.setShowcaseColorMode);

  return (
    <section className={styles.showcase} aria-label="Showcase" data-showcase-workspace>
      <Toolbar label="Showcase">
        <ToolbarSegmented
          label="Color Mode"
          value={colorMode}
          options={COLOR_MODES}
          onChange={setColorMode}
        />
      </Toolbar>
      {snapshot ? (
        <ShowcaseStage snapshot={snapshot} colorMode={colorMode} />
      ) : (
        !loading && (
          <div className={styles.empty}>
            <h2>Nothing to visualize yet</h2>
            <p>Add a clip to the arrangement, then return here to watch the music play.</p>
          </div>
        )
      )}
    </section>
  );
}

/** Mounts the theme and keeps it on the playback position. */
function ShowcaseStage({
  snapshot,
  colorMode,
}: {
  snapshot: VisualSnapshot;
  colorMode: ColorMode;
}) {
  const [theme] = useState(createStudioConsoleTheme);
  const containerRef = useRef<HTMLDivElement>(null);
  const initialSnapshot = useRef(snapshot);
  const playing = useSession((state) => state.transport.status?.state === 'playing');
  const displayTick = useSession((state) => state.transport.displayTick);
  const currentSeconds = useSession((state) => state.transport.currentSeconds);
  const seek = useSession((state) => state.transport.seek);
  const timebase = useSession(
    (state) => state.project.boot!.canonical.session.arrangement.timebase,
  );

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    theme.mount(container, initialSnapshot.current);
    return () => theme.unmount();
  }, [theme]);

  useEffect(() => {
    theme.updateSnapshot?.(snapshot);
  }, [theme, snapshot]);

  const draw = useCallback(() => {
    theme.render(snapshot, currentSeconds(timebase), {
      colorMode,
      showHeader: false,
      onSeek: (timeSec) => void seek(secondsToTicks(timeSec, timebase)),
    });
  }, [theme, snapshot, colorMode, currentSeconds, seek, timebase]);

  useEffect(() => {
    if (!playing) return;
    draw();
    let frame = requestAnimationFrame(function loop() {
      draw();
      frame = requestAnimationFrame(loop);
    });
    return () => cancelAnimationFrame(frame);
  }, [playing, draw]);

  useEffect(() => {
    if (playing) return;
    draw();
  }, [playing, displayTick, draw]);

  return <div ref={containerRef} className={styles.stage} />;
}
