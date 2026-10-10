import type {
  Marker,
  ProjectTimebase,
  TimelineLoopRange,
  TimelinePunchRange,
} from '@/model/domain';
import {
  formatClock,
  timelineGridDensity,
  TRACK_HEADER_WIDTH,
} from '@/features/arrange/model/arrange-timeline';
import { meterSegment } from '@/shared/session/timebase';
import styles from '../WorkspaceArrange.module.css';

type ArrangeRange = 'loop' | 'punch';

interface ArrangeRulerProps {
  timebase: ProjectTimebase;
  timelineTicks: number;
  timelineWidth: number;
  pixelsPerTick: number;
  mode: 'bars' | 'time';
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
  const bars: { tick: number; bar: number; end: number; beatTicks: number; numerator: number }[] =
    [];
  for (let tick = 0; tick < props.timelineTicks;) {
    const meter = meterSegment(props.timebase, { tick });
    const length = meter.beatTicks * meter.numerator;
    const nextChange = props.timebase.timeSignatureChanges.find(
      (change) =>
        change.tick > tick &&
        (change.numerator !== meter.numerator || change.denominator !== meter.denominator),
    );
    const end = Math.min(tick + length, nextChange?.tick ?? Infinity, props.timelineTicks);
    bars.push({
      tick,
      bar: meter.firstBar + Math.floor((tick - meter.tick) / length),
      end,
      beatTicks: meter.beatTicks,
      numerator: meter.numerator,
    });
    tick = end;
  }
  const density = timelineGridDensity(props.timebase, props.pixelsPerTick);
  return (
    <>
      <div className={styles.rulerCorner}>
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
        {bars.map(({ tick, bar, end, beatTicks, numerator }) => {
          const subdivisionOffsets: number[] = [];
          if (density.subdivisionTicks)
            for (
              let offset = density.subdivisionTicks;
              tick + offset < end;
              offset += density.subdivisionTicks
            )
              if (offset % beatTicks !== 0) subdivisionOffsets.push(offset);
          return (
            <div className={styles.barMark} key={bar} style={{ left: tick * props.pixelsPerTick }}>
              <strong>
                {(bar - 1) % density.labelEveryBars === 0
                  ? props.mode === 'bars'
                    ? bar
                    : formatClock(tick, props.timebase)
                  : null}
              </strong>
              {density.showBeats &&
                Array.from({ length: numerator - 1 }, (_, beat) => (beat + 1) * beatTicks)
                  .filter((offset) => tick + offset < end)
                  .map((offset) => (
                    <i key={offset} style={{ left: offset * props.pixelsPerTick }} />
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
