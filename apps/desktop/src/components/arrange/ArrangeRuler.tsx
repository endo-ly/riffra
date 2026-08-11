import type { Marker, ProjectTimebase, TimelineLoopRange, TimelinePunchRange } from '@/lib/domain';
import {
  formatClock,
  ticksPerBar,
  ticksPerBeat,
  timelineGridDensity,
  TRACK_HEADER_WIDTH,
} from '@/lib/arrange-timeline';
import styles from './WorkspaceArrange.module.css';

type ArrangeRange = 'loop' | 'punch';

interface ArrangeRulerProps {
  timebase: ProjectTimebase;
  timelineTicks: number;
  timelineWidth: number;
  pixelsPerTick: number;
  mode: 'bars' | 'time';
  position: string;
  clock: string;
  scrollTop: number;
  loopRange: TimelineLoopRange;
  punchRange?: TimelinePunchRange;
  markers: Marker[];
  selectedMarkerId: string | null;
  selectedRange: ArrangeRange | null;
  timeSelection: { startTick: number; endTick: number } | null;
  onPointerDown: (event: React.PointerEvent<HTMLDivElement>) => void;
  onLoopHandle: (event: React.PointerEvent<HTMLSpanElement>, boundary: 'start' | 'end') => void;
  onPunchHandle?: (event: React.PointerEvent<HTMLSpanElement>, boundary: 'start' | 'end') => void;
  onSelectRange: (range: ArrangeRange) => void;
  onRulerContextMenu?: (event: React.MouseEvent<HTMLDivElement>, tick: number) => void;
  onRangeContextMenu?: (event: React.MouseEvent<HTMLDivElement>, range: ArrangeRange) => void;
  onMarkerContextMenu?: (event: React.MouseEvent, marker: Marker) => void;
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
  const density = timelineGridDensity(props.timebase, props.pixelsPerTick);
  const beatTicksInBar = barTicks / props.timebase.timeSignatureNumerator;
  const subdivisionOffsets = density.subdivisionTicks
    ? Array.from(
        { length: Math.floor((barTicks - 1) / density.subdivisionTicks) },
        (_, index) => (index + 1) * density.subdivisionTicks!,
      ).filter((offset) => offset % beatTicksInBar !== 0)
    : [];
  return (
    <>
      <div className={styles.rulerCorner} style={{ top: props.scrollTop }}>
        <div className={styles.rulerReadout}>
          <strong>{props.position}</strong>
          <small>{props.clock}</small>
        </div>
        <div className={styles.rulerMode}>
          <span>TRACKS</span>
          <small>{props.mode === 'bars' ? 'BARS + BEATS' : 'MIN : SEC'}</small>
        </div>
      </div>
      <div
        data-arrange-ruler
        className={styles.ruler}
        aria-label="Timeline ruler"
        style={{ left: TRACK_HEADER_WIDTH, top: props.scrollTop, width: props.timelineWidth }}
        onPointerDown={props.onPointerDown}
        onContextMenu={(event) => {
          if (
            props.onRulerContextMenu &&
            !(event.target as HTMLElement).closest('[data-marker-id], [data-range-handle]')
          ) {
            const bounds = event.currentTarget.getBoundingClientRect();
            const tick = Math.max(0, (event.clientX - bounds.left) / props.pixelsPerTick);
            props.onRulerContextMenu(event, tick);
          }
        }}
        onDoubleClick={(event) => {
          if (
            (event.target as HTMLElement).closest(
              '[data-marker-id], [data-range-band], [data-range-handle]',
            )
          )
            return;
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
        {props.punchRange && (
          <div
            className={`${styles.punchRange} ${
              props.selectedRange === 'punch' ? styles.rangeSelected : ''
            }`}
            style={{
              left: props.punchRange.startTick * props.pixelsPerTick,
              width: (props.punchRange.endTick - props.punchRange.startTick) * props.pixelsPerTick,
            }}
            data-range-band="punch"
            data-range-selected={props.selectedRange === 'punch' || undefined}
            onPointerDown={(event) => {
              event.preventDefault();
              event.stopPropagation();
              props.onSelectRange('punch');
            }}
            onContextMenu={(event) => {
              event.preventDefault();
              event.stopPropagation();
              props.onRangeContextMenu?.(event, 'punch');
            }}
          >
            <span className={styles.rangeLabel}>PUNCH</span>
            {props.onPunchHandle && (
              <>
                <span
                  data-range-handle
                  role="slider"
                  aria-label="Punch start"
                  className={`${styles.punchHandle} ${styles.punchHandleStart}`}
                  onPointerDown={(event) => {
                    props.onSelectRange('punch');
                    props.onPunchHandle?.(event, 'start');
                  }}
                />
                <span
                  data-range-handle
                  role="slider"
                  aria-label="Punch end"
                  className={`${styles.punchHandle} ${styles.punchHandleEnd}`}
                  onPointerDown={(event) => {
                    props.onSelectRange('punch');
                    props.onPunchHandle?.(event, 'end');
                  }}
                />
              </>
            )}
          </div>
        )}
        {props.loopRange.enabled && (
          <div
            className={`${styles.loopRange} ${
              props.selectedRange === 'loop' ? styles.rangeSelected : ''
            }`}
            style={{
              left: props.loopRange.startTick * props.pixelsPerTick,
              width: (props.loopRange.endTick - props.loopRange.startTick) * props.pixelsPerTick,
            }}
            data-range-band="loop"
            data-range-selected={props.selectedRange === 'loop' || undefined}
            onPointerDown={(event) => {
              event.preventDefault();
              event.stopPropagation();
              props.onSelectRange('loop');
            }}
            onContextMenu={(event) => {
              event.preventDefault();
              event.stopPropagation();
              props.onRangeContextMenu?.(event, 'loop');
            }}
          >
            <span className={styles.rangeLabel}>LOOP</span>
            <span
              data-range-handle
              role="slider"
              aria-label="Loop start"
              className={`${styles.loopHandle} ${styles.loopHandleStart}`}
              onPointerDown={(event) => {
                props.onSelectRange('loop');
                props.onLoopHandle(event, 'start');
              }}
            />
            <span
              data-range-handle
              role="slider"
              aria-label="Loop end"
              className={`${styles.loopHandle} ${styles.loopHandleEnd}`}
              onPointerDown={(event) => {
                props.onSelectRange('loop');
                props.onLoopHandle(event, 'end');
              }}
            />
          </div>
        )}
        {bars.map((bar) => {
          const tick = bar * barTicks;
          return (
            <div className={styles.barMark} key={bar} style={{ left: tick * props.pixelsPerTick }}>
              <strong>
                {bar % density.labelEveryBars === 0
                  ? props.mode === 'bars'
                    ? bar + 1
                    : formatClock(tick, props.timebase)
                  : null}
              </strong>
              {density.showBeats &&
                Array.from({ length: props.timebase.timeSignatureNumerator - 1 }, (_, beat) => (
                  <i key={beat} style={{ left: (beat + 1) * beatTicks * props.pixelsPerTick }} />
                ))}
              {subdivisionOffsets.map((offset) =>
                tick + offset < props.timelineTicks ? (
                  <i
                    key={offset}
                    className={styles.subdivisionMark}
                    style={{ left: offset * props.pixelsPerTick }}
                  />
                ) : null,
              )}
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
              if (props.onMarkerContextMenu) props.onMarkerContextMenu(event, marker);
              else props.onRemoveMarker(marker);
            }}
            title={`${marker.name} · right-click for options`}
          >
            <span>{marker.name}</span>
          </div>
        ))}
      </div>
    </>
  );
}
