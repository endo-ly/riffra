import { useEffect, useMemo, useState } from 'react';
import type { MidiClip, MidiNote, ProjectTimebase } from '@/lib/domain';
import { snapGridTicks, ticksPerBar, ticksPerBeat } from '@/lib/arrange-timeline';
import type { SnapGrid } from '@/lib/arrange-timeline';
import { isBlackKey } from '@/lib/musical-typing';
import styles from './MidiEditorPanel.module.css';

interface MidiEditorPanelProps {
  clip: MidiClip | null;
  timebase: ProjectTimebase;
  onUpdateNote?: (clipId: string, note: MidiNote) => void;
  onUpdateNotes?: (clipId: string, updates: { noteId: string; patch: Partial<MidiNote> }[]) => void;
  onRemoveNote?: (clipId: string, noteId: string) => void;
  onAddNote?: (clipId: string, startTick: number, pitch: number) => void;
  onQuantize?: (clipId: string, noteIds: string[], gridTicks: number) => void;
  onDuplicateNotes?: (clipId: string, noteIds: string[], offsetTicks: number) => void;
}

const PITCH_HIGH = 96;
const PITCH_LOW = 24;

export function MidiEditorPanel(props: MidiEditorPanelProps) {
  const { clip, onRemoveNote } = props;
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [dragging, setDragging] = useState<{
    noteId: string;
    preview: MidiNote;
  } | null>(null);
  const [selectedNoteIds, setSelectedNoteIds] = useState<string[]>([]);
  const [pixelsPerTick, setPixelsPerTick] = useState(0.18);
  const [marquee, setMarquee] = useState<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);
  const rowHeight = 12;
  const visibleTicks = Math.max(props.clip?.durationTicks ?? 1920, 1920);
  const laneHeight = (PITCH_HIGH - PITCH_LOW) * rowHeight;
  const beatTicks = ticksPerBeat(props.timebase);
  const barTicks = ticksPerBar(props.timebase);
  const snapTicks = snapGridTicks(snap, props.timebase);

  const notes = props.clip?.notes ?? [];
  const pitchRows = useMemo(
    () => Array.from({ length: PITCH_HIGH - PITCH_LOW }, (_, index) => PITCH_LOW + index).reverse(),
    [],
  );

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key !== 'Delete' || !clip || selectedNoteIds.length === 0) return;
      event.preventDefault();
      for (const noteId of selectedNoteIds) onRemoveNote?.(clip.id, noteId);
      setSelectedNoteIds([]);
    };
    window.addEventListener('keydown', keydown);
    return () => window.removeEventListener('keydown', keydown);
  }, [clip, onRemoveNote, selectedNoteIds]);

  useEffect(() => {
    setSelectedNoteIds([]);
    setDragging(null);
    setMarquee(null);
  }, [clip?.id]);

  if (!props.clip) {
    return <div className={styles.empty}>Select a MIDI Clip and double-click to open it.</div>;
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
          snapTicks
            ? Math.round((note.startTick + deltaTicks) / snapTicks) * snapTicks
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
          snapTicks
            ? Math.round((note.durationTicks + deltaTicks) / snapTicks) * snapTicks
            : Math.round(note.durationTicks + deltaTicks),
        );
        preview = { ...note, durationTicks: nextDur };
      }
      setDragging((current) => (current ? { ...current, preview } : current));
    };
    let finish: () => void = () => undefined;
    let cancel: () => void = () => undefined;
    const cleanup = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
    };
    finish = () => {
      cleanup();
      if (
        preview.startTick !== note.startTick ||
        preview.note !== note.note ||
        preview.durationTicks !== note.durationTicks
      ) {
        if (mode === 'move' && selectedNoteIds.includes(note.id) && selectedNoteIds.length > 1) {
          const tickDelta = preview.startTick - note.startTick;
          const pitchDelta = preview.note - note.note;
          props.onUpdateNotes?.(
            props.clip!.id,
            props
              .clip!.notes.filter((candidate) => selectedNoteIds.includes(candidate.id))
              .map((candidate) => ({
                noteId: candidate.id,
                patch: {
                  startTick: Math.max(0, candidate.startTick + tickDelta),
                  note: Math.max(0, Math.min(127, candidate.note + pitchDelta)),
                },
              })),
          );
        } else {
          props.onUpdateNote?.(props.clip!.id, preview);
        }
      }
      setDragging(null);
    };
    cancel = () => {
      cleanup();
      setDragging(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
  };

  return (
    <div className={styles.editor} aria-label="MIDI Editor">
      <header className={styles.header}>
        <div className={styles.clipInfo}>
          <strong>{props.clip.name}</strong>
          <small>
            {props.clip.notes.length} notes · {Math.ceil(props.clip.durationTicks / barTicks)} bars
            {selectedNoteIds.length > 0 && ` · ${selectedNoteIds.length} selected`}
          </small>
        </div>
        <div className={styles.editorTools}>
          <label className={styles.control}>
            <span>Snap</span>
            <select value={snap} onChange={(event) => setSnap(event.target.value as SnapGrid)}>
              {['bar', '1/2', '1/4', '1/8', '1/8t', '1/16', '1/16t', '1/32', 'off'].map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </label>
          <div className={styles.zoomGroup} aria-label="MIDI Editor zoom">
            <span>Zoom</span>
            <button
              type="button"
              aria-label="Zoom out"
              onClick={() => setPixelsPerTick((value) => Math.max(0.05, value / 1.25))}
            >
              −
            </button>
            <button
              type="button"
              aria-label="Zoom in"
              onClick={() => setPixelsPerTick((value) => Math.min(1, value * 1.25))}
            >
              ＋
            </button>
          </div>
          <button
            type="button"
            className={styles.toolButton}
            disabled={!selectedNoteIds.length || snapTicks === 0}
            title={snapTicks === 0 ? 'Select a snap grid before quantizing' : undefined}
            onClick={() => props.onQuantize?.(props.clip!.id, selectedNoteIds, snapTicks)}
          >
            Quantize
          </button>
          <button
            type="button"
            className={styles.toolButton}
            disabled={!selectedNoteIds.length}
            onClick={() =>
              props.onDuplicateNotes?.(props.clip!.id, selectedNoteIds, snapTicks || 240)
            }
          >
            Duplicate
          </button>
          <label className={`${styles.control} ${styles.velocityControl}`}>
            <span>Velocity</span>
            <input
              aria-label="Selected MIDI note velocity"
              type="range"
              min="1"
              max="127"
              defaultValue="96"
              disabled={!selectedNoteIds.length}
              onPointerUp={(event) =>
                props.onUpdateNotes?.(
                  props.clip!.id,
                  selectedNoteIds.map((noteId) => ({
                    noteId,
                    patch: { velocity: Number(event.currentTarget.value) },
                  })),
                )
              }
            />
          </label>
        </div>
      </header>
      <div className={styles.laneViewport}>
        <div
          className={styles.lane}
          style={{ height: laneHeight, width: visibleTicks * pixelsPerTick }}
          onPointerDown={(event) => {
            const target = event.target as HTMLElement;
            if (target.closest('[data-note-id]')) return;
            const lane = event.currentTarget;
            const bounds = lane.getBoundingClientRect();
            const originX = event.clientX;
            const originY = event.clientY;
            const move = (pointer: PointerEvent) =>
              setMarquee({
                left: Math.min(originX, pointer.clientX) - bounds.left,
                top: Math.min(originY, pointer.clientY) - bounds.top,
                width: Math.abs(pointer.clientX - originX),
                height: Math.abs(pointer.clientY - originY),
              });
            let cancel: () => void = () => undefined;
            const finish = (pointer: PointerEvent) => {
              window.removeEventListener('pointermove', move);
              window.removeEventListener('pointerup', finish);
              window.removeEventListener('pointercancel', cancel);
              const width = Math.abs(pointer.clientX - originX);
              const height = Math.abs(pointer.clientY - originY);
              if (width < 4 && height < 4) {
                const tick = (originX - bounds.left) / pixelsPerTick;
                const pitch = PITCH_HIGH - Math.floor((originY - bounds.top) / rowHeight);
                if (pitch >= PITCH_LOW && pitch < PITCH_HIGH) {
                  const startTick = snapTicks
                    ? Math.round(tick / snapTicks) * snapTicks
                    : Math.max(0, Math.round(tick));
                  props.onAddNote?.(props.clip!.id, Math.max(0, startTick), pitch);
                }
              } else {
                const left = Math.min(originX, pointer.clientX);
                const right = Math.max(originX, pointer.clientX);
                const top = Math.min(originY, pointer.clientY);
                const bottom = Math.max(originY, pointer.clientY);
                const ids = [...lane.querySelectorAll<HTMLElement>('[data-note-id]')]
                  .filter((element) => {
                    const rect = element.getBoundingClientRect();
                    return (
                      rect.right >= left &&
                      rect.left <= right &&
                      rect.bottom >= top &&
                      rect.top <= bottom
                    );
                  })
                  .map((element) => element.dataset.noteId!)
                  .filter(Boolean);
                setSelectedNoteIds(
                  event.ctrlKey || event.shiftKey
                    ? [...new Set([...selectedNoteIds, ...ids])]
                    : ids,
                );
              }
              setMarquee(null);
            };
            cancel = () => {
              window.removeEventListener('pointermove', move);
              window.removeEventListener('pointerup', finish);
              window.removeEventListener('pointercancel', cancel);
              setMarquee(null);
            };
            window.addEventListener('pointermove', move);
            window.addEventListener('pointerup', finish);
            window.addEventListener('pointercancel', cancel);
          }}
        >
          {marquee && <div className={styles.marquee} style={marquee} />}
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
              className={`${styles.pitchRow} ${isBlackKey(pitch % 12) ? styles.pitchBlack : ''} ${pitch % 12 === 0 ? styles.pitchOctave : ''}`}
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
                  className={`${styles.note} ${dragging?.noteId === note.id ? styles.dragging : ''} ${selectedNoteIds.includes(note.id) ? styles.selected : ''}`}
                  style={{
                    left: visibleNote.startTick * pixelsPerTick,
                    top: (PITCH_HIGH - visibleNote.note - 1) * rowHeight,
                    width: Math.max(4, visibleNote.durationTicks * pixelsPerTick),
                    height: rowHeight - 1,
                    opacity: 0.4 + (note.velocity / 127) * 0.6,
                  }}
                  onPointerDown={(event) => handlePointerDown(event, note, 'move')}
                  onClick={(event) => {
                    event.stopPropagation();
                    setSelectedNoteIds((current) =>
                      event.ctrlKey || event.shiftKey
                        ? current.includes(note.id)
                          ? current.filter((id) => id !== note.id)
                          : [...current, note.id]
                        : [note.id],
                    );
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    props.onRemoveNote?.(props.clip!.id, note.id);
                    setSelectedNoteIds((current) => current.filter((id) => id !== note.id));
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
      </div>
    </div>
  );
}
