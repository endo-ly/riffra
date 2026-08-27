import { useRef, useState } from 'react';
import type { Ref } from 'react';
import type { MidiNote } from '@/model/domain';
import { midiNoteName } from '@/features/arrange/play-surface/musical-typing';
import styles from './MidiEditorPanel.module.css';

const VELOCITY_DRAG_THRESHOLD = 3;

interface MidiVelocityLaneProps {
  notes: MidiNote[];
  selectedNoteIds: string[];
  visibleTicks: number;
  pixelsPerTick: number;
  barTicks: number;
  beatTicks: number;
  height: number;
  playheadRef: Ref<HTMLElement>;
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
  const previewGestureRef = useRef(0);

  const beginGesture = (event: React.PointerEvent<HTMLButtonElement>, note: MidiNote) => {
    event.preventDefault();
    event.stopPropagation();
    props.onFocus?.();
    const targetIds = props.selectedNoteIds.includes(note.id) ? props.selectedNoteIds : [note.id];
    if (!props.selectedNoteIds.includes(note.id)) props.onSelectNoteIds([note.id]);
    const originY = event.clientY;
    const originValues = new Map(
      props.notes
        .filter((candidate) => targetIds.includes(candidate.id))
        .map((candidate) => [candidate.id, candidate.velocity]),
    );
    const bounds = event.currentTarget.parentElement?.getBoundingClientRect();
    if (!bounds || originValues.size === 0) return;
    const gestureId = previewGestureRef.current + 1;
    previewGestureRef.current = gestureId;
    const clearPreview = () => {
      if (previewGestureRef.current === gestureId) setPreview({});
    };
    const valuesAt = (clientY: number) => {
      const delta = Math.round(((originY - clientY) / bounds.height) * 127);
      return Object.fromEntries(
        [...originValues.entries()].map(([id, original]) => [
          id,
          Math.max(1, Math.min(127, original + delta)),
        ]),
      );
    };
    const updatePreview = (clientY: number) => {
      const values = valuesAt(clientY);
      setPreview(values);
      props.onVelocityChange?.(values[note.id] ?? note.velocity);
    };
    let dragging = false;
    let finish: (pointer: PointerEvent) => void = () => undefined;
    const move = (pointer: PointerEvent) => {
      if (!dragging) {
        if (Math.abs(pointer.clientY - originY) < VELOCITY_DRAG_THRESHOLD) return;
        dragging = true;
      }
      updatePreview(pointer.clientY);
    };
    const cancel = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      clearPreview();
    };
    finish = (pointer) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      if (!dragging) {
        clearPreview();
        return;
      }
      const values = valuesAt(pointer.clientY);
      updatePreview(pointer.clientY);
      const updates = [...originValues.entries()]
        .filter(([noteId, original]) => original !== values[noteId])
        .map(([noteId]) => ({ noteId, patch: { velocity: values[noteId] } }));
      if (!updates.length) {
        clearPreview();
        return;
      }
      const operation = props.onUpdateNotes?.(props.clipId, updates);
      if (!operation) {
        clearPreview();
        return;
      }
      void Promise.resolve(operation).then(clearPreview, clearPreview);
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
      <i ref={props.playheadRef} className={styles.editorPlayhead} style={{ display: 'none' }} />
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
