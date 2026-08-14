import { useState } from 'react';
import type { MidiNote } from '@/model/domain';
import { midiNoteName } from '@/features/arrange/play-surface/musical-typing';
import styles from './MidiEditorPanel.module.css';

interface MidiVelocityLaneProps {
  notes: MidiNote[];
  selectedNoteIds: string[];
  visibleTicks: number;
  pixelsPerTick: number;
  barTicks: number;
  beatTicks: number;
  height: number;
  playheadTick?: number;
  onSelectNoteIds: (noteIds: string[]) => void;
  onUpdateNotes?: (
    clipId: string,
    updates: { noteId: string; patch: Partial<MidiNote> }[],
  ) => void | PromiseLike<unknown>;
  clipId: string;
  onFocus?: () => void;
  onVelocityChange?: (value: number) => void;
}

export function MidiVelocityLane(props: MidiVelocityLaneProps) {
  const [preview, setPreview] = useState<Record<string, number>>({});

  const beginGesture = (event: React.PointerEvent<HTMLButtonElement>, note: MidiNote) => {
    event.preventDefault();
    event.stopPropagation();
    props.onFocus?.();
    const targetIds = props.selectedNoteIds.includes(note.id) ? props.selectedNoteIds : [note.id];
    if (!props.selectedNoteIds.includes(note.id)) props.onSelectNoteIds([note.id]);
    const originValues = new Map(
      props.notes
        .filter((candidate) => targetIds.includes(candidate.id))
        .map((candidate) => [candidate.id, candidate.velocity]),
    );
    const bounds = event.currentTarget.parentElement?.getBoundingClientRect();
    if (!bounds || originValues.size === 0) return;
    const valueAt = (clientY: number) =>
      Math.max(1, Math.min(127, Math.round(127 - ((clientY - bounds.top) / bounds.height) * 127)));
    const updatePreview = (clientY: number) => {
      const value = valueAt(clientY);
      setPreview(Object.fromEntries([...originValues.keys()].map((id) => [id, value])));
      props.onVelocityChange?.(value);
    };
    updatePreview(event.clientY);
    let finish: (pointer: PointerEvent) => void = () => undefined;
    const move = (pointer: PointerEvent) => updatePreview(pointer.clientY);
    const cancel = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      setPreview({});
    };
    finish = (pointer) => {
      updatePreview(pointer.clientY);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      const value = valueAt(pointer.clientY);
      const updates = [...originValues.entries()]
        .filter(([, original]) => original !== value)
        .map(([noteId]) => ({ noteId, patch: { velocity: value } }));
      setPreview({});
      if (updates.length) void Promise.resolve(props.onUpdateNotes?.(props.clipId, updates));
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
  };

  return (
    <div
      className={styles.velocityLane}
      data-velocity-lane
      style={{ width: props.visibleTicks * props.pixelsPerTick, height: props.height }}
      onPointerDown={(event) => {
        if (!(event.target as HTMLElement).closest('[data-velocity-note-id]')) {
          props.onSelectNoteIds([]);
        }
      }}
    >
      {Array.from({ length: Math.ceil(props.visibleTicks / props.barTicks) }, (_, bar) => (
        <i
          key={`bar-${bar}`}
          className={styles.velocityBarLine}
          style={{ left: bar * props.barTicks * props.pixelsPerTick }}
        />
      ))}
      {Array.from({ length: Math.ceil(props.visibleTicks / props.beatTicks) }, (_, beat) => (
        <i
          key={`beat-${beat}`}
          className={styles.velocityBeatLine}
          style={{ left: beat * props.beatTicks * props.pixelsPerTick }}
        />
      ))}
      {props.playheadTick !== undefined &&
        props.playheadTick >= 0 &&
        props.playheadTick <= props.visibleTicks && (
          <i
            className={styles.editorPlayhead}
            style={{ left: props.playheadTick * props.pixelsPerTick }}
          />
        )}
      {props.notes.map((note) => {
        const velocity = preview[note.id] ?? note.velocity;
        return (
          <button
            key={note.id}
            type="button"
            data-velocity-note-id={note.id}
            className={`${styles.velocityBar} ${props.selectedNoteIds.includes(note.id) ? styles.velocitySelected : ''}`}
            aria-label={`${midiNoteName(note.note)} velocity ${velocity}`}
            style={{
              left: note.startTick * props.pixelsPerTick,
              width: Math.max(4, note.durationTicks * props.pixelsPerTick),
              height: `${Math.max(2, (velocity / 127) * (props.height - 8))}px`,
            }}
            onPointerDown={(event) => beginGesture(event, note)}
          />
        );
      })}
    </div>
  );
}
