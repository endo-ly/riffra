import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
  type RefObject,
} from 'react';
import type { MidiClip, MidiNote, MidiNoteInput } from '@/model/generated';
import type { usePointerGesture } from '@/shared/usePointerGesture';
import { buildNoteDragPreview, calculateNoteDrag, type NoteDragMode } from '../model/note-drag';
import type { NoteEdit } from './useNoteEditing';

interface Options {
  clip: MidiClip | null;
  selectedNoteIds: string[];
  pixelsPerTick: number;
  rowHeight: number;
  placeTick: (tick: number, free: boolean) => number;
  editorRef: RefObject<HTMLDivElement | null>;
  startGesture: ReturnType<typeof usePointerGesture>;
  setLastNoteLength: Dispatch<SetStateAction<number | null>>;
  auditionNote: (pitch: number, velocity: number, duration?: number) => void;
  selectAddedNotes: (operation: NoteEdit) => NoteEdit;
  onInsertNotes: (clipId: string, notes: MidiNoteInput[]) => NoteEdit;
  onUpdateNote: (clipId: string, note: MidiNote) => NoteEdit;
  onUpdateNotes: (
    clipId: string,
    updates: { noteId: string; patch: Partial<MidiNote> }[],
  ) => NoteEdit;
}
const NOTE_DRAG_THRESHOLD = 3;
export function useNoteDrag({
  clip,
  selectedNoteIds,
  pixelsPerTick,
  rowHeight,
  placeTick,
  editorRef,
  startGesture,
  setLastNoteLength,
  auditionNote,
  selectAddedNotes,
  onInsertNotes,
  onUpdateNotes,
  onUpdateNote,
}: Options) {
  const dragGestureRef = useRef(0);
  const suppressNoteClickRef = useRef(false);
  const [dragging, setDragging] = useState<{
    gestureId: number;
    noteId: string;
    mode: NoteDragMode;
    /** Ctrl-drag places moved copies and leaves the originals in place. */
    copying: boolean;
    previewNotes: Record<string, MidiNote>;
    awaitingCanonical: boolean;
  } | null>(null);

  const resetDrag = useCallback(() => {
    setDragging(null);
    dragGestureRef.current += 1;
  }, []);
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

  const handlePointerDown = (
    event: React.PointerEvent<HTMLElement>,
    note: MidiNote,
    mode: NoteDragMode,
  ) => {
    editorRef.current?.focus();
    event.stopPropagation();
    setLastNoteLength(note.durationTicks);
    const originX = event.clientX;
    const originY = event.clientY;
    const copying = mode === 'move' && (event.ctrlKey || event.metaKey);
    const movingNoteIds =
      selectedNoteIds.includes(note.id) && selectedNoteIds.length > 1 ? selectedNoteIds : [note.id];
    const originNotes = clip!.notes.filter((candidate) => movingNoteIds.includes(candidate.id));
    let preview = note;
    let previewNotes = Object.fromEntries(
      originNotes.map((candidate) => [candidate.id, candidate]),
    ) as Record<string, MidiNote>;
    const gestureId = dragGestureRef.current + 1;
    dragGestureRef.current = gestureId;
    setDragging({
      gestureId,
      noteId: note.id,
      mode,
      copying,
      previewNotes,
      awaitingCanonical: false,
    });
    const updatePreview = (pointer: PointerEvent) =>
      calculateNoteDrag(
        (pointer.clientX - originX) / pixelsPerTick,
        Math.round((originY - pointer.clientY) / rowHeight),
        pointer.altKey,
        note,
        originNotes,
        mode,
        placeTick,
      );
    const buildPreviewNotes = (nextNote: MidiNote) =>
      buildNoteDragPreview(note, originNotes, nextNote);
    // A press that stays put is a click: it selects without snapping the note anywhere.
    const travelled = (pointer: PointerEvent) =>
      Math.abs(pointer.clientX - originX) >= NOTE_DRAG_THRESHOLD ||
      Math.abs(pointer.clientY - originY) >= NOTE_DRAG_THRESHOLD;
    let moved = false;
    const move = (pointer: PointerEvent) => {
      moved ||= travelled(pointer);
      if (!moved) return;
      preview = updatePreview(pointer);
      previewNotes = buildPreviewNotes(preview);
      setDragging((current) =>
        current?.gestureId === gestureId
          ? { ...current, previewNotes, awaitingCanonical: false }
          : current,
      );
    };
    const clearDragging = () => {
      setDragging((current) => (current?.gestureId === gestureId ? null : current));
    };
    const finish = (pointer: PointerEvent) => {
      moved ||= travelled(pointer);
      preview = moved ? updatePreview(pointer) : note;
      previewNotes = buildPreviewNotes(preview);
      const changed = originNotes.some((candidate) => {
        const next = previewNotes[candidate.id];
        return (
          next.startTick !== candidate.startTick ||
          next.note !== candidate.note ||
          next.durationTicks !== candidate.durationTicks
        );
      });
      // A drag that edited notes ends without the click that would reselect them.
      suppressNoteClickRef.current = changed;
      if (!changed) {
        clearDragging();
        return;
      }
      auditionNote(preview.note, preview.velocity, preview.durationTicks);
      if (mode !== 'move') setLastNoteLength(preview.durationTicks);
      let operation: NoteEdit;
      if (copying) {
        operation = selectAddedNotes(
          onInsertNotes(
            clip!.id,
            originNotes.map((candidate) => {
              const next = previewNotes[candidate.id];
              return {
                pitch: next.note,
                startTick: next.startTick,
                durationTicks: next.durationTicks,
                velocity: next.velocity,
                channel: next.channel,
              };
            }),
          ),
        );
      } else if (originNotes.length > 1) {
        operation = onUpdateNotes(
          clip!.id,
          originNotes.map((candidate) => {
            const next = previewNotes[candidate.id];
            return {
              noteId: candidate.id,
              patch:
                mode === 'move'
                  ? { startTick: next.startTick, note: next.note }
                  : mode === 'resizeStart'
                    ? { startTick: next.startTick, durationTicks: next.durationTicks }
                    : { durationTicks: next.durationTicks },
            };
          }),
        );
      } else {
        operation = onUpdateNote(clip!.id, preview);
      }
      setDragging((current) =>
        current?.gestureId === gestureId
          ? { ...current, previewNotes, awaitingCanonical: true }
          : current,
      );
      void operation.then(clearDragging, clearDragging);
    };
    startGesture(event, { onMove: move, onEnd: finish, onCancel: clearDragging });
  };

  return { dragging, handlePointerDown, suppressNoteClickRef, resetDrag };
}
