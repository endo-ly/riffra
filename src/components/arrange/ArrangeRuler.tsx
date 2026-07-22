import type { ProjectTimebase, TimelineLoopRange } from '@/lib/domain';
import { formatClock, ticksPerBar, ticksPerBeat, TRACK_HEADER_WIDTH } from '@/lib/arrange-timeline';
import styles from './WorkspaceArrange.module.css';

interface ArrangeRulerProps {
  timebase: ProjectTimebase;
  timelineTicks: number;
  timelineWidth: number;
  pixelsPerTick: number;
  mode: 'bars' | 'time';
  loopRange: TimelineLoopRange;
  onPointerDown: (event: React.PointerEvent<HTMLDivElement>) => void;
}

export function ArrangeRuler(props: ArrangeRulerProps) {
  const barTicks = ticksPerBar(props.timebase);
  const beatTicks = ticksPerBeat(props.timebase);
  const bars = Array.from(
    { length: Math.ceil(props.timelineTicks / barTicks) },
    (_, index) => index,
  );
  const showBeats = beatTicks * props.pixelsPerTick >= 20;
  return (
    <>
      <div className={styles.rulerCorner}>
        <span>TRACKS</span>
        <small>{props.mode === 'bars' ? 'BARS + BEATS' : 'MIN : SEC'}</small>
      </div>
      <div
        data-arrange-ruler
        className={styles.ruler}
        aria-label="Timeline ruler"
        style={{ left: TRACK_HEADER_WIDTH, width: props.timelineWidth }}
        onPointerDown={props.onPointerDown}
      >
        {props.loopRange.enabled && (
          <div
            className={styles.loopRange}
            style={{
              left: props.loopRange.startTick * props.pixelsPerTick,
              width: (props.loopRange.endTick - props.loopRange.startTick) * props.pixelsPerTick,
            }}
          >
            <span>LOOP</span>
          </div>
        )}
        {bars.map((bar) => {
          const tick = bar * barTicks;
          return (
            <div className={styles.barMark} key={bar} style={{ left: tick * props.pixelsPerTick }}>
              <strong>{props.mode === 'bars' ? bar + 1 : formatClock(tick, props.timebase)}</strong>
              {showBeats &&
                Array.from({ length: props.timebase.timeSignatureNumerator - 1 }, (_, beat) => (
                  <i key={beat} style={{ left: (beat + 1) * beatTicks * props.pixelsPerTick }} />
                ))}
            </div>
          );
        })}
      </div>
    </>
  );
}
