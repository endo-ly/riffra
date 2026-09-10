import { Fragment, useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import type {
  AutomationParameter,
  AudioStatus,
  BuiltInInstrumentSummary,
  CanonicalState,
  CreativeSession,
  PluginEntry,
  RuntimeProjectionStatus,
  TrackKind,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from './arrange-api';
import { ArrangeRuler } from './timeline/ArrangeRuler';
import { ArrangeToolbar } from './timeline/ArrangeToolbar';
import { ArrangeTrack } from './timeline/ArrangeTrack';
import { AutomationLaneView } from './timeline/AutomationLaneView';
import type { MidiGhostNote } from './midi-editor/MidiEditorPanel';
import { ArrangeDetailArea } from './ArrangeDetailArea';
import { ArrangeOverlays, type ArrangeConfirmRequest } from './ArrangeOverlays';
import { ArrangeMidiEditor } from './ArrangeMidiEditor';
import { ArrangePlayhead } from './components/ArrangePlayhead';
import { PlaySurfacePanel, type PlaySurfaceMode } from './play-surface/PlaySurfacePanel';
import { ToolbarButton } from '@/shared/ui/Toolbar';
import {
  buildTrackTimeline,
  timelineObjectEndTick,
  formatClock,
  formatMusicalPosition,
  ticksPerBar,
  ticksPerBeat,
  timelineGridDensity,
  TRACK_HEADER_WIDTH,
  type ArrangeTool,
  type SnapGrid,
  type TrackSize,
} from '@/features/arrange/model/arrange-timeline';
import { RIFFRA_ASSET_MIME } from '@/shared/asset-drag';
import { HostConnectionChangedError, getHostGeneration } from '@/native/invoke';
import { isEditableTarget } from '@/features/arrange/model/interaction';
import { useArrangeEditor, type ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import { useArrangeStatusToast } from '@/features/arrange/hooks/useArrangeStatusToast';
import { useArrangeDetailController } from '@/features/arrange/hooks/useArrangeDetailController';
import { useArrangeRulerController } from '@/features/arrange/hooks/useArrangeRulerController';
import { useArrangeTransport } from '@/features/arrange/hooks/useArrangeTransport';
import { useArrangeViewport } from '@/features/arrange/hooks/useArrangeViewport';
import {
  useArrangeContextMenus,
  type ArrangePluginPickerRequest,
} from '@/features/arrange/hooks/useArrangeContextMenus';
import { useArrangeDrop } from '@/features/arrange/hooks/useArrangeDrop';
import { useWaveformAnalyses } from '@/features/arrange/hooks/useWaveformAnalyses';
import styles from './WorkspaceArrange.module.css';

interface WorkspaceArrangeProps {
  hostGeneration?: number;
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  selection: ArrangeSelection;
  setSelection: (selection: ArrangeSelection) => void;
  api: ArrangeWorkspaceApi;
  audio: AudioStatus;
  focusedTrackId: string | null;
  onFocusTrack: (trackId: string | null) => void;
  onToggleTransport: () => void;
  runtimeProjectionStatus: RuntimeProjectionStatus;
  runtimeProjectionFailure: string | null;
  onRetryRuntimeProjection: () => Promise<void>;
  missingDeviceIds?: string[];
  plugins?: PluginEntry[];
  builtInInstruments?: BuiltInInstrumentSummary[];
  playSurfaceHost: HTMLElement | null;
}

export function WorkspaceArrange(props: WorkspaceArrangeProps) {
  const { arrangement } = props.session;
  const { timebase } = arrangement;
  const { api } = props;
  const { onToggleTransport } = props;
  const [tool, setTool] = useState<ArrangeTool>('select');
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const trackSize: TrackSize = 'normal';
  const [trackSizes, setTrackSizes] = useState<Record<string, TrackSize>>({});
  const [automationParameters, setAutomationParameters] = useState<
    Partial<Record<string, AutomationParameter>>
  >({});
  const [rulerMode, setRulerMode] = useState<'bars' | 'time'>('bars');
  const [confirmRequest, setConfirmRequest] = useState<ArrangeConfirmRequest | null>(null);
  const [playSurfaceMode, setPlaySurfaceMode] = useState<PlaySurfaceMode>('closed');
  const [playSurfaceSummary, setPlaySurfaceSummary] = useState('');
  const [emptyDragOver, setEmptyDragOver] = useState(false);
  const [pluginPicker, setPluginPicker] = useState<ArrangePluginPickerRequest | null>(null);
  const { transport, displayTick, displayTickRef, seekLocally } = useArrangeTransport(
    props.api,
    timebase,
    props.hostGeneration ?? 0,
  );
  const { scrollerRef, zoom, pixelsPerTick, applyZoom, zoomToRange, scrollTop } =
    useArrangeViewport({ timebase, transport, displayTickRef });
  const analyses = useWaveformAnalyses(
    props.api,
    arrangement.audioClips,
    props.hostGeneration ?? 0,
  );
  const barTicks = ticksPerBar(timebase);
  const timelineTicks = useMemo(() => {
    const contentEnd = Math.max(
      ...arrangement.audioClips.map((clip) => timelineObjectEndTick(clip, timebase)),
      ...arrangement.midiClips.map((clip) => timelineObjectEndTick(clip, timebase)),
      ...arrangement.automationLanes.flatMap((lane) => lane.points.map((point) => point.tick)),
      ...arrangement.markers.map((marker) => marker.tick),
      arrangement.loopRange.startTick,
      arrangement.loopRange.endTick,
      ...(arrangement.punchRange
        ? [arrangement.punchRange.startTick, arrangement.punchRange.endTick]
        : []),
      0,
    );
    return Math.max(barTicks * 16, contentEnd + barTicks * 2);
  }, [
    arrangement.audioClips,
    arrangement.automationLanes,
    arrangement.loopRange,
    arrangement.markers,
    arrangement.midiClips,
    arrangement.punchRange,
    barTicks,
    timebase,
  ]);
  const timelineWidth = timelineTicks * pixelsPerTick;
  const timelineGridStyle = useMemo(() => {
    const beatWidth = ticksPerBeat(timebase) * pixelsPerTick;
    const barWidth = barTicks * pixelsPerTick;
    const density = timelineGridDensity(timebase, pixelsPerTick);
    const layers = [
      `repeating-linear-gradient(90deg, rgba(211, 232, 235, 0.2) 0 1px, transparent 1px ${barWidth}px)`,
    ];
    if (density.showBeats) {
      layers.push(
        `repeating-linear-gradient(90deg, rgba(211, 232, 235, 0.09) 0 1px, transparent 1px ${beatWidth}px)`,
      );
    }
    if (density.subdivisionTicks) {
      const subdivisionWidth = density.subdivisionTicks * pixelsPerTick;
      layers.push(
        `repeating-linear-gradient(90deg, rgba(211, 232, 235, 0.045) 0 1px, transparent 1px ${subdivisionWidth}px)`,
      );
    }
    return { width: timelineWidth, backgroundImage: layers.join(', ') } as CSSProperties;
  }, [barTicks, pixelsPerTick, timebase, timelineWidth]);
  const editor = useArrangeEditor({
    ...props,
    tool,
    snap,
    pixelsPerTick,
    displayTick,
    analyses,
    onSplitToolUsed: () => setTool('select'),
  });
  const { commit, setMessage } = editor;
  const { handleDrop, isOsFileDrag } = useArrangeDrop({
    api: props.api,
    commit,
    hostGeneration: props.hostGeneration ?? 0,
    pixelsPerTick,
    snapTick: editor.snapTick,
    setMessage,
  });
  const sendMidiPreview = useCallback(
    (trackId: string, bytes: number[]) => api.sendMidiToTrack(trackId, bytes),
    [api],
  );
  const panicMidiPreview = useCallback((trackId: string) => api.panicMidiTrack(trackId), [api]);
  const { playbackOutOfSync } = useArrangeStatusToast({
    runtimeProjectionStatus: props.runtimeProjectionStatus,
    runtimeProjectionFailure: props.runtimeProjectionFailure ?? null,
    onRetryRuntimeProjection: props.onRetryRuntimeProjection,
    editorMessage: editor.message,
    unavailableClipIds: transport?.unavailableClipIds ?? [],
    missingDeviceIds: transport?.missingDeviceIds ?? [],
  });
  const selectedClipIds = props.selection.kind === 'clips' ? props.selection.clipIds : [];
  const selectedTrackId = props.selection.kind === 'track' ? props.selection.trackId : null;
  const focusedTrackId = props.focusedTrackId;
  const focusedTrack = arrangement.tracks.find((track) => track.id === focusedTrackId) ?? null;
  const trackClipCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const clip of arrangement.audioClips) {
      counts.set(clip.trackId, (counts.get(clip.trackId) ?? 0) + 1);
    }
    for (const clip of arrangement.midiClips) {
      counts.set(clip.trackId, (counts.get(clip.trackId) ?? 0) + 1);
    }
    return counts;
  }, [arrangement.audioClips, arrangement.midiClips]);
  const ruler = useArrangeRulerController({
    arrangement,
    api,
    commit: editor.commit,
    snapTick: editor.snapTick,
    pixelsPerTick,
    displayTickRef,
    selectedClipCount: selectedClipIds.length,
    seekLocally,
    setMessage: editor.setMessage,
  });
  const detail = useArrangeDetailController({
    midiClips: arrangement.midiClips,
    selectClip: editor.selectClip,
  });
  const { activeMidiClip } = detail;
  const { handleKeyboard: handleRulerKeyboard, timeSelection: rulerTimeSelection } = ruler;
  const activeMidiTrack = activeMidiClip
    ? (arrangement.tracks.find((track) => track.id === activeMidiClip.trackId) ?? null)
    : null;
  // Notes from other clips that sound at the same arrangement time, rebased
  // to the active clip's local time so the editor can show them behind it.
  const midiGhostNotes = useMemo<MidiGhostNote[]>(() => {
    if (!activeMidiClip) return [];
    return arrangement.midiClips.flatMap((clip) => {
      if (clip.id === activeMidiClip.id) return [];
      return clip.notes.flatMap((note) => {
        const localTick = clip.startTick + note.startTick - activeMidiClip.startTick;
        if (localTick < 0 || localTick >= activeMidiClip.durationTicks) return [];
        return [
          {
            id: `${clip.id}:${note.id}`,
            pitch: note.note,
            startTick: localTick,
            durationTicks: Math.max(
              1,
              Math.min(note.durationTicks, activeMidiClip.durationTicks - localTick),
            ),
          },
        ];
      });
    });
  }, [arrangement.midiClips, activeMidiClip]);
  const runtimeReady =
    !playbackOutOfSync &&
    props.audio.state !== 'starting' &&
    props.audio.state !== 'faulted' &&
    props.audio.state !== 'offline';
  const activeInstrumentUnavailable = Boolean(
    activeMidiTrack?.instrument?.source.type === 'vst3' &&
    (activeMidiTrack.instrument.source.disabledPlaceholder ||
      props.missingDeviceIds?.includes(activeMidiTrack.instrument.id) ||
      transport?.missingDeviceIds.includes(activeMidiTrack.instrument.id)),
  );
  const midiPreviewAvailable = Boolean(
    runtimeReady &&
    activeMidiTrack?.kind === 'instrument' &&
    activeMidiTrack.instrument &&
    !activeInstrumentUnavailable,
  );

  const detailControls = (
    <>
      <ToolbarButton
        icon={detail.collapsed ? 'expand' : 'collapse'}
        ariaLabel={detail.collapsed ? 'Restore detail area' : 'Collapse detail area'}
        title={detail.collapsed ? 'Restore detail area' : 'Collapse detail area'}
        onClick={() => detail.setCollapsed(!detail.collapsed)}
      />
      <ToolbarButton
        icon={detail.maximized ? 'restore' : 'maximize'}
        ariaLabel={detail.maximized ? 'Restore detail area size' : 'Maximize detail area'}
        title={detail.maximized ? 'Restore detail area size' : 'Maximize detail area'}
        onClick={() => detail.setMaximized(!detail.maximized)}
      />
      <ToolbarButton
        icon="close"
        ariaLabel="Close detail area"
        title="Close detail area"
        onClick={detail.close}
      />
    </>
  );

  // One shell-level keyboard boundary coordinates transport, zoom, and the
  // ruler controller without adding competing window listeners in child hooks.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (handleRulerKeyboard(event) || event.defaultPrevented) return;
      if (event.key === ' ' && !isEditableTarget(event.target)) {
        event.preventDefault();
        onToggleTransport();
        return;
      }
      if (event.key.toLowerCase() === 'z' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        if (!rulerTimeSelection || isEditableTarget(event.target)) return;
        event.preventDefault();
        zoomToRange(rulerTimeSelection.startTick, rulerTimeSelection.endTick);
        return;
      }
      if (event.key.toLowerCase() === 'f' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        if (isEditableTarget(event.target)) return;
        const clipEdges = [
          ...arrangement.audioClips.flatMap((clip) => [
            clip.startTick,
            timelineObjectEndTick(clip, timebase),
          ]),
          ...arrangement.midiClips.flatMap((clip) => [
            clip.startTick,
            timelineObjectEndTick(clip, timebase),
          ]),
        ];
        if (!clipEdges.length) return;
        event.preventDefault();
        zoomToRange(Math.min(...clipEdges), Math.max(...clipEdges));
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [
    arrangement.audioClips,
    arrangement.midiClips,
    handleRulerKeyboard,
    onToggleTransport,
    rulerTimeSelection,
    timebase,
    zoomToRange,
  ]);

  const setTrackSizeForTrack = (trackId: string, size: TrackSize) => {
    setTrackSizes((value) => ({ ...value, [trackId]: size }));
  };
  const toggleAutomation = (trackId: string) =>
    setAutomationParameters((current) => ({
      ...current,
      [trackId]: current[trackId] ? undefined : 'volume',
    }));

  const createEmptyMidiClip = async (trackId: string, rawTick: number) => {
    const track = arrangement.tracks.find((item) => item.id === trackId);
    if (!track || track.kind !== 'instrument') return;
    const beforeIds = new Set(arrangement.midiClips.map((clip) => clip.id));
    const startTick = ruler.timeSelection
      ? ruler.timeSelection.startTick
      : editor.snapTick(rawTick);
    const durationTicks = ruler.timeSelection
      ? Math.max(1, ruler.timeSelection.endTick - ruler.timeSelection.startTick)
      : Math.max(1, barTicks);
    const next = await editor.commit(api.createMidiClip(trackId, startTick, durationTicks));
    if (!next) return;
    const created = next.arrangement.midiClips.find((clip) => !beforeIds.has(clip.id));
    if (!created) return;
    ruler.clearTimeSelection();
    detail.openMidiEditor(created);
  };

  const seekMidiEditor = (tick: number) => {
    const nextTick = Math.max(0, Math.round(tick));
    seekLocally(nextTick);
    void props.api.seekTimeline(nextTick).catch((error) => {
      if (error instanceof HostConnectionChangedError) return;
      setMessage(String(error));
    });
  };

  const addTrack = (kind: TrackKind) =>
    editor.commit(
      api.addTrack(
        `${kind === 'audio' ? 'Audio' : 'Instrument'} ${
          arrangement.tracks.filter((track) => track.kind === kind).length + 1
        }`,
        kind,
      ),
    );

  const performDeleteTrack = async (trackId: string) => {
    const requestGeneration = getHostGeneration();
    const deletedTrack = arrangement.tracks.find((track) => track.id === trackId);
    if (
      props.focusedTrackId === trackId &&
      deletedTrack?.kind === 'instrument' &&
      deletedTrack.instrument
    ) {
      try {
        const status = await props.api.panicMidiTrack(trackId);
        if (getHostGeneration() !== requestGeneration) return;
        if (status) editor.setMessage(status.message);
      } catch (error) {
        if (error instanceof HostConnectionChangedError) return;
        editor.setMessage(String(error));
      }
    }
    if (getHostGeneration() !== requestGeneration) return;
    const next = await editor.commit(props.api.removeTrack(trackId));
    if (next) {
      if (props.focusedTrackId === trackId) {
        props.onFocusTrack(null);
      }
      const remaining = new Set([
        ...next.arrangement.audioClips.map((clip) => clip.id),
        ...next.arrangement.midiClips.map((clip) => clip.id),
      ]);
      const clipIds = selectedClipIds.filter((id) => remaining.has(id));
      props.setSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
    }
  };

  const deleteTrack = (trackId: string, name: string, clipCount: number) => {
    const detail = clipCount
      ? ` This also removes ${clipCount} Clip${clipCount === 1 ? '' : 's'} from the Timeline.`
      : '';
    setConfirmRequest({
      title: `Delete ${name}`,
      message: `${detail}\n\nSource assets will be kept.`,
      confirmLabel: 'Delete Track',
      danger: true,
      onConfirm: () => {
        setConfirmRequest(null);
        void performDeleteTrack(trackId);
      },
    });
  };

  const menus = useArrangeContextMenus({
    arrangement,
    api,
    editor,
    ruler,
    detail,
    snap,
    timebase,
    displayTick,
    setPluginPicker,
    addTrack,
    deleteTrack,
    trackClipCounts,
    createEmptyMidiClip,
  });

  return (
    <section
      className={styles.workspace}
      aria-label="Arrange timeline"
      data-arrange-workspace
      style={{ '--header-width': `${TRACK_HEADER_WIDTH}px` } as CSSProperties}
    >
      <ArrangeToolbar
        tool={tool}
        snap={snap}
        zoom={zoom}
        rulerMode={rulerMode}
        onTool={setTool}
        onSnap={setSnap}
        onZoom={applyZoom}
        onRulerMode={setRulerMode}
        automationAvailable={selectedTrackId !== null}
        automationOpen={selectedTrackId !== null && Boolean(automationParameters[selectedTrackId])}
        onToggleAutomation={() => {
          if (selectedTrackId) toggleAutomation(selectedTrackId);
        }}
        playSurfaceAvailable={focusedTrack?.kind === 'instrument'}
        playSurfaceOpen={playSurfaceMode !== 'closed'}
        onTogglePlaySurface={() =>
          setPlaySurfaceMode(playSurfaceMode === 'closed' ? 'expanded' : 'closed')
        }
      />

      <ArrangeOverlays
        api={props.api}
        plugins={props.plugins}
        builtInInstruments={props.builtInInstruments ?? []}
        commit={commit}
        ruler={ruler}
        contextMenu={menus.contextMenu}
        onCloseContextMenu={menus.closeContextMenu}
        confirmRequest={confirmRequest}
        onDismissConfirm={() => setConfirmRequest(null)}
        pluginPicker={pluginPicker}
        setPluginPicker={setPluginPicker}
      />

      <div
        ref={scrollerRef}
        className={styles.scroller}
        onWheel={(event) => {
          if (!event.ctrlKey) return;
          event.preventDefault();
          applyZoom(zoom * (event.deltaY > 0 ? 0.9 : 1.1), event.clientX);
        }}
      >
        <div
          data-arrange-timeline
          className={styles.timeline}
          style={{ width: TRACK_HEADER_WIDTH + timelineWidth }}
          onPointerDown={editor.beginMarquee}
          onContextMenu={(event) => {
            if (event.target !== event.currentTarget) return;
            menus.openTrackAreaContextMenu(event, null);
          }}
        >
          <ArrangeRuler
            timebase={timebase}
            timelineTicks={timelineTicks}
            timelineWidth={timelineWidth}
            pixelsPerTick={pixelsPerTick}
            mode={rulerMode}
            position={formatMusicalPosition(displayTick, timebase)}
            clock={formatClock(displayTick, timebase)}
            scrollTop={scrollTop}
            loopRange={ruler.loopPreview ?? arrangement.loopRange}
            punchRange={ruler.punchPreview ?? arrangement.punchRange}
            markers={arrangement.markers}
            selectedMarkerId={ruler.selectedMarkerId}
            selectedRange={ruler.selectedRange}
            timeSelection={ruler.timeSelection}
            onPointerDown={ruler.seekFromRuler}
            onLoopHandle={ruler.dragLoopHandle}
            onPunchHandle={ruler.dragPunchHandle}
            onSelectRange={ruler.selectRange}
            onRulerContextMenu={menus.openRulerContextMenu}
            onRangeContextMenu={menus.openRangeContextMenu}
            onMarkerContextMenu={menus.openMarkerContextMenu}
            onAddMarker={ruler.addMarkerAt}
            onMoveMarker={ruler.moveMarker}
            onRenameMarker={ruler.renameMarker}
            onRemoveMarker={ruler.removeMarker}
            onSelectMarker={ruler.selectMarker}
          />
          <div
            data-timeline-grid
            aria-hidden="true"
            className={styles.timelineGrid}
            style={timelineGridStyle}
          />
          {transport && transport.recordingPhase !== 'idle' && (
            <div
              className={styles.recordingPreview}
              style={{
                left: TRACK_HEADER_WIDTH + transport.recordingStartTick * pixelsPerTick,
                width: Math.max(1, displayTick - transport.recordingStartTick) * pixelsPerTick,
              }}
            >
              {transport.recordingPhase.toUpperCase()} ·{' '}
              {transport.armedTrackIds
                .map(
                  (trackId) =>
                    arrangement.tracks.find((track) => track.id === trackId)?.name ?? trackId,
                )
                .join(' · ')}{' '}
              · PASS {transport.recordingPassOrdinal}
            </div>
          )}
          <ArrangePlayhead
            positionRef={displayTickRef}
            positionTick={displayTick}
            pixelsPerTick={pixelsPerTick}
            playing={transport?.state === 'playing'}
          />
          {ruler.timeSelection && (
            <div
              data-time-selection-chip
              className={styles.selectionChip}
              style={{
                left:
                  TRACK_HEADER_WIDTH +
                  ((ruler.timeSelection.startTick + ruler.timeSelection.endTick) / 2) *
                    pixelsPerTick,
              }}
            >
              <span>
                {formatMusicalPosition(ruler.timeSelection.startTick, timebase)} →{' '}
                {formatMusicalPosition(ruler.timeSelection.endTick, timebase)}
              </span>
              <button onClick={ruler.setLoopToSelection}>Set Loop</button>
              <button onClick={ruler.setPunchToSelection}>Set Punch</button>
            </div>
          )}
          {editor.snapGuide != null && (
            <div
              className={styles.snapGuide}
              style={{ left: TRACK_HEADER_WIDTH + editor.snapGuide * pixelsPerTick }}
            />
          )}
          {editor.marquee && <div className={styles.marquee} style={editor.marquee} />}

          {arrangement.tracks.length === 0 ? (
            <div
              className={`${styles.empty} ${emptyDragOver ? styles.emptyDragOver : ''}`}
              onDragOver={(event) => {
                if (!event.dataTransfer.types.includes(RIFFRA_ASSET_MIME) && !isOsFileDrag(event))
                  return;
                event.preventDefault();
                event.dataTransfer.dropEffect = 'copy';
              }}
              onDragEnter={() => setEmptyDragOver(true)}
              onDragLeave={(event) => {
                if (!event.currentTarget.contains(event.relatedTarget as Node))
                  setEmptyDragOver(false);
              }}
              onDrop={(event) => {
                setEmptyDragOver(false);
                handleDrop(event);
              }}
            >
              <span className={styles.emptyIcon}>≋</span>
              <strong>Start arranging</strong>
              <div className={styles.emptyActions}>
                <button onClick={() => void addTrack('audio')}>＋ Add Audio Track</button>
                <button onClick={() => void addTrack('instrument')}>＋ Add Instrument Track</button>
              </div>
            </div>
          ) : (
            arrangement.tracks.map((track, trackIndex) => (
              <Fragment key={track.id}>
                <ArrangeTrack
                  track={track}
                  trackIndex={trackIndex}
                  timeline={buildTrackTimeline(
                    track.id,
                    arrangement.audioClips,
                    arrangement.midiClips,
                    timebase,
                  )}
                  timebase={timebase}
                  analyses={analyses}
                  selectedClipIds={selectedClipIds}
                  unavailableClipIds={transport?.unavailableClipIds ?? []}
                  selected={
                    props.selection.kind === 'track' && props.selection.trackId === track.id
                  }
                  focused={props.focusedTrackId === track.id}
                  onSelectTrack={() => {
                    ruler.clearSelectedRange();
                    props.setSelection({ kind: 'track', trackId: track.id });
                    props.onFocusTrack(track.kind === 'instrument' ? track.id : null);
                  }}
                  timelineWidth={timelineWidth}
                  pixelsPerTick={pixelsPerTick}
                  trackSize={trackSizes[track.id] ?? trackSize}
                  api={props.api}
                  onCommit={editor.commit}
                  onDrop={(event, trackId, trackKind) => {
                    handleDrop(event, trackId, trackKind);
                  }}
                  onContextMenu={menus.openTrackAreaContextMenu}
                  onDoubleClickLane={
                    track.kind === 'instrument'
                      ? (event, trackId, tick) => {
                          event.preventDefault();
                          void createEmptyMidiClip(trackId, tick);
                        }
                      : undefined
                  }
                  onMove={editor.beginMove}
                  onMoveMidi={editor.beginMidiMove}
                  onTrimMidi={editor.beginMidiTrim}
                  onSelect={(clipId, append = false) => {
                    ruler.clearSelectedRange();
                    editor.selectClip(clipId, append);
                    const selectedMidiClip = arrangement.midiClips.find(
                      (clip) => clip.id === clipId,
                    );
                    if (selectedMidiClip && !append && detail.view === 'midiEditor')
                      detail.keepSelectedMidiClipVisible(clipId);
                  }}
                  onTrim={editor.beginTrim}
                  onFade={editor.beginFade}
                  onOpenMidiEditor={(clip) => {
                    detail.openMidiEditor(clip);
                  }}
                  onAudioClipContextMenu={menus.openAudioClipContextMenu}
                  onMidiClipContextMenu={menus.openMidiClipContextMenu}
                  onRename={(name) => void editor.commit(props.api.updateTrack(track.id, { name }))}
                  onDuplicate={() => void editor.commit(props.api.duplicateTrack(track.id))}
                  onDelete={() =>
                    void deleteTrack(track.id, track.name, trackClipCounts.get(track.id) ?? 0)
                  }
                  missingDeviceIds={[
                    ...(props.missingDeviceIds ?? []),
                    ...(transport?.missingDeviceIds ?? []),
                  ]}
                  onAddDevice={() =>
                    setPluginPicker({
                      trackId: track.id,
                      kind: track.kind === 'audio' ? 'effect' : 'instrument',
                    })
                  }
                  onOpenPluginEditor={(deviceId) => {
                    void props.api
                      .openTrackPluginEditor(track.id, deviceId)
                      .catch((error: unknown) => {
                        editor.setMessage(error instanceof Error ? error.message : String(error));
                      });
                  }}
                  onReorder={(sourceTrackId, insertAfter) => {
                    const sourceIndex = arrangement.tracks.findIndex(
                      (candidate) => candidate.id === sourceTrackId,
                    );
                    if (sourceIndex < 0 || sourceIndex === trackIndex) return;
                    const targetIndex = insertAfter
                      ? sourceIndex < trackIndex
                        ? trackIndex
                        : trackIndex + 1
                      : sourceIndex < trackIndex
                        ? trackIndex - 1
                        : trackIndex;
                    void editor.commit(props.api.reorderTrack(sourceTrackId, targetIndex));
                  }}
                  onSetTrackSize={(size) => setTrackSizeForTrack(track.id, size)}
                />
                {automationParameters[track.id] && (
                  <AutomationLaneView
                    track={track}
                    lane={arrangement.automationLanes.find(
                      (lane) =>
                        lane.trackId === track.id &&
                        lane.parameter === automationParameters[track.id],
                    )}
                    parameter={automationParameters[track.id]!}
                    timelineWidth={timelineWidth}
                    pixelsPerTick={pixelsPerTick}
                    snapTick={editor.snapTick}
                    onParameter={(parameter) =>
                      setAutomationParameters((current) => ({
                        ...current,
                        [track.id]: parameter,
                      }))
                    }
                    onCommit={(points) =>
                      void editor.commit(
                        props.api.setTrackAutomation(
                          track.id,
                          automationParameters[track.id]!,
                          points,
                        ),
                      )
                    }
                  />
                )}
              </Fragment>
            ))
          )}
        </div>
      </div>

      <ArrangeDetailArea
        view={detail.view}
        height={detail.height}
        collapsed={detail.collapsed}
        maximized={detail.maximized}
        onCollapsedChange={detail.setCollapsed}
        onHeightChange={detail.setHeight}
        collapsedControls={detailControls}
        midiEditor={
          <ArrangeMidiEditor
            clip={activeMidiClip}
            timebase={timebase}
            ghostNotes={midiGhostNotes}
            playheadTick={displayTick}
            playheadTickRef={displayTickRef}
            playing={transport?.state === 'playing'}
            onSeek={seekMidiEditor}
            previewAvailable={midiPreviewAvailable}
            onSendMidi={sendMidiPreview}
            onPanicMidi={panicMidiPreview}
            toolbarTrailing={detail.collapsed ? null : detailControls}
            api={props.api}
            commit={commit}
          />
        }
      />

      <PlaySurfacePanel
        host={props.playSurfaceHost}
        mode={playSurfaceMode}
        track={focusedTrack}
        summary={playSurfaceSummary}
        onModeChange={setPlaySurfaceMode}
        audio={props.audio}
        api={props.api}
        runtimeReady={runtimeReady}
        missingDeviceIds={[
          ...(props.missingDeviceIds ?? []),
          ...(transport?.missingDeviceIds ?? []),
        ]}
        onChooseInstrument={() => {
          if (focusedTrack) setPluginPicker({ trackId: focusedTrack.id, kind: 'instrument' });
        }}
        onSummaryChange={setPlaySurfaceSummary}
      />
    </section>
  );
}
