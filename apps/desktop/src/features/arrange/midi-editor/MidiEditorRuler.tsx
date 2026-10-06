import type { Ref } from 'react';
import type { ProjectTimebase } from '@/model/domain';
import {
  formatMusicalPosition,
  ticksPerBar,
  ticksPerBeat,
  clipGridTicks,
} from '@/features/arrange/model/arrange-timeline';
import styles from './MidiEditorPanel.module.css';

interface MidiEditorRulerProps {
  timebase: ProjectTimebase;
  clipStartTick: number;
  visibleTicks: number;
  pixelsPerTick: number;
  playheadRef: Ref<HTMLElement>;
  onSeek?: (tick: number) => void;
}

/**
 * Downbeats read as the bar number; a mid-bar clip start includes the beat.
 */
function barLabel(tick: number, timebase: ProjectTimebase): string {
  const [bar, beat] = formatMusicalPosition(tick, timebase).split('.');
  return tick % ticksPerBar(timebase) === 0 ? bar : `${bar}.${beat}`;
}

export function MidiEditorRuler(props: MidiEditorRulerProps) {
  const barTicks = ticksPerBar(props.timebase);
  const beatTicks = ticksPerBeat(props.timebase);
  const barStarts =
    props.visibleTicks > 0
      ? [
          0,
          ...clipGridTicks(props.clipStartTick, props.visibleTicks, barTicks).filter(
            (tick) => tick > 0,
          ),
        ]
      : [];

  return (
    <div
      className={styles.midiRuler}
      aria-label="MIDI editor ruler"
      style={{ width: props.visibleTicks * props.pixelsPerTick }}
      onPointerDown={(event) => {
        const bounds = event.currentTarget.getBoundingClientRect();
        const localTick = Math.max(
          0,
          Math.min(props.visibleTicks, (event.clientX - bounds.left) / props.pixelsPerTick),
        );
        props.onSeek?.(props.clipStartTick + localTick);
      }}
    >
      {barStarts.map((tick, index) => {
        const beatMarks = [];
        const sectionEnd = barStarts[index + 1] ?? props.visibleTicks;
        for (const offset of clipGridTicks(
          props.clipStartTick + tick,
          sectionEnd - tick,
          beatTicks,
        )) {
          if (offset === 0) continue;
          beatMarks.push(<span key={offset} style={{ left: offset * props.pixelsPerTick }} />);
        }
        return (
          <i
            key={tick}
            className={styles.editorBarMark}
            style={{ left: tick * props.pixelsPerTick }}
          >
            <strong>{barLabel(props.clipStartTick + tick, props.timebase)}</strong>
            {beatMarks}
          </i>
        );
      })}
      <i ref={props.playheadRef} className={styles.editorPlayhead} style={{ display: 'none' }} />
    </div>
  );
}
