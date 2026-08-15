import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CreativeSession, MidiClip, MidiNote, ProjectTimebase } from '@/model/domain';
import {
  SNAP_GRID_OPTIONS,
  snapGridLabel,
  snapGridTicks,
  ticksPerBar,
  ticksPerBeat,
  countOffGridNotes,
} from '@/features/arrange/model/arrange-timeline';
import type { SnapGrid } from '@/features/arrange/model/arrange-timeline';
import { isBlackKey, midiNoteName } from '@/features/arrange/play-surface/musical-typing';
import { isEditableTarget } from '@/features/arrange/model/interaction';
import { toast } from '@/shared/toasts';
import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import {
  Toolbar,
  ToolbarButton,
  ToolbarDivider,
  ToolbarSegmented,
  ToolbarSelect,
  ToolbarSlider,
  ToolbarStepper,
  ToolbarToggle,
} from '@/shared/ui/Toolbar';
import { MidiEditorRuler } from './MidiEditorRuler';
import { MidiVelocityLane } from './MidiVelocityLane';
import styles from './MidiEditorPanel.module.css';

type MidiEditResult = void | PromiseLike<CreativeSession | null>;
type MidiEditorTool = 'pointer' | 'draw';
interface MidiNoteInput {
  pitch: number;
  startTick: number;
  durationTicks: number;
  velocity: number;
  channel: number;
}

const ZOOM_STEP = 1.25;

interface MidiEditorPanelProps {
  clip: MidiClip | null;
  timebase: ProjectTimebase;
  onUpdateNote?: (clipId: string, note: MidiNote) => void | PromiseLike<CreativeSession | null>;
  onUpdateNotes?: (
    clipId: string,
    updates: { noteId: string; patch: Partial<MidiNote> }[],
  ) => void | PromiseLike<CreativeSession | null>;
  onRemoveNotes: (clipId: string, noteIds: string[]) => MidiEditResult;
  onAddNote?: (
    clipId: string,
    startTick: number,
    pitch: number,
    durationTicks: number,
    velocity: number,
    channel: number,
  ) => MidiEditResult;
  onInsertNotes?: (clipId: string, notes: MidiNoteInput[]) => MidiEditResult;
  onQuantize?: (clipId: string, noteIds: string[], gridTicks: number) => MidiEditResult;
  onDuplicateNotes?: (clipId: string, noteIds: string[], offsetTicks: number) => MidiEditResult;
  playheadTick?: number;
  onSeek?: (tick: number) => void;
  previewAvailable?: boolean;
  onSendMidi?: (trackId: string, bytes: number[]) => Promise<unknown>;
  onPanicMidi?: (trackId: string) => Promise<unknown>;
}

const PITCH_HIGH = 128;
const PITCH_LOW = 0;
const EMPTY_NOTES: MidiNote[] = [];

function pitchFromClientY(
  clientY: number,
  boundsTop: number,
  rowHeight: number,
  clampToRange = false,
) {
  const row = Math.floor((clientY - boundsTop) / rowHeight);
  const pitch = PITCH_HIGH - 1 - row;
  if (clampToRange) return Math.max(PITCH_LOW, Math.min(PITCH_HIGH - 1, pitch));
  return pitch >= PITCH_LOW && pitch < PITCH_HIGH ? pitch : null;
}

function pitchRowTop(pitch: number, rowHeight: number) {
  return (PITCH_HIGH - pitch - 1) * rowHeight;
}

export function MidiEditorPanel(props: MidiEditorPanelProps) {
  const { clip } = props;
  const { onPanicMidi } = props;
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [dragging, setDragging] = useState<{
    gestureId: number;
    noteId: string;
    previewNotes: Record<string, MidiNote>;
    awaitingCanonical: boolean;
  } | null>(null);
  const [tool, setTool] = useState<MidiEditorTool>('pointer');
  const [selectedNoteIds, setSelectedNoteIds] = useState<string[]>([]);
  const [velocityDraft, setVelocityDraft] = useState(96);
  const [lastUsedVelocity, setLastUsedVelocity] = useState(96);
  const [pixelsPerTick, setPixelsPerTick] = useState(0.18);
  const [rowHeight, setRowHeight] = useState(12);
  const [horizontalScrollLeft, setHorizontalScrollLeft] = useState(0);
  const [previewEnabled, setPreviewEnabled] = useState(true);
  const [marquee, setMarquee] = useState<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);
  const [drawPreview, setDrawPreview] = useState<{
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
  const editorRef = useRef<HTMLDivElement>(null);
  const laneViewportRef = useRef<HTMLDivElement>(null);
  const centeredClipIdRef = useRef<string | undefined>(undefined);
  const previewHeldNotesRef = useRef<Set<number>>(new Set());
  const clipboardRef = useRef<{
    notes: (Omit<MidiNoteInput, 'startTick'> & { relativeStartTick: number })[];
  } | null>(null);
  const clipId = clip?.id;
  const hasClip = clip !== null;
  const clipNoteCenter = clip?.notes.length
    ? clip.notes.reduce((total, note) => total + note.note, 0) / clip.notes.length
    : 60;
  const visibleTicks = Math.max(props.clip?.durationTicks ?? 1920, 1920);
  const laneHeight = (PITCH_HIGH - PITCH_LOW) * rowHeight;
  const velocityLaneHeight = 88;
  const laneWidth = visibleTicks * pixelsPerTick;
  const canvasWidth = 48 + laneWidth;
  const beatTicks = ticksPerBeat(props.timebase);
  const barTicks = ticksPerBar(props.timebase);
  const snapTicks = snapGridTicks(snap, props.timebase);
  const subdivisionLineTicks = useMemo(() => {
    if (snapTicks <= 0) return [];
    const pixelsPerSubdivision = snapTicks * pixelsPerTick;
    const stride = Math.max(1, Math.ceil(8 / Math.max(1, pixelsPerSubdivision)));
    const step = snapTicks * stride;
    return Array.from({ length: Math.ceil(visibleTicks / step) }, (_, index) => index * step)
      .filter((tick) => tick > 0)
      .filter((tick) => tick % beatTicks !== 0 && tick % barTicks !== 0);
  }, [barTicks, beatTicks, pixelsPerTick, snapTicks, visibleTicks]);

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

  const previewTrackId = clip?.trackId ?? null;
  const previewCanSend = Boolean(
    previewEnabled && props.previewAvailable && previewTrackId && props.onSendMidi,
  );

  const releasePreviewNotes = useCallback(() => {
    if (!previewTrackId || previewHeldNotesRef.current.size === 0) return;
    previewHeldNotesRef.current.clear();
    void onPanicMidi?.(previewTrackId);
  }, [onPanicMidi, previewTrackId]);

  useEffect(() => {
    return releasePreviewNotes;
  }, [clipId, previewEnabled, props.previewAvailable, releasePreviewNotes]);

  const previewNoteOn = (pitch: number) => {
    if (!previewCanSend || !previewTrackId || previewHeldNotesRef.current.has(pitch)) return;
    previewHeldNotesRef.current.add(pitch);
    void props.onSendMidi?.(previewTrackId, [0x90, pitch, lastUsedVelocity]);
  };

  const previewNoteOff = (pitch: number) => {
    if (!previewTrackId || !previewHeldNotesRef.current.delete(pitch)) return;
    void props.onSendMidi?.(previewTrackId, [0x80, pitch, 0]);
  };

  const applyHorizontalZoom = useCallback(
    (next: number, clientX?: number) => {
      const bounded = Math.min(1, Math.max(0.05, next));
      const viewport = laneViewportRef.current;
      if (!viewport) {
        setPixelsPerTick(bounded);
        return;
      }
      const bounds = viewport.getBoundingClientRect();
      const cursor =
        clientX === undefined
          ? viewport.clientWidth / 2
          : Math.max(0, Math.min(viewport.clientWidth, clientX - bounds.left));
      const tick = Math.max(0, (viewport.scrollLeft + cursor - 48) / pixelsPerTick);
      setPixelsPerTick(bounded);
      requestAnimationFrame(() => {
        viewport.scrollLeft = Math.max(0, 48 + tick * bounded - cursor);
      });
    },
    [pixelsPerTick],
  );

  const applyVerticalZoom = useCallback(
    (next: number) => {
      const bounded = Math.min(24, Math.max(8, next));
      const viewport = laneViewportRef.current;
      if (!viewport) {
        setRowHeight(bounded);
        return;
      }
      const centerPitch =
        PITCH_HIGH - 1 - (viewport.scrollTop + viewport.clientHeight / 2) / rowHeight;
      setRowHeight(bounded);
      requestAnimationFrame(() => {
        viewport.scrollTop = Math.max(
          0,
          (PITCH_HIGH - 1 - centerPitch) * bounded - viewport.clientHeight / 2,
        );
      });
    },
    [rowHeight],
  );

  const removeSelectedNotes = (noteIds = selectedNoteIds): MidiEditResult => {
    if (!clip || noteIds.length === 0) return;
    const uniqueIds = [...new Set(noteIds)];
    setSelectedNoteIds([]);
    return props.onRemoveNotes(clip.id, uniqueIds);
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

  const pasteNotes = (): MidiEditResult => {
    if (!clip || !clipboardRef.current || !props.onInsertNotes) return;
    const anchor = Math.max(0, Math.round(props.playheadTick ?? 0));
    const beforeIds = new Set(clip.notes.map((note) => note.id));
    const inputs = clipboardRef.current.notes.map(({ relativeStartTick, ...note }) => ({
      ...note,
      startTick: anchor + relativeStartTick,
    }));
    const operation = props.onInsertNotes(clip.id, inputs);
    if (operation === undefined) return;
    void Promise.resolve(operation).then((next) => {
      if (!next) return;
      const nextClip = next.arrangement.midiClips.find((candidate) => candidate.id === clip.id);
      if (!nextClip) return;
      setSelectedNoteIds(
        nextClip.notes.filter((note) => !beforeIds.has(note.id)).map((note) => note.id),
      );
    });
    return operation;
  };

  const duplicateSelectedNotes = (): MidiEditResult => {
    if (!clip || selectedNotes.length === 0) return;
    const start = Math.min(...selectedNotes.map((note) => note.startTick));
    const end = Math.max(
      ...selectedNotes.map((note) => note.startTick + Math.max(1, note.durationTicks)),
    );
    return props.onDuplicateNotes?.(clip.id, selectedNoteIds, Math.max(1, end - start));
  };

  const nudgeSelectedNotes = (
    direction: 'left' | 'right' | 'up' | 'down',
    pitchStep = 1,
  ): MidiEditResult => {
    if (!clip || selectedNotes.length === 0 || !props.onUpdateNotes) return;
    const timeStep = snapTicks || beatTicks;
    const requestedPitchDelta =
      direction === 'up' ? pitchStep : direction === 'down' ? -pitchStep : 0;
    const lowestSelectedPitch = Math.min(...selectedNotes.map((note) => note.note));
    const highestSelectedPitch = Math.max(...selectedNotes.map((note) => note.note));
    const pitchDelta =
      requestedPitchDelta > 0
        ? Math.min(requestedPitchDelta, PITCH_HIGH - 1 - highestSelectedPitch)
        : Math.max(requestedPitchDelta, PITCH_LOW - lowestSelectedPitch);
    const timeDelta = direction === 'left' ? -timeStep : direction === 'right' ? timeStep : 0;
    if (pitchDelta === 0 && timeDelta === 0) return;
    return props.onUpdateNotes(
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
    durationTicks = snapTicks || beatTicks,
  ): MidiEditResult => {
    if (!clip || !props.onAddNote) return;
    const bounds = lane.getBoundingClientRect();
    const rawTick = (clientX - bounds.left) / pixelsPerTick;
    const rawPitch = pitchFromClientY(clientY, bounds.top, rowHeight);
    if (rawPitch === null) return;
    const requestedStartTick = snapTicks
      ? Math.max(0, Math.round(rawTick / snapTicks) * snapTicks)
      : Math.max(0, Math.round(rawTick));
    const startTick = Math.min(Math.max(0, clip.durationTicks - 1), requestedStartTick);
    const beforeIds = new Set(clip.notes.map((note) => note.id));
    const operation = props.onAddNote(
      clip.id,
      startTick,
      rawPitch,
      Math.max(1, Math.round(durationTicks)),
      lastUsedVelocity,
      1,
    );
    if (operation === undefined) return;
    void Promise.resolve(operation).then((next) => {
      if (!next) return;
      const nextClip = next.arrangement.midiClips.find((candidate) => candidate.id === clip.id);
      if (!nextClip) return;
      setSelectedNoteIds(
        nextClip.notes.filter((note) => !beforeIds.has(note.id)).map((note) => note.id),
      );
    });
    return operation;
  };

  const handleEditorKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (isEditableTarget(event.target)) return;
    const key = event.key.toLowerCase();
    const modifier = event.ctrlKey || event.metaKey;
    let operation: MidiEditResult | undefined;

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
      setMarquee(null);
      setNoteContextMenu(null);
    } else {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    void Promise.resolve(operation);
  };

  useEffect(() => {
    if (!clipId) return;
    requestAnimationFrame(() => editorRef.current?.focus());
  }, [clipId]);

  useEffect(() => {
    setVelocityDraft(selectedVelocity);
  }, [selectedVelocity]);

  useEffect(() => {
    setSelectedNoteIds([]);
    setDragging(null);
    dragGestureRef.current += 1;
    setMarquee(null);
    setDrawPreview(null);
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
  }, [clipId, clipNoteCenter, hasClip, rowHeight]);

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

  const runQuantize = () => {
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
  };

  const handleVelocityChange = (value: number) => {
    setVelocityDraft(value);
    setLastUsedVelocity(value);
  };

  const commitVelocity = (value: number) => {
    setLastUsedVelocity(value);
    void props.onUpdateNotes?.(
      props.clip!.id,
      selectedNoteIds.map((noteId) => ({ noteId, patch: { velocity: value } })),
    );
  };

  const contextNote = noteContextMenu
    ? props.clip.notes.find((note) => note.id === noteContextMenu.noteId)
    : null;
  const noteContextItems: ContextMenuItem[] = contextNote
    ? [
        {
          label: 'Cut',
          onClick: () => {
            if (copySelectedNotes()) void Promise.resolve(removeSelectedNotes());
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
          disabled: snapTicks === 0 || !props.onQuantize,
          onClick: () => {
            setNoteContextMenu(null);
            const ids = selectedNoteIds.length ? selectedNoteIds : [contextNote.id];
            const selected = props.clip!.notes.filter((note) => ids.includes(note.id));
            const offGrid = countOffGridNotes(selected, snapTicks);
            if (offGrid === 0) {
              toast('Selected notes are already on the grid.');
              return;
            }
            void Promise.resolve(props.onQuantize?.(props.clip!.id, ids, snapTicks));
          },
        },
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            void Promise.resolve(removeSelectedNotes());
            setNoteContextMenu(null);
          },
        },
        {
          label: 'Duplicate',
          onClick: () => {
            void Promise.resolve(duplicateSelectedNotes());
            setNoteContextMenu(null);
          },
        },
      ]
    : [];

  const handlePointerDown = (
    event: React.PointerEvent<HTMLSpanElement>,
    note: MidiNote,
    mode: 'move' | 'resize',
  ) => {
    editorRef.current?.focus();
    event.stopPropagation();
    const originX = event.clientX;
    const originY = event.clientY;
    const movingNoteIds =
      selectedNoteIds.includes(note.id) && selectedNoteIds.length > 1 ? selectedNoteIds : [note.id];
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
        const snappedStart = snapTicks
          ? Math.round((note.startTick + deltaTicks) / snapTicks) * snapTicks
          : Math.round(note.startTick + deltaTicks);
        const minTickDelta = -Math.min(...originNotes.map((candidate) => candidate.startTick));
        const tickDelta = Math.max(minTickDelta, snappedStart - note.startTick);
        const rawPitchDelta = Math.round((originY - clientY) / rowHeight);
        const minPitchDelta = Math.max(
          ...originNotes.map((candidate) => PITCH_LOW - candidate.note),
        );
        const maxPitchDelta = Math.min(
          ...originNotes.map((candidate) => PITCH_HIGH - 1 - candidate.note),
        );
        const pitchDelta = Math.max(minPitchDelta, Math.min(maxPitchDelta, rawPitchDelta));
        return {
          ...note,
          startTick: note.startTick + tickDelta,
          note: note.note + pitchDelta,
        };
      }
      const snappedDuration = snapTicks
        ? Math.round((note.durationTicks + deltaTicks) / snapTicks) * snapTicks
        : Math.round(note.durationTicks + deltaTicks);
      const minDurationDelta = Math.max(
        ...originNotes.map((candidate) => 1 - candidate.durationTicks),
      );
      const durationDelta = Math.max(minDurationDelta, snappedDuration - note.durationTicks);
      return { ...note, durationTicks: note.durationTicks + durationDelta };
    };
    const buildPreviewNotes = (nextNote: MidiNote) => {
      if (originNotes.length < 2) return { [nextNote.id]: nextNote };
      if (mode === 'move') {
        const tickDelta = nextNote.startTick - note.startTick;
        const pitchDelta = nextNote.note - note.note;
        return Object.fromEntries(
          originNotes.map((candidate) => [
            candidate.id,
            {
              ...candidate,
              startTick: candidate.startTick + tickDelta,
              note: candidate.note + pitchDelta,
            },
          ]),
        ) as Record<string, MidiNote>;
      }
      const durationDelta = nextNote.durationTicks - note.durationTicks;
      return Object.fromEntries(
        originNotes.map((candidate) => [
          candidate.id,
          { ...candidate, durationTicks: candidate.durationTicks + durationDelta },
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
        const updates = originNotes.map((candidate) => {
          const next = previewNotes[candidate.id];
          return {
            noteId: candidate.id,
            patch:
              mode === 'move'
                ? { startTick: next.startTick, note: next.note }
                : { durationTicks: next.durationTicks },
          };
        });
        if (props.onUpdateNotes && originNotes.length > 1) {
          operation = props.onUpdateNotes(props.clip!.id, updates);
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
    <div
      ref={editorRef}
      className={styles.editor}
      aria-label="MIDI Editor"
      tabIndex={0}
      onKeyDown={handleEditorKeyDown}
      onPointerDown={(event) => {
        if (!(event.target as HTMLElement).closest('button, input, select')) {
          editorRef.current?.focus();
        }
      }}
    >
      <Toolbar
        label="MIDI Editor toolbar"
        trailing={
          <>
            <ToolbarStepper
              label="Time"
              ariaLabel="MIDI Editor zoom"
              onStep={(direction) =>
                applyHorizontalZoom(pixelsPerTick * (direction > 0 ? ZOOM_STEP : 1 / ZOOM_STEP))
              }
            />
            <ToolbarStepper
              label="Pitch"
              ariaLabel="MIDI Editor pitch zoom"
              onStep={(direction) => applyVerticalZoom(rowHeight + (direction > 0 ? 2 : -2))}
            />
          </>
        }
      >
        <ToolbarSegmented
          label="MIDI Editor tool"
          value={tool}
          onChange={setTool}
          options={[
            { value: 'pointer', label: 'Pointer', icon: 'pointer' },
            { value: 'draw', label: 'Draw', icon: 'pencil' },
          ]}
        />
        <ToolbarDivider />
        <ToolbarSelect
          label="Snap"
          value={snap}
          onChange={setSnap}
          options={SNAP_GRID_OPTIONS.map((value) => ({ value, label: snapGridLabel(value) }))}
        />
        <ToolbarToggle
          icon="speaker"
          ariaLabel="Preview"
          active={previewEnabled}
          disabled={!props.previewAvailable}
          title={
            props.previewAvailable
              ? 'Preview notes on the active instrument'
              : 'Audio runtime unavailable'
          }
          onClick={() => setPreviewEnabled((enabled) => !enabled)}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon="magnet"
          ariaLabel="Quantize"
          disabled={!selectedNoteIds.length || snapTicks === 0 || !props.onQuantize}
          title={
            snapTicks === 0
              ? 'Select a snap grid before quantizing'
              : 'Quantize notes to the snap grid'
          }
          onClick={runQuantize}
        />
        <ToolbarButton
          icon="copy"
          ariaLabel="Duplicate"
          disabled={!selectedNoteIds.length}
          title="Duplicate the selected notes"
          onClick={() => void Promise.resolve(duplicateSelectedNotes())}
        />
        <ToolbarSlider
          label="Vel"
          ariaLabel="Selected MIDI note velocity"
          value={velocityDraft}
          min={1}
          max={127}
          disabled={!selectedNoteIds.length}
          onChange={handleVelocityChange}
          onCommit={commitVelocity}
        />
      </Toolbar>
      <div className={styles.editorSurface}>
        <div className={styles.rulerViewport}>
          <div className={styles.laneLabel}>Ruler</div>
          <div
            className={styles.rulerContent}
            style={{
              width: laneWidth,
              transform: `translate3d(${-horizontalScrollLeft}px, 0, 0)`,
            }}
          >
            <MidiEditorRuler
              timebase={props.timebase}
              clipStartTick={clip!.startTick}
              visibleTicks={visibleTicks}
              pixelsPerTick={pixelsPerTick}
              playheadTick={props.playheadTick}
              onSeek={props.onSeek}
            />
          </div>
        </div>
        <div
          ref={laneViewportRef}
          className={styles.laneViewport}
          data-midi-pitch-viewport
          onScroll={(event) => setHorizontalScrollLeft(event.currentTarget.scrollLeft)}
        >
          <div className={styles.roll} style={{ width: canvasWidth }}>
            <div
              className={styles.pitchKeyboard}
              style={{ height: laneHeight }}
              aria-label="MIDI piano keyboard"
            >
              {pitchRows.map((pitch) => (
                <div
                  key={pitch}
                  className={styles.pianoKey}
                  style={{
                    top: pitchRowTop(pitch, rowHeight),
                    height: rowHeight,
                  }}
                  data-piano-key={pitch}
                  role="button"
                  tabIndex={0}
                  aria-label={`Preview ${midiNoteName(pitch)}`}
                  onPointerDown={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    event.currentTarget.setPointerCapture?.(event.pointerId);
                    previewNoteOn(pitch);
                  }}
                  onPointerUp={(event) => {
                    event.stopPropagation();
                    previewNoteOff(pitch);
                  }}
                  onPointerCancel={() => previewNoteOff(pitch)}
                  onPointerLeave={() => previewNoteOff(pitch)}
                  onKeyDown={(event) => {
                    if (event.key !== 'Enter' && event.key !== ' ') return;
                    event.preventDefault();
                    event.stopPropagation();
                    previewNoteOn(pitch);
                  }}
                  onKeyUp={(event) => {
                    if (event.key !== 'Enter' && event.key !== ' ') return;
                    event.preventDefault();
                    event.stopPropagation();
                    previewNoteOff(pitch);
                  }}
                >
                  <span className={styles.pianoWhiteKey}>
                    {pitch % 12 === 0 ? midiNoteName(pitch) : null}
                  </span>
                  {isBlackKey(pitch % 12) && (
                    <span
                      className={styles.pianoBlackKey}
                      style={{
                        top: Math.max(1, Math.round(rowHeight * 0.08)),
                        height: Math.max(5, Math.round(rowHeight * 0.75)),
                      }}
                      aria-hidden="true"
                    />
                  )}
                </div>
              ))}
            </div>
            <div
              className={styles.lane}
              data-midi-lane
              style={{ height: laneHeight, width: laneWidth }}
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
                let cancel: () => void = () => undefined;
                const finish = (pointer: PointerEvent) => {
                  window.removeEventListener('pointermove', move);
                  window.removeEventListener('pointerup', finish);
                  window.removeEventListener('pointercancel', cancel);
                  const width = Math.abs(pointer.clientX - originX);
                  const height = Math.abs(pointer.clientY - originY);
                  if (draw) {
                    const left = Math.min(originX, pointer.clientX);
                    const right = Math.max(originX, pointer.clientX);
                    const rawStart = (left - bounds.left) / pixelsPerTick;
                    const rawEnd = (right - bounds.left) / pixelsPerTick;
                    const startTick = snapTicks
                      ? Math.max(0, Math.round(rawStart / snapTicks) * snapTicks)
                      : Math.max(0, Math.round(rawStart));
                    const endTick = snapTicks
                      ? Math.max(startTick, Math.round(rawEnd / snapTicks) * snapTicks)
                      : Math.max(startTick, Math.round(rawEnd));
                    const durationTicks = Math.max(
                      1,
                      endTick - startTick || snapTicks || beatTicks,
                    );
                    void Promise.resolve(addNoteAt(lane, left, originY, durationTicks));
                  } else if (width < 4 && height < 4) {
                    setSelectedNoteIds([]);
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
                    setSelectedNoteIds(append ? [...new Set([...selectedNoteIds, ...ids])] : ids);
                  }
                  setMarquee(null);
                  setDrawPreview(null);
                };
                cancel = () => {
                  window.removeEventListener('pointermove', move);
                  window.removeEventListener('pointerup', finish);
                  window.removeEventListener('pointercancel', cancel);
                  setMarquee(null);
                  setDrawPreview(null);
                };
                window.addEventListener('pointermove', move);
                window.addEventListener('pointerup', finish);
                window.addEventListener('pointercancel', cancel);
              }}
              onDoubleClick={(event) => {
                if ((event.target as HTMLElement).closest('[data-note-id]')) return;
                event.preventDefault();
                void Promise.resolve(addNoteAt(event.currentTarget, event.clientX, event.clientY));
              }}
            >
              {marquee && <div className={styles.marquee} style={marquee} />}
              {drawPreview && <div className={styles.drawPreview} style={drawPreview} />}
              {props.playheadTick !== undefined &&
                props.playheadTick >= 0 &&
                props.playheadTick <= visibleTicks && (
                  <i
                    className={styles.editorPlayhead}
                    style={{ left: props.playheadTick * pixelsPerTick }}
                  />
                )}
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
              {subdivisionLineTicks.map((tick) => (
                <i
                  key={`subdivision-${tick}`}
                  className={styles.subdivisionLine}
                  data-grid-subdivision
                  style={{ left: tick * pixelsPerTick }}
                />
              ))}
              {pitchRows.map((pitch) => (
                <div
                  key={pitch}
                  className={`${styles.pitchRow} ${isBlackKey(pitch % 12) ? styles.pitchBlack : ''} ${pitch % 12 === 0 ? styles.pitchOctave : ''}`}
                  style={{
                    top: pitchRowTop(pitch, rowHeight),
                    height: rowHeight,
                    lineHeight: `${rowHeight}px`,
                  }}
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
                        top: pitchRowTop(visibleNote.note, rowHeight),
                        width: Math.max(4, noteWidth),
                        height: rowHeight - 1,
                        opacity: Math.max(0.45, 0.4 + (visibleNote.velocity / 127) * 0.6),
                      }}
                      onPointerDown={(event) => handlePointerDown(event, note, 'move')}
                      onClick={(event) => {
                        event.stopPropagation();
                        editorRef.current?.focus();
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
        <div className={styles.velocityViewport} data-midi-velocity-viewport>
          <div className={styles.velocityRow} style={{ width: canvasWidth }}>
            <div className={styles.laneLabel}>Velocity</div>
            <div
              className={styles.velocityContent}
              style={{
                width: laneWidth,
                transform: `translate3d(${-horizontalScrollLeft}px, 0, 0)`,
              }}
            >
              <MidiVelocityLane
                notes={notes}
                selectedNoteIds={selectedNoteIds}
                visibleTicks={visibleTicks}
                pixelsPerTick={pixelsPerTick}
                barTicks={barTicks}
                beatTicks={beatTicks}
                height={velocityLaneHeight}
                playheadTick={props.playheadTick}
                clipId={clip!.id}
                onSelectNoteIds={setSelectedNoteIds}
                onUpdateNotes={props.onUpdateNotes}
                onFocus={() => editorRef.current?.focus()}
                onVelocityChange={setLastUsedVelocity}
              />
            </div>
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
