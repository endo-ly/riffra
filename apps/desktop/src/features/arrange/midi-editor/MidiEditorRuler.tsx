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
 * Bars that start on a downbeat read as the bar number, matching the Arrange
 * ruler; a clip that starts mid-bar labels its marks with the beat as well.
 */
function barLabel(position: string): string {
  const [bar, beat] = position.split('.');
  return beat === '1' ? bar : `${bar}.${beat}`;
}

export function MidiEditorRuler(props: MidiEditorRulerProps) {
  const barTicks = ticksPerBar(props.timebase);
  const beatTicks = ticksPerBeat(props.timebase);
  const barCount = Math.ceil(props.visibleTicks / barTicks);

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
      {Array.from({ length: barCount }, (_, bar) => {
        const tick = bar * barTicks;
        const position = formatMusicalPosition(props.clipStartTick + tick, props.timebase);
        return (
          <i
            key={bar}
            className={styles.editorBarMark}
            style={{ left: tick * props.pixelsPerTick }}
          >
            <strong>{barLabel(position)}</strong>
            {Array.from({ length: props.timebase.timeSignatureNumerator - 1 }, (_, beat) => (
              <span key={beat} style={{ left: (beat + 1) * beatTicks * props.pixelsPerTick }} />
            ))}
          </i>
        );
      })}
      <i ref={props.playheadRef} className={styles.editorPlayhead} style={{ display: 'none' }} />
    </div>
  );
}
