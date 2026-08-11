import { useEffect, useMemo, useRef, useState } from 'react';
import type { CreativeSession, MidiClip, MidiNote, ProjectTimebase } from '@/lib/domain';
import {
  SNAP_GRID_OPTIONS,
  snapGridLabel,
  snapGridTicks,
  ticksPerBar,
  ticksPerBeat,
  countOffGridNotes,
} from '@/lib/arrange-timeline';
import type { SnapGrid } from '@/lib/arrange-timeline';
import { isBlackKey, midiNoteName } from '@/lib/musical-typing';
import { isEditableTarget } from '@/lib/interaction';
import { toast } from '@/lib/toasts';
import { ContextMenu, type ContextMenuItem } from '../shared/ContextMenu';
import styles from './MidiEditorPanel.module.css';

type MidiEditResult = void | PromiseLike<CreativeSession | null>;

interface MidiEditorPanelProps {
  clip: MidiClip | null;
  timebase: ProjectTimebase;
  onUpdateNote?: (clipId: string, note: MidiNote) => void | PromiseLike<CreativeSession | null>;
  onUpdateNotes?: (
    clipId: string,
    updates: { noteId: string; patch: Partial<MidiNote> }[],
  ) => void | PromiseLike<CreativeSession | null>;
  onRemoveNote?: (clipId: string, noteId: string) => void;
  onAddNote?: (clipId: string, startTick: number, pitch: number) => void;
  onQuantize?: (clipId: string, noteIds: string[], gridTicks: number) => MidiEditResult;
  onDuplicateNotes?: (clipId: string, noteIds: string[], offsetTicks: number) => void;
}

const PITCH_HIGH = 128;
const PITCH_LOW = 0;
const EMPTY_NOTES: MidiNote[] = [];

export function MidiEditorPanel(props: MidiEditorPanelProps) {
  const { clip, onRemoveNote } = props;
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [dragging, setDragging] = useState<{
    gestureId: number;
    noteId: string;
    previewNotes: Record<string, MidiNote>;
    awaitingCanonical: boolean;
  } | null>(null);
  const [selectedNoteIds, setSelectedNoteIds] = useState<string[]>([]);
  const [velocityDraft, setVelocityDraft] = useState(96);
  const [pixelsPerTick, setPixelsPerTick] = useState(0.18);
  const [marquee, setMarquee] = useState<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);
  const [noteContextMenu, setNoteContextMenu] = useState<{
    x: number;
    y: number;
    noteId: string;
  } | null>(null);
  const dragGestureRef = useRef(0);
  const laneViewportRef = useRef<HTMLDivElement>(null);
  const centeredClipIdRef = useRef<string | undefined>(undefined);
  const rowHeight = 12;
  const clipId = clip?.id;
  const hasClip = clip !== null;
  const clipNoteCenter = clip?.notes.length
    ? clip.notes.reduce((total, note) => total + note.note, 0) / clip.notes.length
    : 60;
  const visibleTicks = Math.max(props.clip?.durationTicks ?? 1920, 1920);
  const laneHeight = (PITCH_HIGH - PITCH_LOW) * rowHeight;
  const beatTicks = ticksPerBeat(props.timebase);
  const barTicks = ticksPerBar(props.timebase);
  const snapTicks = snapGridTicks(snap, props.timebase);

  const notes = props.clip?.notes ?? EMPTY_NOTES;
  const selectedNotes = useMemo(
    () => notes.filter((note) => selectedNoteIds.includes(note.id)),
    [notes, selectedNoteIds],
  );
  const selectedVelocity = selectedNotes.length
    ? Math.round(
        selectedNotes.reduce((total, note) => total + note.velocity, 0) / selectedNotes.length,
      )
    : 96;
  const pitchRows = useMemo(
    () => Array.from({ length: PITCH_HIGH - PITCH_LOW }, (_, index) => PITCH_LOW + index).reverse(),
    [],
  );

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key !== 'Delete' || !clip || selectedNoteIds.length === 0) return;
      if (isEditableTarget(event.target)) return;
      event.preventDefault();
      for (const noteId of selectedNoteIds) onRemoveNote?.(clip.id, noteId);
      setSelectedNoteIds([]);
    };
    window.addEventListener('keydown', keydown);
    return () => window.removeEventListener('keydown', keydown);
  }, [clip, onRemoveNote, selectedNoteIds]);

  useEffect(() => {
    setVelocityDraft(selectedVelocity);
  }, [selectedVelocity]);

  useEffect(() => {
    setSelectedNoteIds([]);
    setDragging(null);
    dragGestureRef.current += 1;
    setMarquee(null);
    setNoteContextMenu(null);
  }, [clipId, hasClip]);

  useEffect(() => {
    if (!hasClip || clipId === undefined) {
      centeredClipIdRef.current = undefined;
      return;
    }
    if (centeredClipIdRef.current === clipId) return;
    centeredClipIdRef.current = clipId;
    const viewport = laneViewportRef.current;
    if (!viewport) return;
    viewport.scrollTop = Math.max(
      0,
      (PITCH_HIGH - 1 - clipNoteCenter) * rowHeight - viewport.clientHeight / 2,
    );
  }, [clipId, clipNoteCenter, hasClip]);

  useEffect(() => {
    if (!dragging?.awaitingCanonical || !clip) return;
    const previews = Object.values(dragging.previewNotes);
    const canonicalNotes = new Map(clip.notes.map((note) => [note.id, note]));
    if (previews.some((preview) => !canonicalNotes.has(preview.id))) {
      setDragging((current) => (current?.gestureId === dragging.gestureId ? null : current));
      return;
    }
    const canonicalMatches = previews.every((preview) => {
      const canonical = canonicalNotes.get(preview.id)!;
      return (
        canonical.note === preview.note &&
        canonical.startTick === preview.startTick &&
        canonical.durationTicks === preview.durationTicks &&
        canonical.velocity === preview.velocity
      );
    });
    if (canonicalMatches) {
      setDragging((current) => (current?.gestureId === dragging.gestureId ? null : current));
    }
  }, [clip, dragging]);

  if (!props.clip) {
    return <div className={styles.empty} />;
  }

  const contextNote = noteContextMenu
    ? props.clip.notes.find((note) => note.id === noteContextMenu.noteId)
    : null;
  const noteContextItems: ContextMenuItem[] = contextNote
    ? [
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            props.onRemoveNote?.(props.clip!.id, contextNote.id);
            setSelectedNoteIds((current) => current.filter((id) => id !== contextNote.id));
          },
        },
        {
          label: 'Duplicate',
          onClick: () =>
            props.onDuplicateNotes?.(props.clip!.id, [contextNote.id], snapTicks || 240),
        },
      ]
    : [];

  const handlePointerDown = (
    event: React.PointerEvent<HTMLSpanElement>,
    note: MidiNote,
    mode: 'move' | 'resize',
  ) => {
    event.stopPropagation();
    const originX = event.clientX;
    const originY = event.clientY;
    const movingNoteIds =
      mode === 'move' && selectedNoteIds.includes(note.id) && selectedNoteIds.length > 1
        ? selectedNoteIds
        : [note.id];
    const originNotes = props.clip!.notes.filter((candidate) =>
      movingNoteIds.includes(candidate.id),
    );
    let preview = note;
    let previewNotes = Object.fromEntries(
      originNotes.map((candidate) => [candidate.id, candidate]),
    ) as Record<string, MidiNote>;
    const gestureId = dragGestureRef.current + 1;
    dragGestureRef.current = gestureId;
    setDragging({
      gestureId,
      noteId: note.id,
      previewNotes,
      awaitingCanonical: false,
    });
    const handle = event.currentTarget;
    handle.setPointerCapture?.(event.pointerId);
    const updatePreview = (clientX: number, clientY: number) => {
      const deltaTicks = (clientX - originX) / pixelsPerTick;
      if (mode === 'move') {
        const nextTick = Math.max(
          0,
          snapTicks
            ? Math.round((note.startTick + deltaTicks) / snapTicks) * snapTicks
            : Math.round(note.startTick + deltaTicks),
        );
        const pitchDelta = Math.round((originY - clientY) / rowHeight);
        const nextPitch = Math.max(PITCH_LOW, Math.min(PITCH_HIGH - 1, note.note + pitchDelta));
        return {
          ...note,
          startTick: nextTick,
          note: nextPitch,
        };
      }
      const nextDur = Math.max(
        1,
        snapTicks
          ? Math.round((note.durationTicks + deltaTicks) / snapTicks) * snapTicks
          : Math.round(note.durationTicks + deltaTicks),
      );
      return { ...note, durationTicks: nextDur };
    };
    const buildPreviewNotes = (nextNote: MidiNote) => {
      if (mode !== 'move' || originNotes.length < 2) return { [nextNote.id]: nextNote };
      const tickDelta = nextNote.startTick - note.startTick;
      const pitchDelta = nextNote.note - note.note;
      return Object.fromEntries(
        originNotes.map((candidate) => [
          candidate.id,
          {
            ...candidate,
            startTick: Math.max(0, candidate.startTick + tickDelta),
            note: Math.max(PITCH_LOW, Math.min(PITCH_HIGH - 1, candidate.note + pitchDelta)),
          },
        ]),
      ) as Record<string, MidiNote>;
    };
    const move = (pointer: PointerEvent) => {
      preview = updatePreview(pointer.clientX, pointer.clientY);
      previewNotes = buildPreviewNotes(preview);
      setDragging((current) =>
        current?.gestureId === gestureId
          ? { ...current, previewNotes, awaitingCanonical: false }
          : current,
      );
    };
    let finish: (pointer: PointerEvent) => void = () => undefined;
    let cancel: () => void = () => undefined;
    const cleanup = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
    };
    const clearDragging = () => {
      setDragging((current) => (current?.gestureId === gestureId ? null : current));
    };
    finish = (pointer) => {
      cleanup();
      preview = updatePreview(pointer.clientX, pointer.clientY);
      previewNotes = buildPreviewNotes(preview);
      let operation: MidiEditResult | undefined;
      const changed = originNotes.some((candidate) => {
        const next = previewNotes[candidate.id];
        return (
          next &&
          (next.startTick !== candidate.startTick ||
            next.note !== candidate.note ||
            next.durationTicks !== candidate.durationTicks)
        );
      });
      if (changed) {
        if (mode === 'move' && originNotes.length > 1) {
          operation = props.onUpdateNotes?.(
            props.clip!.id,
            originNotes.map((candidate) => ({
              noteId: candidate.id,
              patch: {
                startTick: previewNotes[candidate.id].startTick,
                note: previewNotes[candidate.id].note,
              },
            })),
          );
        } else {
          operation = props.onUpdateNote?.(props.clip!.id, preview);
        }
      }
      if (operation === undefined) {
        clearDragging();
        return;
      }
      setDragging((current) =>
        current?.gestureId === gestureId
          ? { ...current, previewNotes, awaitingCanonical: true }
          : current,
      );
      void Promise.resolve(operation).then(clearDragging, clearDragging);
    };
    cancel = () => {
      cleanup();
      clearDragging();
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
          <small>{Math.ceil(props.clip.durationTicks / barTicks)} bars</small>
        </div>
        <div className={styles.editorTools}>
          <label className={styles.control}>
            <span>Snap</span>
            <select value={snap} onChange={(event) => setSnap(event.target.value as SnapGrid)}>
              {SNAP_GRID_OPTIONS.map((value) => (
                <option key={value} value={value}>
                  {snapGridLabel(value)}
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
            disabled={!selectedNoteIds.length || snapTicks === 0 || !props.onQuantize}
            title={snapTicks === 0 ? 'Select a snap grid before quantizing' : undefined}
            onClick={() => {
              if (!props.clip) return;
              const selected = props.clip.notes.filter((note) => selectedNoteIds.includes(note.id));
              const offGrid = countOffGridNotes(selected, snapTicks);
              if (offGrid === 0) {
                toast('Selected notes are already on the grid.');
                return;
              }
              const operation = props.onQuantize?.(props.clip.id, selectedNoteIds, snapTicks);
              if (!operation) return;
              void Promise.resolve(operation).then(
                (next) => {
                  if (next) {
                    toast(`Quantized ${offGrid} note${offGrid === 1 ? '' : 's'} to ${snap}.`);
                  }
                },
                (error) => {
                  const detail = error instanceof Error ? error.message : String(error);
                  toast(`Quantize failed: ${detail}`, { kind: 'error' });
                },
              );
            }}
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
              value={velocityDraft}
              disabled={!selectedNoteIds.length}
              onChange={(event) => setVelocityDraft(Number(event.currentTarget.value))}
              onKeyUp={(event) => {
                if (event.key.startsWith('Arrow') || event.key === 'Home' || event.key === 'End') {
                  void props.onUpdateNotes?.(
                    props.clip!.id,
                    selectedNoteIds.map((noteId) => ({
                      noteId,
                      patch: { velocity: Number(event.currentTarget.value) },
                    })),
                  );
                }
              }}
              onPointerUp={(event) => {
                void props.onUpdateNotes?.(
                  props.clip!.id,
                  selectedNoteIds.map((noteId) => ({
                    noteId,
                    patch: { velocity: Number(event.currentTarget.value) },
                  })),
                );
              }}
            />
          </label>
        </div>
      </header>
      <div ref={laneViewportRef} className={styles.laneViewport}>
        <div className={styles.roll}>
          <div
            className={styles.pitchKeyboard}
            style={{ height: laneHeight }}
            aria-label="MIDI piano keyboard"
          >
            {pitchRows.map((pitch) => (
              <div
                key={pitch}
                className={styles.pianoKey}
                style={{ top: (PITCH_HIGH - pitch - 1) * rowHeight }}
              >
                <span className={styles.pianoWhiteKey}>
                  {pitch % 12 === 0 ? midiNoteName(pitch) : null}
                </span>
                {isBlackKey(pitch % 12) && (
                  <span className={styles.pianoBlackKey} aria-hidden="true" />
                )}
              </div>
            ))}
          </div>
          <div
            className={styles.lane}
            style={{ height: laneHeight, width: visibleTicks * pixelsPerTick }}
            onPointerDown={(event) => {
              setNoteContextMenu(null);
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
                  const pitch = PITCH_HIGH - 1 - Math.floor((originY - bounds.top) / rowHeight);
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
                {pitch % 12 === 0 ? midiNoteName(pitch) : null}
              </div>
            ))}
            {notes
              .filter((note) => note.note >= PITCH_LOW && note.note < PITCH_HIGH)
              .map((note) => {
                const visibleNote = dragging?.previewNotes[note.id] ?? note;
                const isPreviewed = Boolean(dragging?.previewNotes[note.id]);
                const noteWidth = visibleNote.durationTicks * pixelsPerTick;
                return (
                  <span
                    key={note.id}
                    data-note-id={note.id}
                    className={`${styles.note} ${isPreviewed ? styles.dragging : ''} ${noteWidth < 12 ? styles.narrow : ''} ${selectedNoteIds.includes(note.id) ? styles.selected : ''}`}
                    style={{
                      left: visibleNote.startTick * pixelsPerTick,
                      top: (PITCH_HIGH - visibleNote.note - 1) * rowHeight,
                      width: Math.max(4, noteWidth),
                      height: rowHeight - 1,
                      opacity: Math.max(0.45, 0.4 + (visibleNote.velocity / 127) * 0.6),
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
                      event.stopPropagation();
                      setNoteContextMenu({ x: event.clientX, y: event.clientY, noteId: note.id });
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
      {noteContextMenu && contextNote && (
        <ContextMenu
          x={noteContextMenu.x}
          y={noteContextMenu.y}
          items={noteContextItems}
          onClose={() => setNoteContextMenu(null)}
        />
      )}
    </div>
  );
}
