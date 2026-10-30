import { useEffect, useMemo, useRef, useState, type MutableRefObject } from 'react';
import type {
  ArrangementMutationResult,
  MidiClip,
  MidiNote,
  MidiNoteInput,
  ProjectTimebase,
} from '@/model/generated';
import { countOffGridNotes, snapClipTick, type SnapGrid } from '@/model/clip-grid';
import { snapToBar } from '@/model/timebase';
import { errorText } from '@/shared/errors';
import { isEditableTypingTarget } from '@/shared/input';
import { useSessionContext } from '@/shared/session/session-context';
import { toast } from '@/shared/toasts';
import type { ContextMenuItem } from '@/shared/ui/ContextMenu';
import { PITCH_HIGH, PITCH_LOW, pitchFromClientY } from '../model/pitch-grid';

/** A note edit: the canonical the Host accepted, or null when it did not run. */
export type NoteEdit = Promise<ArrangementMutationResult | null>;

interface NoteEditingOptions {
  clip: MidiClip | null;
  snap: SnapGrid;
  snapTicks: number;
  beatTicks: number;
  timebase: ProjectTimebase;
  pixelsPerTick: number;
  rowHeight: number;
  /** Velocity the next placed or nudged note takes. */
  velocity: number;
  onVelocityChange(value: number): void;
  /** Snaps a clip-local tick to the grid, or keeps it free while Alt is held. */
  placeTick(tick: number, free: boolean): number;
  playheadTickRef: MutableRefObject<number>;
  auditionNote(pitch: number, velocity: number, durationTicks?: number): void;
  /** Clears the transient view state owned by the panel. */
  onClearTransient(): void;
}

const EMPTY_NOTES: MidiNote[] = [];

/** Note selection, editing commands and the note context menu of the MIDI editor. */
export function useNoteEditing(options: NoteEditingOptions) {
  const { clip, snap, snapTicks, beatTicks, timebase, pixelsPerTick, rowHeight } = options;
  const { api, commit } = useSessionContext();
  const [selectedNoteIds, setSelectedNoteIds] = useState<string[]>([]);
  const [lastNoteLength, setLastNoteLength] = useState<number | null>(null);
  const [velocityDraft, setVelocityDraft] = useState(96);
  const [noteContextMenu, setNoteContextMenu] = useState<{
    x: number;
    y: number;
    noteId: string;
  } | null>(null);
  const clipboardRef = useRef<{
    notes: (Omit<MidiNoteInput, 'startTick'> & { relativeStartTick: number })[];
  } | null>(null);

  const notes = clip?.notes ?? EMPTY_NOTES;
  const selectedNotes = useMemo(
    () => notes.filter((note) => selectedNoteIds.includes(note.id)),
    [notes, selectedNoteIds],
  );
  // New notes take the length of the note touched last, so a phrase keeps one rhythm.
  const newNoteLength = lastNoteLength ?? (snapTicks || beatTicks);
  // Without a selection the velocity control sets the velocity of the next placed note.
  const selectedVelocity = selectedNotes.length
    ? Math.round(
        selectedNotes.reduce((total, note) => total + note.velocity, 0) / selectedNotes.length,
      )
    : options.velocity;

  // The velocity control follows the notes the selection points at.
  useEffect(() => {
    setVelocityDraft(selectedVelocity);
  }, [selectedVelocity]);

  const mutations = {
    insertNotes: (clipId: string, inputs: MidiNoteInput[]) =>
      commit(api.insertMidiNotes(clipId, inputs)),
    updateNote: (clipId: string, note: MidiNote) =>
      commit(
        api.updateMidiNote(clipId, note.id, {
          note: note.note,
          startTick: note.startTick,
          durationTicks: note.durationTicks,
          velocity: note.velocity,
        }),
      ),
    updateNotes: (clipId: string, updates: { noteId: string; patch: Partial<MidiNote> }[]) =>
      commit(
        api.updateMidiNotes(
          clipId,
          updates.map((update) => ({
            noteId: update.noteId,
            patch: {
              note: update.patch.note,
              startTick: update.patch.startTick,
              durationTicks: update.patch.durationTicks,
              velocity: update.patch.velocity,
            },
          })),
        ),
      ),
  };

  const snapLocalTick = (tick: number, start: number, gridTicks: number) =>
    snap === 'bar'
      ? Math.max(0, snapToBar(start + tick, timebase) - start)
      : snapClipTick(tick, start, gridTicks);

  const removeSelectedNotes = (noteIds = selectedNoteIds): NoteEdit | null => {
    if (!clip || noteIds.length === 0) return null;
    const uniqueIds = [...new Set(noteIds)];
    setSelectedNoteIds([]);
    return commit(api.removeMidiNotes(clip.id, uniqueIds));
  };

  const copySelectedNotes = (): boolean => {
    if (!selectedNotes.length) return false;
    const anchor = Math.min(...selectedNotes.map((note) => note.startTick));
    clipboardRef.current = {
      notes: selectedNotes.map((note) => ({
        pitch: note.note,
        relativeStartTick: note.startTick - anchor,
        durationTicks: note.durationTicks,
        velocity: note.velocity,
        channel: note.channel,
      })),
    };
    return true;
  };

  /** Selects the notes an edit adds to the clip once the edit lands. */
  const selectAddedNotes = (operation: NoteEdit): NoteEdit => {
    if (clip)
      void operation.then((next) => {
        if (next) setSelectedNoteIds(next.createdEntityIds.midiNotes ?? []);
      });
    return operation;
  };

  const pasteNotes = (): NoteEdit | null => {
    if (!clip || !clipboardRef.current) return null;
    const anchor = snapLocalTick(
      options.playheadTickRef.current - clip.startTick,
      clip.startTick,
      snapTicks,
    );
    const inputs = clipboardRef.current.notes.map(({ relativeStartTick, ...note }) => ({
      ...note,
      startTick: anchor + relativeStartTick,
    }));
    return selectAddedNotes(mutations.insertNotes(clip.id, inputs));
  };

  const duplicateSelectedNotes = (): NoteEdit | null => {
    if (!clip || selectedNotes.length === 0) return null;
    const start = Math.min(...selectedNotes.map((note) => note.startTick));
    const end = Math.max(
      ...selectedNotes.map((note) => note.startTick + Math.max(1, note.durationTicks)),
    );
    return commit(api.duplicateMidiNotes(clip.id, selectedNoteIds, Math.max(1, end - start)));
  };

  const nudgeSelectedNotes = (
    direction: 'left' | 'right' | 'up' | 'down',
    pitchStep = 1,
  ): NoteEdit | null => {
    if (!clip || selectedNotes.length === 0) return null;
    const timeStep = snapTicks || beatTicks;
    const requestedPitchDelta =
      direction === 'up' ? pitchStep : direction === 'down' ? -pitchStep : 0;
    const lowestSelectedPitch = Math.min(...selectedNotes.map((note) => note.note));
    const highestSelectedPitch = Math.max(...selectedNotes.map((note) => note.note));
    const pitchDelta =
      requestedPitchDelta > 0
        ? Math.min(requestedPitchDelta, PITCH_HIGH - 1 - highestSelectedPitch)
        : Math.max(requestedPitchDelta, PITCH_LOW - lowestSelectedPitch);
    let timeDelta = direction === 'left' ? -timeStep : direction === 'right' ? timeStep : 0;
    if (timeDelta !== 0 && snapTicks > 0) {
      const anchor = selectedNotes[0].startTick;
      const units = (clip.startTick + anchor) / snapTicks;
      const next = direction === 'left' ? Math.ceil(units) - 1 : Math.floor(units) + 1;
      timeDelta = next * snapTicks - clip.startTick - anchor;
    }
    timeDelta = Math.max(-Math.min(...selectedNotes.map((note) => note.startTick)), timeDelta);
    if (pitchDelta === 0 && timeDelta === 0) return null;
    if (pitchDelta !== 0) {
      const first = selectedNotes[0];
      options.auditionNote(first.note + pitchDelta, first.velocity, first.durationTicks);
    }
    return mutations.updateNotes(
      clip.id,
      selectedNotes.map((note) => ({
        noteId: note.id,
        patch: {
          startTick: Math.max(0, note.startTick + timeDelta),
          note: Math.max(PITCH_LOW, Math.min(PITCH_HIGH - 1, note.note + pitchDelta)),
        },
      })),
    );
  };

  const addNoteAt = (
    lane: HTMLElement,
    clientX: number,
    clientY: number,
    durationTicks = newNoteLength,
    free = false,
  ): NoteEdit | null => {
    if (!clip) return null;
    const bounds = lane.getBoundingClientRect();
    const rawTick = (clientX - bounds.left) / pixelsPerTick;
    const rawPitch = pitchFromClientY(clientY, bounds.top, rowHeight);
    if (rawPitch === null) return null;
    const requestedStartTick = options.placeTick(rawTick, free);
    const startTick = Math.min(Math.max(0, clip.durationTicks - 1), requestedStartTick);
    const duration = Math.max(1, Math.round(durationTicks));
    setLastNoteLength(duration);
    options.auditionNote(rawPitch, options.velocity, duration);
    return selectAddedNotes(
      commit(api.addMidiNote(clip.id, startTick, rawPitch, duration, options.velocity, 1)),
    );
  };

  const quantize = (targets: MidiNote[]) => {
    if (!clip) return;
    const offGrid = countOffGridNotes(targets, snapTicks, clip.startTick);
    if (offGrid === 0) {
      toast(
        selectedNotes.length
          ? 'Selected notes are already on the grid.'
          : 'Notes are already on the grid.',
      );
      return;
    }
    void Promise.resolve(
      commit(
        api.quantizeMidiNotes(
          clip.id,
          targets.map((note) => note.id),
          snapTicks,
        ),
      ),
    ).then(
      (next) => {
        if (next) {
          toast(`Quantized ${offGrid} note${offGrid === 1 ? '' : 's'} to ${snap}.`);
        }
      },
      (error: unknown) => {
        const detail = errorText(error);
        toast(`Quantize failed: ${detail}`, { kind: 'error' });
      },
    );
  };

  // Quantize works on the selection, or on the whole clip when nothing is selected.
  const quantizeTargets = selectedNotes.length ? selectedNotes : notes;
  const runQuantize = () => quantize(quantizeTargets);

  const handleVelocityChange = (value: number) => {
    setVelocityDraft(value);
    options.onVelocityChange(value);
  };

  const commitVelocity = (value: number) => {
    options.onVelocityChange(value);
    if (!clip || !selectedNoteIds.length) return;
    void mutations.updateNotes(
      clip.id,
      selectedNoteIds.map((noteId) => ({ noteId, patch: { velocity: value } })),
    );
  };

  const handleEditorKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (isEditableTypingTarget(event.target)) return;
    const key = event.key.toLowerCase();
    const modifier = event.ctrlKey || event.metaKey;
    let operation: NoteEdit | null = null;

    if (modifier && key === 'a') {
      setSelectedNoteIds(notes.map((note) => note.id));
    } else if (modifier && key === 'c') {
      copySelectedNotes();
    } else if (modifier && key === 'x') {
      if (copySelectedNotes()) operation = removeSelectedNotes();
    } else if (modifier && key === 'v') {
      operation = pasteNotes();
    } else if (modifier && key === 'd') {
      operation = duplicateSelectedNotes();
    } else if (event.key === 'Delete' && selectedNoteIds.length > 0) {
      operation = removeSelectedNotes();
    } else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      operation = nudgeSelectedNotes(event.key === 'ArrowLeft' ? 'left' : 'right');
    } else if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
      operation = nudgeSelectedNotes(
        event.key === 'ArrowUp' ? 'up' : 'down',
        event.shiftKey ? 12 : 1,
      );
    } else if (event.key === 'Escape') {
      setSelectedNoteIds([]);
      setNoteContextMenu(null);
      options.onClearTransient();
    } else {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    if (operation) void operation;
  };

  const contextNote = noteContextMenu
    ? notes.find((note) => note.id === noteContextMenu.noteId)
    : null;
  const noteContextItems: ContextMenuItem[] = contextNote
    ? [
        {
          label: 'Cut',
          onClick: () => {
            if (copySelectedNotes()) void removeSelectedNotes();
            setNoteContextMenu(null);
          },
        },
        {
          label: 'Copy',
          onClick: () => {
            copySelectedNotes();
            setNoteContextMenu(null);
          },
        },
        {
          label: 'Quantize',
          disabled: snapTicks === 0,
          onClick: () => {
            setNoteContextMenu(null);
            const ids = selectedNoteIds.length ? selectedNoteIds : [contextNote.id];
            quantize(notes.filter((note) => ids.includes(note.id)));
          },
        },
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            void removeSelectedNotes();
            setNoteContextMenu(null);
          },
        },
        {
          label: 'Duplicate',
          onClick: () => {
            void duplicateSelectedNotes();
            setNoteContextMenu(null);
          },
        },
      ]
    : [];

  return {
    notes,
    selectedNotes,
    selectedNoteIds,
    setSelectedNoteIds,
    selectedVelocity,
    velocityDraft,
    handleVelocityChange,
    commitVelocity,
    setLastNoteLength,
    newNoteLength,
    selectAddedNotes,
    addNoteAt,
    runQuantize,
    quantizeTargets,
    handleEditorKeyDown,
    noteContextMenu,
    setNoteContextMenu,
    noteContextItems,
    mutations,
  };
}
