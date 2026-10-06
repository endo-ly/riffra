import type { Ref } from 'react';
import type { ProjectTimebase } from '@/model/domain';
import {
  formatMusicalPosition,
  ticksPerBar,
  ticksPerBeat,
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
  const endTick = props.clipStartTick + props.visibleTicks;
  const barStarts = props.visibleTicks > 0 ? [props.clipStartTick] : [];
  for (
    let tick = (Math.floor(props.clipStartTick / barTicks) + 1) * barTicks;
    tick < endTick;
    tick += barTicks
  ) {
    barStarts.push(tick);
  }

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
        const sectionEnd = barStarts[index + 1] ?? endTick;
        for (
          let beat = (Math.floor(tick / beatTicks) + 1) * beatTicks;
          beat < sectionEnd;
          beat += beatTicks
        ) {
          beatMarks.push(<span key={beat} style={{ left: (beat - tick) * props.pixelsPerTick }} />);
        }
        return (
          <i
            key={tick}
            className={styles.editorBarMark}
            style={{ left: (tick - props.clipStartTick) * props.pixelsPerTick }}
          >
            <strong>{barLabel(tick, props.timebase)}</strong>
            {beatMarks}
          </i>
        );
      })}
      <i ref={props.playheadRef} className={styles.editorPlayhead} style={{ display: 'none' }} />
    </div>
  );
}
