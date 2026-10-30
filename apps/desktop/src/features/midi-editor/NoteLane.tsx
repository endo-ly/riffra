import type { CSSProperties, Dispatch, SetStateAction, RefObject } from 'react';
import type { MidiClip, MidiNote } from '@/model/generated';
import { idsInClientRect } from '@/shared/rect-selection';
import { isBlackKey, midiNoteName } from '@/shared/midi';
import type { usePointerGesture } from '@/shared/usePointerGesture';
import { pitchFromClientY, pitchRowTop, PITCH_HIGH, PITCH_LOW } from './model/pitch-grid';
import type { MidiGhostNote } from './model/ghost-notes';
import type { useNoteDrag } from './hooks/useNoteDrag';
import styles from './NoteLane.module.css';
import sharedStyles from './MidiEditorPanel.module.css';

interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}
interface NoteLaneProps {
  clip: MidiClip;
  laneHeight: number;
  laneWidth: number;
  pixelsPerTick: number;
  rowHeight: number;
  hoverPitch: number | null;
  setHoverPitch: Dispatch<SetStateAction<number | null>>;
  tool: 'pointer' | 'draw';
  setNoteContextMenu: Dispatch<SetStateAction<{ x: number; y: number; noteId: string } | null>>;
  editorRef: RefObject<HTMLDivElement | null>;
  setDrawPreview: Dispatch<SetStateAction<Rect | null>>;
  setMarquee: Dispatch<SetStateAction<Rect | null>>;
  setSelectedNoteIds: Dispatch<SetStateAction<string[]>>;
  selectedNoteIds: string[];
  placeTick: (tick: number, free: boolean) => number;
  newNoteLength: number;
  addNoteAt: (
    lane: HTMLElement,
    x: number,
    y: number,
    duration?: number,
    free?: boolean,
  ) => unknown;
  startGesture: ReturnType<typeof usePointerGesture>;
  marquee: Rect | null;
  drawPreview: Rect | null;
  lanePlayheadRef: RefObject<HTMLElement | null>;
  barLineTicks: number[];
  beatLineTicks: number[];
  subdivisionLineTicks: number[];
  ghostNotes: MidiGhostNote[];
  pitchRows: number[];
  notes: MidiNote[];
  dragging: ReturnType<typeof useNoteDrag>['dragging'];
  warnedNoteIds: Set<string>;
  noteBox: (note: MidiNote) => CSSProperties;
  handlePointerDown: ReturnType<typeof useNoteDrag>['handlePointerDown'];
  suppressNoteClickRef: ReturnType<typeof useNoteDrag>['suppressNoteClickRef'];
  dragReadout: { text: string; style: CSSProperties } | null;
}
const NOTE_LABEL_MIN_WIDTH = 30;
const NOTE_LABEL_MIN_HEIGHT = 11;
export function NoteLane({
  clip,
  laneHeight,
  laneWidth,
  pixelsPerTick,
  rowHeight,
  hoverPitch,
  setHoverPitch,
  tool,
  setNoteContextMenu,
  editorRef,
  setDrawPreview,
  setMarquee,
  setSelectedNoteIds,
  selectedNoteIds,
  placeTick,
  newNoteLength,
  addNoteAt,
  startGesture,
  marquee,
  drawPreview,
  lanePlayheadRef,
  barLineTicks,
  beatLineTicks,
  subdivisionLineTicks,
  ghostNotes,
  pitchRows,
  notes,
  dragging,
  warnedNoteIds,
  noteBox,
  handlePointerDown,
  suppressNoteClickRef,
  dragReadout,
}: NoteLaneProps) {
  return (
    <div
      className={styles.lane}
      data-midi-lane
      style={{ height: laneHeight, width: laneWidth }}
      onPointerMove={(event) => {
        const bounds = event.currentTarget.getBoundingClientRect();
        const pitch = pitchFromClientY(event.clientY, bounds.top, rowHeight);
        if (pitch !== hoverPitch) setHoverPitch(pitch);
      }}
      onPointerLeave={() => setHoverPitch(null)}
      onPointerDown={(event) => {
        setNoteContextMenu(null);
        editorRef.current?.focus();
        const target = event.target as HTMLElement;
        if (target.closest('[data-note-id]')) return;
        const lane = event.currentTarget;
        const bounds = lane.getBoundingClientRect();
        const originX = event.clientX;
        const originY = event.clientY;
        const append = event.ctrlKey || event.shiftKey;
        const draw = tool === 'draw';
        const move = (pointer: PointerEvent) =>
          draw
            ? setDrawPreview({
                left: Math.min(originX, pointer.clientX) - bounds.left,
                top: pitchRowTop(
                  pitchFromClientY(originY, bounds.top, rowHeight, true) ?? PITCH_LOW,
                  rowHeight,
                ),
                width: Math.abs(pointer.clientX - originX),
                height: rowHeight - 1,
              })
            : setMarquee({
                left: Math.min(originX, pointer.clientX) - bounds.left,
                top: Math.min(originY, pointer.clientY) - bounds.top,
                width: Math.abs(pointer.clientX - originX),
                height: Math.abs(pointer.clientY - originY),
              });
        const finish = (pointer: PointerEvent) => {
          const width = Math.abs(pointer.clientX - originX);
          const height = Math.abs(pointer.clientY - originY);
          if (draw) {
            const left = Math.min(originX, pointer.clientX);
            const right = Math.max(originX, pointer.clientX);
            const rawStart = (left - bounds.left) / pixelsPerTick;
            const rawEnd = (right - bounds.left) / pixelsPerTick;
            const startTick = placeTick(rawStart, pointer.altKey);
            const endTick = Math.max(startTick, placeTick(rawEnd, pointer.altKey));
            const durationTicks = endTick - startTick || newNoteLength;
            void Promise.resolve(addNoteAt(lane, left, originY, durationTicks, pointer.altKey));
          } else if (width < 4 && height < 4) {
            setSelectedNoteIds([]);
          } else {
            const ids = idsInClientRect(
              lane,
              '[data-note-id]',
              (element) => element.dataset.noteId,
              {
                left: Math.min(originX, pointer.clientX),
                right: Math.max(originX, pointer.clientX),
                top: Math.min(originY, pointer.clientY),
                bottom: Math.max(originY, pointer.clientY),
              },
            );
            setSelectedNoteIds(append ? [...new Set([...selectedNoteIds, ...ids])] : ids);
          }
          setMarquee(null);
          setDrawPreview(null);
        };
        const cancel = () => {
          setMarquee(null);
          setDrawPreview(null);
        };
        startGesture(event, { onMove: move, onEnd: finish, onCancel: cancel });
      }}
      onDoubleClick={(event) => {
        if ((event.target as HTMLElement).closest('[data-note-id]')) return;
        event.preventDefault();
        void Promise.resolve(
          addNoteAt(event.currentTarget, event.clientX, event.clientY, newNoteLength, event.altKey),
        );
      }}
    >
      <div
        className={styles.clipEnd}
        style={{ left: clip.durationTicks * pixelsPerTick }}
        aria-hidden="true"
      />
      {marquee && <div className={styles.marquee} style={marquee} />}
      {drawPreview && <div className={styles.drawPreview} style={drawPreview} />}
      <i
        ref={lanePlayheadRef}
        className={sharedStyles.editorPlayhead}
        style={{ display: 'none' }}
      />
      {barLineTicks.map((tick) => (
        <i key={tick} className={styles.barLine} style={{ left: tick * pixelsPerTick }} />
      ))}
      {beatLineTicks.map((tick) => (
        <i key={tick} className={styles.beatLine} style={{ left: tick * pixelsPerTick }} />
      ))}
      {subdivisionLineTicks.map((tick) => (
        <i
          key={`subdivision-${tick}`}
          className={styles.subdivisionLine}
          data-grid-subdivision
          style={{ left: tick * pixelsPerTick }}
        />
      ))}
      {ghostNotes
        .filter((ghost) => ghost.pitch >= PITCH_LOW && ghost.pitch < PITCH_HIGH)
        .map((ghost) => (
          <div
            key={ghost.id}
            data-ghost-note-id={ghost.id}
            aria-hidden="true"
            className={styles.ghostNote}
            style={
              {
                left: ghost.startTick * pixelsPerTick,
                top: pitchRowTop(ghost.pitch, rowHeight),
                width: Math.max(4, ghost.durationTicks * pixelsPerTick),
                height: rowHeight - 1,
                '--ghost-color': ghost.trackColor,
              } as CSSProperties
            }
          />
        ))}
      {pitchRows.map((pitch) => (
        <div
          key={pitch}
          className={`${styles.pitchRow} ${isBlackKey(pitch % 12) ? styles.pitchBlack : ''} ${pitch % 12 === 0 ? styles.pitchOctave : ''}`}
          data-hover={pitch === hoverPitch || undefined}
          style={{ top: pitchRowTop(pitch, rowHeight), height: rowHeight }}
        />
      ))}
      {notes
        .filter((note) => note.note >= PITCH_LOW && note.note < PITCH_HIGH)
        .map((note) => {
          const preview = dragging?.copying ? undefined : dragging?.previewNotes[note.id];
          const visibleNote = preview ?? note;
          return (
            <span
              key={note.id}
              data-note-id={note.id}
              data-midi-warning={warnedNoteIds.has(note.id) || undefined}
              className={`${styles.note} ${preview ? styles.dragging : ''} ${selectedNoteIds.includes(note.id) ? styles.selected : ''}`}
              style={noteBox(visibleNote)}
              onPointerDown={(event) => handlePointerDown(event, note, 'move')}
              onClick={(event) => {
                event.stopPropagation();
                editorRef.current?.focus();
                if (suppressNoteClickRef.current) {
                  suppressNoteClickRef.current = false;
                  return;
                }
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
                event.stopPropagation();
                editorRef.current?.focus();
                if (!selectedNoteIds.includes(note.id)) setSelectedNoteIds([note.id]);
                setNoteContextMenu({ x: event.clientX, y: event.clientY, noteId: note.id });
              }}
            >
              {visibleNote.durationTicks * pixelsPerTick >= NOTE_LABEL_MIN_WIDTH &&
                rowHeight >= NOTE_LABEL_MIN_HEIGHT && (
                  <span className={styles.noteLabel}>{midiNoteName(visibleNote.note)}</span>
                )}
              <i
                className={styles.resizeStart}
                onPointerDown={(event) => handlePointerDown(event, note, 'resizeStart')}
              />
              <i
                className={styles.resizeEnd}
                onPointerDown={(event) => handlePointerDown(event, note, 'resizeEnd')}
              />
            </span>
          );
        })}
      {dragging?.copying &&
        Object.values(dragging.previewNotes).map((copy) => (
          <span
            key={`copy-${copy.id}`}
            aria-hidden="true"
            className={`${styles.note} ${styles.dragging}`}
            style={noteBox(copy)}
          />
        ))}
      {dragReadout && (
        <span className={styles.dragReadout} style={dragReadout.style}>
          {dragReadout.text}
        </span>
      )}
    </div>
  );
}
