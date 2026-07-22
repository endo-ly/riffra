import type { Marker, ProjectTimebase, TimelineLoopRange } from '@/lib/domain';
import { formatClock, ticksPerBar, ticksPerBeat, TRACK_HEADER_WIDTH } from '@/lib/arrange-timeline';
import styles from './WorkspaceArrange.module.css';

interface ArrangeRulerProps {
  timebase: ProjectTimebase;
  timelineTicks: number;
  timelineWidth: number;
  pixelsPerTick: number;
  mode: 'bars' | 'time';
  scrollTop: number;
  loopRange: TimelineLoopRange;
  markers: Marker[];
  selectedMarkerId: string | null;
  timeSelection: { startTick: number; endTick: number } | null;
  onPointerDown: (event: React.PointerEvent<HTMLDivElement>) => void;
  onLoopHandle: (event: React.PointerEvent<HTMLSpanElement>, boundary: 'start' | 'end') => void;
  onAddMarker: (tick: number) => void;
  onMoveMarker: (marker: Marker, tick: number) => void;
  onRenameMarker: (marker: Marker) => void;
  onRemoveMarker: (marker: Marker) => void;
  onSelectMarker: (markerId: string | null) => void;
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
      <div className={styles.rulerCorner} style={{ top: props.scrollTop }}>
        <span>TRACKS</span>
        <small>{props.mode === 'bars' ? 'BARS + BEATS' : 'MIN : SEC'}</small>
      </div>
      <div
        data-arrange-ruler
        className={styles.ruler}
        aria-label="Timeline ruler"
        style={{ left: TRACK_HEADER_WIDTH, top: props.scrollTop, width: props.timelineWidth }}
        onPointerDown={props.onPointerDown}
        onDoubleClick={(event) => {
          if ((event.target as HTMLElement).closest('[data-marker-id]')) return;
          const bounds = event.currentTarget.getBoundingClientRect();
          const tick = Math.max(0, (event.clientX - bounds.left) / props.pixelsPerTick);
          props.onAddMarker(tick);
        }}
      >
        {props.timeSelection && (
          <div
            className={styles.timeSelection}
            style={{
              left: props.timeSelection.startTick * props.pixelsPerTick,
              width:
                Math.max(1, props.timeSelection.endTick - props.timeSelection.startTick) *
                props.pixelsPerTick,
            }}
          />
        )}
        {props.loopRange.enabled && (
          <div
            className={styles.loopRange}
            style={{
              left: props.loopRange.startTick * props.pixelsPerTick,
              width: (props.loopRange.endTick - props.loopRange.startTick) * props.pixelsPerTick,
            }}
          >
            <span>LOOP</span>
            <span
              data-loop-handle
              className={`${styles.loopHandle} ${styles.loopHandleStart}`}
              onPointerDown={(event) => props.onLoopHandle(event, 'start')}
            />
            <span
              data-loop-handle
              className={`${styles.loopHandle} ${styles.loopHandleEnd}`}
              onPointerDown={(event) => props.onLoopHandle(event, 'end')}
            />
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
        {props.markers.map((marker) => (
          <div
            key={marker.id}
            data-marker-id={marker.id}
            className={`${styles.marker} ${props.selectedMarkerId === marker.id ? styles.markerSelected : ''}`}
            style={{ left: marker.tick * props.pixelsPerTick }}
            onPointerDown={(event) => {
              event.stopPropagation();
              props.onSelectMarker(marker.id);
              const handle = event.currentTarget;
              const originX = event.clientX;
              const originTick = marker.tick;
              handle.setPointerCapture?.(event.pointerId);
              const move = (pointer: PointerEvent) => {
                const next = Math.max(
                  0,
                  originTick + (pointer.clientX - originX) / props.pixelsPerTick,
                );
                handle.style.left = `${next * props.pixelsPerTick}px`;
              };
              const finish = (pointer: PointerEvent) => {
                handle.removeEventListener('pointermove', move);
                handle.removeEventListener('pointerup', finish);
                const next = Math.max(
                  0,
                  Math.round(originTick + (pointer.clientX - originX) / props.pixelsPerTick),
                );
                if (next !== originTick) props.onMoveMarker(marker, next);
              };
              handle.addEventListener('pointermove', move);
              handle.addEventListener('pointerup', finish);
            }}
            onDoubleClick={(event) => {
              event.stopPropagation();
              props.onRenameMarker(marker);
            }}
            onContextMenu={(event) => {
              event.preventDefault();
              event.stopPropagation();
              props.onRemoveMarker(marker);
            }}
            title={`${marker.name} · right-click to delete`}
          >
            <span>{marker.name}</span>
          </div>
        ))}
      </div>
    </>
  );
}
