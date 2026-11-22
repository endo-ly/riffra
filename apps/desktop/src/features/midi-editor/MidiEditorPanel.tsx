import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import type { MidiNote } from '@/model/generated';
import type { MidiGhostNote } from '@/features/midi-editor/model/ghost-notes';
import {
  clipGridTicks,
  showsBeatGrid,
  snapClipTick,
  snapGridTicks,
  type SnapGrid,
} from '@/model/clip-grid';
import { formatMusicalLength, formatMusicalPosition } from '@/model/time-format';
import { musicalGridTicks, snapToBar, ticksPerBar, ticksPerBeat } from '@/model/timebase';
import { resolveTrackColorById } from '@/model/track-color';
import { selectMidiClip, selectTrack } from '@/session/selectors';
import { midiNoteName } from '@/model/midi';
import { useSession, useSessionContext } from '@/session/session-context';
import { useInstrumentMidiInspection } from '@/session/hooks/useInstrumentMidiInspection';
import {
  selectRuntimeReady,
  selectTrackInstrumentPlayable,
  selectStoppedPlayheadTick,
} from '@/session/selectors';
import { logNativeError } from '@/native/invoke';
import { usePointerGesture } from '@/shared/hooks/usePointerGesture';
import { ContextMenu } from '@/shared/ui/ContextMenu';
import { collectGhostNotes } from '@/features/midi-editor/model/ghost-notes';
import { PITCH_HIGH, PITCH_LOW, pitchRowTop } from '@/features/midi-editor/model/pitch-grid';
import { MidiEditorRuler } from './MidiEditorRuler';
import { MidiEditorToolbar } from './MidiEditorToolbar';
import { MidiVelocityLane } from './MidiVelocityLane';
import { NoteLane } from './NoteLane';
import { PianoKeyboard } from './PianoKeyboard';
import { LANE_LABEL_WIDTH } from '@/features/midi-editor/model/pitch-grid';
import { useMidiEditorViewport } from '@/features/midi-editor/hooks/useMidiEditorViewport';
import { useMidiPreview } from '@/features/midi-editor/hooks/useMidiPreview';
import { useNoteDrag } from '@/features/midi-editor/hooks/useNoteDrag';
import { useNoteEditing } from '@/features/midi-editor/hooks/useNoteEditing';
import styles from './MidiEditorPanel.module.css';

type MidiEditorTool = 'pointer' | 'draw';

const EMPTY_GHOSTS: MidiGhostNote[] = [];

export const MidiEditorPanel = memo(function MidiEditorPanel() {
  const { api } = useSessionContext();
  const clip = useSession((state) => selectMidiClip(state, state.workspace.lower.activeMidiClipId));
  const arrangement = useSession((state) => state.project.boot!.canonical.session.arrangement);
  const trackColor = useSession((state) => {
    const owner = selectTrack(state, clip?.trackId ?? null);
    return owner
      ? resolveTrackColorById(state.project.boot!.canonical.session.arrangement.tracks, owner.id)
      : null;
  });
  const sessionId = useSession((state) => state.project.boot!.canonical.session.sessionId);
  const hostGeneration = useSession((state) => state.host.connection.generation);
  const playhead = useSession((state) => state.transport.playhead);
  const seek = useSession((state) => state.transport.seek);
  const stoppedTick = useSession(selectStoppedPlayheadTick);
  const playing = useSession((state) => state.transport.status?.state === 'playing');
  const runtimeReady = useSession(selectRuntimeReady);
  const playable = useSession((state) =>
    selectTrackInstrumentPlayable(state, clip?.trackId ?? null),
  );
  const { inspection } = useInstrumentMidiInspection(clip?.trackId ?? null);
  const [ghostsVisible, setGhostsVisible] = useState(true);
  const ghostNotes = useMemo(
    () =>
      ghostsVisible
        ? collectGhostNotes(clip, arrangement.midiClips, arrangement.tracks)
        : EMPTY_GHOSTS,
    [clip, arrangement.midiClips, arrangement.tracks, ghostsVisible],
  );
  const instrumentSettings = useSession(
    (state) => selectTrack(state, clip?.trackId ?? null)?.instrument?.midi ?? null,
  );
  const warnedNoteIds = useMemo(
    () => new Set(inspection?.warnings.map((warning) => warning.noteId) ?? []),
    [inspection],
  );
  const startGesture = usePointerGesture();
  const editorRef = useRef<HTMLDivElement>(null);
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [lastUsedVelocity, setLastUsedVelocity] = useState(96);
  const [hoverPitch, setHoverPitch] = useState<number | null>(null);
  const [tool, setTool] = useState<MidiEditorTool>('pointer');
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
  const clipId = clip?.id;
  const hasClip = clip !== null;
  const beatTicks = ticksPerBeat(arrangement.timebase);
  const barTicks = ticksPerBar(arrangement.timebase);
  const {
    pixelsPerTick,
    rowHeight,
    visibleTicks,
    stepHorizontalZoom,
    stepVerticalZoom,
    laneViewportRef,
    rulerContentRef,
    velocityContentRef,
    rulerPlayheadRef,
    lanePlayheadRef,
    velocityPlayheadRef,
  } = useMidiEditorViewport(clip, stoppedTick, playhead, playing, barTicks);
  const laneHeight = (PITCH_HIGH - PITCH_LOW) * rowHeight;
  const velocityLaneHeight = 88;
  const laneWidth = visibleTicks * pixelsPerTick;
  const canvasWidth = LANE_LABEL_WIDTH + laneWidth;
  const snapTicks = snapGridTicks(snap, arrangement.timebase);
  const clipStartTick = clip?.startTick ?? 0;
  const snapLocalTick = (tick: number, start: number, gridTicks: number) =>
    snap === 'bar'
      ? Math.max(0, snapToBar(start + tick, arrangement.timebase) - start)
      : snapClipTick(tick, start, gridTicks);
  /** Snaps a clip-local tick to the grid, or keeps it free while Alt is held. */
  const placeTick = (tick: number, free: boolean) =>
    free ? Math.max(0, Math.round(tick)) : snapLocalTick(tick, clipStartTick, snapTicks);
  const barLineTicks = musicalGridTicks(
    clipStartTick,
    clipStartTick + visibleTicks,
    'bar',
    arrangement.timebase,
  ).map((tick) => tick - clipStartTick);
  const beatLineTicks = showsBeatGrid(beatTicks * pixelsPerTick)
    ? musicalGridTicks(
        clipStartTick,
        clipStartTick + visibleTicks,
        'beat',
        arrangement.timebase,
      ).map((tick) => tick - clipStartTick)
    : [];
  const subdivisionLineTicks = useMemo(() => {
    if (snapTicks <= 0) return [];
    const pixelsPerSubdivision = snapTicks * pixelsPerTick;
    const stride = Math.max(1, Math.ceil(8 / Math.max(1, pixelsPerSubdivision)));
    const step = snapTicks * stride;
    return clipGridTicks(clipStartTick, visibleTicks, step).filter(
      (tick) => (clipStartTick + tick) % beatTicks !== 0 && (clipStartTick + tick) % barTicks !== 0,
    );
  }, [barTicks, beatTicks, clipStartTick, pixelsPerTick, snapTicks, visibleTicks]);
  const pitchRows = useMemo(
    () => Array.from({ length: PITCH_HIGH - PITCH_LOW }, (_, index) => PITCH_LOW + index).reverse(),
    [],
  );

  const sendMidi = useCallback(
    (trackId: string, bytes: number[]) =>
      api.sendMidiToTrack(trackId, bytes).catch(logNativeError('sendMidiToTrack')),
    [api],
  );
  const panicMidi = useCallback(
    (trackId: string) => api.panicMidiTrack(trackId).catch(logNativeError('panicMidiTrack')),
    [api],
  );
  const { previewNoteOn, previewNoteOff, auditionNote } = useMidiPreview({
    clipId,
    trackId: clip?.trackId ?? null,
    previewEnabled,
    previewAvailable: runtimeReady && playable,
    velocity: lastUsedVelocity,
    timebase: arrangement.timebase,
    clipStartTick,
    onSendMidi: sendMidi,
    onPanicMidi: panicMidi,
  });
  const editing = useNoteEditing({
    clip,
    snap,
    snapTicks,
    beatTicks,
    timebase: arrangement.timebase,
    pixelsPerTick,
    rowHeight,
    velocity: lastUsedVelocity,
    onVelocityChange: setLastUsedVelocity,
    placeTick,
    playheadTickRef: playhead,
    auditionNote,
    onClearTransient: () => setMarquee(null),
  });
  const { dragging, handlePointerDown, suppressNoteClickRef, resetDrag } = useNoteDrag({
    clip,
    selectedNoteIds: editing.selectedNoteIdSet,
    pixelsPerTick,
    rowHeight,
    placeTick,
    editorRef,
    startGesture,
    setLastNoteLength: editing.setLastNoteLength,
    auditionNote,
    selectAddedNotes: editing.selectAddedNotes,
    onInsertNotes: editing.mutations.insertNotes,
    onUpdateNotes: editing.mutations.updateNotes,
    onUpdateNote: editing.mutations.updateNote,
  });

  useEffect(() => {
    if (!clipId) return;
    const frame = requestAnimationFrame(() => editorRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [clipId]);

  const { setSelectedNoteIds, setNoteContextMenu } = editing;
  useEffect(() => {
    startGesture.cancel();
    setSelectedNoteIds([]);
    resetDrag();
    setMarquee(null);
    setDrawPreview(null);
    setNoteContextMenu(null);
  }, [
    clipId,
    hasClip,
    hostGeneration,
    sessionId,
    startGesture,
    resetDrag,
    setSelectedNoteIds,
    setNoteContextMenu,
  ]);

  if (!clip) return <div className={styles.empty} />;

  const notes = editing.notes;
  const noteBox = (note: MidiNote): CSSProperties => ({
    left: note.startTick * pixelsPerTick,
    top: pitchRowTop(note.note, rowHeight),
    width: Math.max(4, note.durationTicks * pixelsPerTick),
    height: rowHeight - 1,
    opacity: Math.max(0.45, 0.4 + (note.velocity / 127) * 0.6),
  });
  // While a drag is shown, the dragged note names where it lands: pitch and
  // start for a move, length for a resize.
  const draggedNote = dragging?.awaitingCanonical
    ? undefined
    : dragging?.previewNotes[dragging.noteId];
  const dragReadout =
    dragging && draggedNote
      ? {
          text:
            dragging.mode === 'move'
              ? `${midiNoteName(draggedNote.note)} · ${formatMusicalPosition(clipStartTick + draggedNote.startTick, arrangement.timebase)}`
              : formatMusicalLength(draggedNote.durationTicks, arrangement.timebase),
          style: {
            left: draggedNote.startTick * pixelsPerTick,
            top: Math.max(0, pitchRowTop(draggedNote.note, rowHeight) - 18),
          },
        }
      : null;

  return (
    <div
      ref={editorRef}
      className={styles.editor}
      style={trackColor ? ({ '--track-color': trackColor } as CSSProperties) : undefined}
      aria-label="MIDI Editor"
      data-midi-editor-clip-id={clipId}
      tabIndex={0}
      onKeyDown={editing.handleEditorKeyDown}
      onPointerDown={(event) => {
        if (!(event.target as HTMLElement).closest('button, input, select')) {
          editorRef.current?.focus();
        }
      }}
    >
      <MidiEditorToolbar
        clipName={clip.name}
        tool={tool}
        onToolChange={setTool}
        snap={snap}
        onSnapChange={setSnap}
        snapTicks={snapTicks}
        hasQuantizeTargets={editing.quantizeTargets.length > 0}
        hasSelectedNotes={editing.selectedNotes.length > 0}
        hasSelectedIds={editing.selectedNoteIds.length > 0}
        onQuantize={editing.runQuantize}
        velocityDraft={editing.velocityDraft}
        onVelocityChange={editing.handleVelocityChange}
        onVelocityCommit={editing.commitVelocity}
        previewEnabled={previewEnabled}
        previewAvailable={runtimeReady && playable}
        onTogglePreview={() => setPreviewEnabled((enabled) => !enabled)}
        ghostsVisible={ghostsVisible}
        onToggleGhosts={() => setGhostsVisible((visible) => !visible)}
        onTimeZoom={stepHorizontalZoom}
        onPitchZoom={stepVerticalZoom}
      />
      <div className={styles.editorSurface}>
        <div className={styles.rulerViewport}>
          <div className={styles.laneLabel} />
          <div ref={rulerContentRef} className={styles.rulerContent} style={{ width: laneWidth }}>
            <MidiEditorRuler
              timebase={arrangement.timebase}
              clipStartTick={clip.startTick}
              visibleTicks={visibleTicks}
              pixelsPerTick={pixelsPerTick}
              playheadRef={rulerPlayheadRef}
              onSeek={(tick) => void seek(tick)}
            />
          </div>
        </div>
        <div ref={laneViewportRef} className={styles.laneViewport} data-midi-pitch-viewport>
          <div className={styles.roll} style={{ width: canvasWidth }}>
            <PianoKeyboard
              pitches={pitchRows}
              rowHeight={rowHeight}
              laneHeight={laneHeight}
              hoverPitch={hoverPitch}
              instrumentSettings={instrumentSettings}
              onNoteOn={previewNoteOn}
              onNoteOff={previewNoteOff}
            />
            <NoteLane
              clip={clip}
              laneHeight={laneHeight}
              laneWidth={laneWidth}
              pixelsPerTick={pixelsPerTick}
              rowHeight={rowHeight}
              hoverPitch={hoverPitch}
              setHoverPitch={setHoverPitch}
              tool={tool}
              setNoteContextMenu={editing.setNoteContextMenu}
              editorRef={editorRef}
              setDrawPreview={setDrawPreview}
              setMarquee={setMarquee}
              setSelectedNoteIds={editing.setSelectedNoteIds}
              selectedNoteIds={editing.selectedNoteIdSet}
              placeTick={placeTick}
              newNoteLength={editing.newNoteLength}
              addNoteAt={editing.addNoteAt}
              startGesture={startGesture}
              marquee={marquee}
              drawPreview={drawPreview}
              lanePlayheadRef={lanePlayheadRef}
              barLineTicks={barLineTicks}
              beatLineTicks={beatLineTicks}
              subdivisionLineTicks={subdivisionLineTicks}
              ghostNotes={ghostNotes}
              pitchRows={pitchRows}
              notes={notes}
              dragging={dragging}
              warnedNoteIds={warnedNoteIds}
              noteBox={noteBox}
              handlePointerDown={handlePointerDown}
              suppressNoteClickRef={suppressNoteClickRef}
              dragReadout={dragReadout}
            />
          </div>
        </div>
        <div className={styles.velocityViewport} data-midi-velocity-viewport>
          <div className={styles.velocityRow} style={{ width: canvasWidth }}>
            <div className={styles.laneLabel}>Vel</div>
            <div
              ref={velocityContentRef}
              className={styles.velocityContent}
              style={{ width: laneWidth }}
            >
              <MidiVelocityLane
                editScope={`${hostGeneration}:${sessionId}:${clip.id}`}
                notes={notes}
                selectedNoteIds={editing.selectedNoteIdSet}
                visibleTicks={visibleTicks}
                pixelsPerTick={pixelsPerTick}
                barLineTicks={barLineTicks}
                beatLineTicks={beatLineTicks}
                height={velocityLaneHeight}
                playheadRef={velocityPlayheadRef}
                clipId={clip.id}
                onSelectNoteIds={editing.setSelectedNoteIds}
                onUpdateNotes={editing.mutations.updateNotes}
                onFocus={() => editorRef.current?.focus()}
                onVelocityChange={setLastUsedVelocity}
              />
            </div>
          </div>
        </div>
      </div>
      {editing.noteContextMenu && editing.noteContextItems.length > 0 && (
        <ContextMenu
          x={editing.noteContextMenu.x}
          y={editing.noteContextMenu.y}
          items={editing.noteContextItems}
          onClose={() => editing.setNoteContextMenu(null)}
        />
      )}
    </div>
  );
});
