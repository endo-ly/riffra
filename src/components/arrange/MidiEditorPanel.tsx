import { useMemo, useState } from 'react';
import type { MidiClip, MidiNote, ProjectTimebase } from '@/lib/domain';
import { snapGridTicks, ticksPerBar, ticksPerBeat } from '@/lib/arrange-timeline';
import type { SnapGrid } from '@/lib/arrange-timeline';
import styles from './MidiEditorPanel.module.css';

interface MidiEditorPanelProps {
  clip: MidiClip | null;
  timebase: ProjectTimebase;
  onClose: () => void;
  onUpdateNote?: (clipId: string, note: MidiNote) => void;
  onRemoveNote?: (clipId: string, noteId: string) => void;
  onAddNote?: (clipId: string, startTick: number, pitch: number) => void;
}

const PITCH_HIGH = 96;
const PITCH_LOW = 24;

export function MidiEditorPanel(props: MidiEditorPanelProps) {
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [dragging, setDragging] = useState<{
    noteId: string;
    preview: MidiNote;
  } | null>(null);
  const pixelsPerTick = 0.18;
  const rowHeight = 9;
  const visibleTicks = Math.max(props.clip?.durationTicks ?? 1920, 1920);
  const laneHeight = (PITCH_HIGH - PITCH_LOW) * rowHeight;
  const beatTicks = ticksPerBeat(props.timebase);
  const barTicks = ticksPerBar(props.timebase);

  const notes = props.clip?.notes ?? [];
  const pitchRows = useMemo(
    () => Array.from({ length: PITCH_HIGH - PITCH_LOW }, (_, index) => PITCH_LOW + index).reverse(),
    [],
  );

  if (!props.clip) {
    return (
      <aside className={styles.panel} aria-label="MIDI Editor">
        <header className={styles.header}>
          <strong>MIDI Editor</strong>
          <button onClick={props.onClose}>Close</button>
        </header>
        <div className={styles.empty}>Select a MIDI Clip and double-click to open it.</div>
      </aside>
    );
  }

  const handlePointerDown = (
    event: React.PointerEvent<HTMLSpanElement>,
    note: MidiNote,
    mode: 'move' | 'resize',
  ) => {
    event.stopPropagation();
    const originX = event.clientX;
    let preview = note;
    setDragging({
      noteId: note.id,
      preview: note,
    });
    const handle = event.currentTarget;
    handle.setPointerCapture?.(event.pointerId);
    const move = (pointer: PointerEvent) => {
      const deltaTicks = (pointer.clientX - originX) / pixelsPerTick;
      const parent = handle.parentElement!.getBoundingClientRect();
      const yInLane = pointer.clientY - parent.top;
      const pitchFromY = PITCH_HIGH - Math.floor(yInLane / rowHeight);
      if (mode === 'move') {
        const nextTick = Math.max(
          0,
          snapGridTicks(snap, props.timebase)
            ? Math.round((note.startTick + deltaTicks) / snapGridTicks(snap, props.timebase)) *
                snapGridTicks(snap, props.timebase)
            : Math.round(note.startTick + deltaTicks),
        );
        const nextPitch = Math.max(PITCH_LOW, Math.min(PITCH_HIGH - 1, pitchFromY));
        preview = {
          ...note,
          startTick: nextTick,
          note: nextPitch,
        };
      } else {
        const nextDur = Math.max(
          1,
          snapGridTicks(snap, props.timebase)
            ? Math.round((note.durationTicks + deltaTicks) / snapGridTicks(snap, props.timebase)) *
                snapGridTicks(snap, props.timebase)
            : Math.round(note.durationTicks + deltaTicks),
        );
        preview = { ...note, durationTicks: nextDur };
      }
      setDragging((current) => (current ? { ...current, preview } : current));
    };
    const finish = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      if (
        preview.startTick !== note.startTick ||
        preview.note !== note.note ||
        preview.durationTicks !== note.durationTicks
      )
        props.onUpdateNote?.(props.clip!.id, preview);
      setDragging(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
  };

  return (
    <aside className={styles.panel} aria-label="MIDI Editor">
      <header className={styles.header}>
        <strong>{props.clip.name}</strong>
        <small>
          {props.clip.notes.length} NOTES · {Math.ceil(props.clip.durationTicks / barTicks)} BARS
        </small>
        <label className={styles.snap}>
          SNAP
          <select value={snap} onChange={(event) => setSnap(event.target.value as SnapGrid)}>
            {['1/4', '1/8', '1/16', '1/32', 'off'].map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </select>
        </label>
        <button onClick={props.onClose}>Close</button>
      </header>
      <div
        className={styles.lane}
        style={{ height: laneHeight, width: visibleTicks * pixelsPerTick }}
        onPointerDown={(event) => {
          if (event.target !== event.currentTarget) return;
          const bounds = event.currentTarget.getBoundingClientRect();
          const tick = (event.clientX - bounds.left) / pixelsPerTick;
          const pitch = PITCH_HIGH - Math.floor((event.clientY - bounds.top) / rowHeight);
          if (pitch >= PITCH_LOW && pitch < PITCH_HIGH)
            props.onAddNote?.(props.clip!.id, Math.max(0, tick), pitch);
        }}
      >
        {Array.from({ length: Math.ceil(visibleTicks / barTicks) }, (_, bar) => (
          <i
            key={bar}
            className={styles.barLine}
            style={{ left: bar * barTicks * pixelsPerTick }}
          />
        ))}
        {Array.from({ length: Math.ceil(visibleTicks / beatTicks) }, (_, beat) => (
          <i
            key={beat}
            className={styles.beatLine}
            style={{ left: beat * beatTicks * pixelsPerTick }}
          />
        ))}
        {pitchRows.map((pitch) => (
          <div
            key={pitch}
            className={`${styles.pitchRow} ${pitch % 12 === 0 ? styles.pitchOctave : ''}`}
            style={{ top: (PITCH_HIGH - pitch - 1) * rowHeight }}
          >
            {pitch % 12 === 0 ? pitch : ''}
          </div>
        ))}
        {notes
          .filter((note) => note.note >= PITCH_LOW && note.note < PITCH_HIGH)
          .map((note) => {
            const visibleNote = dragging?.noteId === note.id ? dragging.preview : note;
            return (
              <span
                key={note.id}
                data-note-id={note.id}
                className={`${styles.note} ${dragging?.noteId === note.id ? styles.dragging : ''}`}
                style={{
                  left: visibleNote.startTick * pixelsPerTick,
                  top: (PITCH_HIGH - visibleNote.note - 1) * rowHeight,
                  width: Math.max(4, visibleNote.durationTicks * pixelsPerTick),
                  height: rowHeight - 1,
                  opacity: 0.4 + (note.velocity / 127) * 0.6,
                }}
                onPointerDown={(event) => handlePointerDown(event, note, 'move')}
                onContextMenu={(event) => {
                  event.preventDefault();
                  props.onRemoveNote?.(props.clip!.id, note.id);
                }}
              >
                <i
                  className={styles.resizeHandle}
                  onPointerDown={(event) => handlePointerDown(event, note, 'resize')}
                />
              </span>
            );
          })}
      </div>
    </aside>
  );
}
