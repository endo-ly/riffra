import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import type { MidiClip, MidiNote } from '@/model/generated';
import type { MidiGhostNote } from './model/ghost-notes';
import { clipGridTicks, snapClipTick, snapGridTicks, type SnapGrid } from '@/model/clip-grid';
import { formatMusicalLength, formatMusicalPosition } from '@/model/time-format';
import { musicalGridTicks, snapToBar, ticksPerBar, ticksPerBeat } from '@/model/timebase';
import { midiNoteName } from '@/shared/midi';
import { useSession, useSessionContext } from '@/shared/session/session-context';
import { useInstrumentMidiInspection } from '@/shared/session/useInstrumentMidiInspection';
import { selectRuntimeReady, selectTrackInstrumentPlayable } from '@/shared/session/selectors';
import { logNativeError } from '@/native/invoke';
import { usePointerGesture } from '@/shared/usePointerGesture';
import { ContextMenu } from '@/shared/ui/ContextMenu';
import { collectGhostNotes } from './model/ghost-notes';
import { PITCH_HIGH, PITCH_LOW, pitchRowTop } from './model/pitch-grid';
import { MidiEditorRuler } from './MidiEditorRuler';
import { MidiEditorToolbar } from './MidiEditorToolbar';
import { MidiVelocityLane } from './MidiVelocityLane';
import { NoteLane } from './NoteLane';
import { PianoKeyboard } from './PianoKeyboard';
import { LANE_LABEL_WIDTH, useMidiEditorViewport } from './hooks/useMidiEditorViewport';
import { useMidiPreview } from './hooks/useMidiPreview';
import { useNoteDrag } from './hooks/useNoteDrag';
import { useNoteEditing } from './hooks/useNoteEditing';
import styles from './MidiEditorPanel.module.css';

type MidiEditorTool = 'pointer' | 'draw';

const EMPTY_GHOSTS: MidiGhostNote[] = [];

interface MidiEditorPanelProps {
  clip: MidiClip | null;
  /** Color of the track that owns the clip; notes are drawn in it. */
  trackColor: string | null;
}

export function MidiEditorPanel({ clip, trackColor }: MidiEditorPanelProps) {
  const { api } = useSessionContext();
  const arrangement = useSession((state) => state.project.boot!.canonical.session.arrangement);
  const transport = useSession((state) => state.transport);
  const playing = useSession((state) => state.transport.status?.state === 'playing');
  const runtimeReady = useSession(selectRuntimeReady);
  const playable = useSession((state) =>
    selectTrackInstrumentPlayable(state, clip?.trackId ?? null),
  );
  const { inspection } = useInstrumentMidiInspection(clip?.trackId ?? null, arrangement.revision);
  const ghostNotes = useMemo(
    () => collectGhostNotes(clip, arrangement.midiClips, arrangement.tracks),
    [clip, arrangement.midiClips, arrangement.tracks],
  );
  const instrumentSettings = inspection?.settings ?? null;
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
  const [ghostsVisible, setGhostsVisible] = useState(true);
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
  const {
    pixelsPerTick,
    rowHeight,
    visibleTicks,
    laneViewportRef,
    rulerContentRef,
    velocityContentRef,
    rulerPlayheadRef,
    lanePlayheadRef,
    velocityPlayheadRef,
  } = useMidiEditorViewport(clip, transport.displayTick, transport.playhead, playing);
  const laneHeight = (PITCH_HIGH - PITCH_LOW) * rowHeight;
  const velocityLaneHeight = 88;
  const laneWidth = visibleTicks * pixelsPerTick;
  const canvasWidth = LANE_LABEL_WIDTH + laneWidth;
  const beatTicks = ticksPerBeat(arrangement.timebase);
  const barTicks = ticksPerBar(arrangement.timebase);
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
  const beatLineTicks = musicalGridTicks(
    clipStartTick,
    clipStartTick + visibleTicks,
    'beat',
    arrangement.timebase,
  ).map((tick) => tick - clipStartTick);
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

  const { previewNoteOn, previewNoteOff, auditionNote } = useMidiPreview({
    clipId,
    trackId: clip?.trackId ?? null,
    previewEnabled,
    previewAvailable: runtimeReady && playable,
    velocity: lastUsedVelocity,
    timebase: arrangement.timebase,
    clipStartTick,
    onSendMidi: (trackId, bytes) =>
      api.sendMidiToTrack(trackId, bytes).catch(logNativeError('sendMidiToTrack')),
    onPanicMidi: (trackId) => api.panicMidiTrack(trackId).catch(logNativeError('panicMidiTrack')),
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
    playheadTickRef: transport.playhead,
    auditionNote,
    onClearTransient: () => setMarquee(null),
  });
  const { dragging, handlePointerDown, suppressNoteClickRef, resetDrag } = useNoteDrag({
    clip,
    selectedNoteIds: editing.selectedNoteIds,
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
    requestAnimationFrame(() => editorRef.current?.focus());
  }, [clipId]);

  const { setSelectedNoteIds, setNoteContextMenu } = editing;
  useEffect(() => {
    startGesture.cancel();
    setSelectedNoteIds([]);
    resetDrag();
    setMarquee(null);
    setDrawPreview(null);
    setNoteContextMenu(null);
  }, [clipId, hasClip, startGesture, resetDrag, setSelectedNoteIds, setNoteContextMenu]);

  if (!clip) return <div className={styles.empty} />;

  const notes = editing.notes;
  const ghostLayer = ghostsVisible ? ghostNotes : EMPTY_GHOSTS;
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
      {inspection?.instrumentId && (
        <div className={styles.instrumentMidiInfo} aria-label="Instrument MIDI configuration">
          <span>
            Transpose {instrumentSettings!.transposeSemitones > 0 ? '+' : ''}
            {instrumentSettings!.transposeSemitones} st
          </span>
          <span>
            {instrumentSettings!.playableNotes?.type === 'range'
              ? `Playable ${midiNoteName(instrumentSettings!.playableNotes.start)}–${midiNoteName(instrumentSettings!.playableNotes.end)}`
              : instrumentSettings!.playableNotes?.type === 'set'
                ? `${instrumentSettings!.playableNotes.notes.length} playable notes`
                : 'All notes playable'}
          </span>
          {instrumentSettings!.keyswitches.map((keyswitch) => (
            <span key={`${keyswitch.note}:${keyswitch.name}`}>
              {midiNoteName(keyswitch.note)}: {keyswitch.name}
            </span>
          ))}
          {(inspection.warnings.length ?? 0) > 0 && (
            <strong role="status">
              {inspection.warnings.length} note warning
              {inspection.warnings.length === 1 ? '' : 's'}
            </strong>
          )}
        </div>
      )}
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
              onSeek={(tick) => void transport.seek(tick)}
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
              selectedNoteIds={editing.selectedNoteIds}
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
              ghostNotes={ghostLayer}
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
                notes={notes}
                selectedNoteIds={editing.selectedNoteIds}
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
}
