import { Fragment, useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import type {
  AutomationTarget,
  AudioStatus,
  CreativeSession,
  InstrumentLibraryItem,
  PluginEntry,
  RuntimeProjectionStatus,
  TrackKind,
} from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { ArrangeRuler } from './timeline/ArrangeRuler';
import { ArrangeToolbar } from './timeline/ArrangeToolbar';
import { ArrangeTrack } from './timeline/ArrangeTrack';
import { AutomationLaneEditor } from './timeline/AutomationLaneEditor';
import { automationOwner } from '@/model/automation';
import type { MidiGhostNote } from './midi-editor/MidiEditorPanel';
import { resolveDeviceTarget } from '@/features/devices/device-target';
import { DevicesPanel } from '@/features/devices/DevicesPanel';
import { ArrangeLowerArea } from './ArrangeLowerArea';
import { ArrangeOverlays, type ArrangeConfirmRequest } from './ArrangeOverlays';
import { ArrangeMidiEditor } from './ArrangeMidiEditor';
import { resolveTrackColor } from '@/model/track-color';
import { ArrangePlayhead } from './components/ArrangePlayhead';
import { PlaySurface } from './play-surface/PlaySurface';
import { midiNoteName } from '@/shared/midi';
import { useLivePlay } from './play-surface/useLivePlay';
import { ToolbarButton } from '@/shared/ui/Toolbar';
import { formatMusicalPosition } from '@/model/time-format';
import { ticksPerBar, ticksPerBeat } from '@/model/timebase';
import {
  buildTrackTimeline,
  timelineObjectEndTick,
  timelineGridDensity,
  TRACK_HEADER_WIDTH,
  type ArrangeTool,
  type SnapGrid,
  type TrackSize,
} from '@/features/arrange/model/arrange-timeline';
import { HostConnectionChangedError } from '@/native/invoke';
import { isEditableTypingTarget } from '@/shared/input';
import { useArrangeEditor } from '@/features/arrange/hooks/useArrangeEditor';
import { ownerSelection, selectionOwner, type ArrangeSelection } from './model/selection';
import { useArrangeStatusToast } from '@/features/arrange/hooks/useArrangeStatusToast';
import type { useArrangeLowerAreaController } from '@/features/arrange/hooks/useArrangeLowerAreaController';
import { useArrangeRulerController } from '@/features/arrange/hooks/useArrangeRulerController';
import type { ArrangementTransport } from '@/features/transport/hooks/useArrangementTransport';
import { useArrangeViewport } from '@/features/arrange/hooks/useArrangeViewport';
import { useArrangeContextMenus } from '@/features/arrange/hooks/useArrangeContextMenus';
import { isBrowserItemDrag, useArrangeDrop } from '@/features/arrange/hooks/useArrangeDrop';
import { useWaveformAnalyses } from '@/features/arrange/hooks/useWaveformAnalyses';
import { MixerPanel } from '@/features/mixer/MixerPanel';
import styles from './WorkspaceArrange.module.css';

interface WorkspaceArrangeProps {
  hostConnected: boolean;
  lower: ReturnType<typeof useArrangeLowerAreaController>;
  onDisableMissingPlugin: (deviceId: string) => Promise<void>;
  onReplaceMissingPlugin: (deviceId: string, newPath: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
  transport: ArrangementTransport;
  session: CreativeSession;
  selection: ArrangeSelection;
  setSelection: (selection: ArrangeSelection) => void;
  audio: AudioStatus;
  focusedTrackId: string | null;
  onFocusTrack: (trackId: string | null) => void;
  onToggleTransport: () => void;
  runtimeProjectionStatus: RuntimeProjectionStatus;
  runtimeProjectionFailure: string | null;
  runtimeProjectionRetrying: boolean;
  onRetryRuntimeProjection: () => Promise<void>;
  missingDeviceIds?: string[];
  plugins?: PluginEntry[];
  instruments?: InstrumentLibraryItem[];
}

export function WorkspaceArrange(props: WorkspaceArrangeProps) {
  const { arrangement } = props.session;
  const { timebase } = arrangement;
  const { api, hostGeneration } = useSessionContext();
  const { onToggleTransport } = props;
  const [tool, setTool] = useState<ArrangeTool>('select');
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const trackSize: TrackSize = 'normal';
  const [trackSizes, setTrackSizes] = useState<Record<string, TrackSize>>({});
  const [automationTargets, setAutomationTargets] = useState<
    Partial<Record<string, AutomationTarget>>
  >({});
  const [mixerAutomationTarget, setMixerAutomationTarget] = useState<AutomationTarget | null>(null);
  useEffect(() => {
    setAutomationTargets({});
    setMixerAutomationTarget(null);
  }, [props.session.sessionId]);
  const openAutomation = (target: AutomationTarget) => {
    const owner = automationOwner(arrangement, target);
    if (owner?.kind === 'track')
      setAutomationTargets((current) => ({ ...current, [owner.id]: target }));
    else if (owner) setMixerAutomationTarget(target);
  };
  const [rulerMode, setRulerMode] = useState<'bars' | 'time'>('bars');
  const [confirmRequest, setConfirmRequest] = useState<ArrangeConfirmRequest | null>(null);
  const [emptyDragOver, setEmptyDragOver] = useState(false);
  const [newTrackDragOver, setNewTrackDragOver] = useState(false);
  const { transport, displayTick, displayTickRef, seekLocally } = props.transport;
  const previewScope = `${hostGeneration}:${props.session.sessionId}`;
  const previewAutomation = (
    target: AutomationTarget,
    value: number | null,
    token: number,
  ): Promise<void> => {
    if (value !== null && transport?.state !== 'playing') return Promise.resolve();
    return api.previewAutomationValue(target, value, token).catch(() => undefined);
  };
  const { scrollerRef, zoom, pixelsPerTick, applyZoom, zoomToRange, scrollTop } =
    useArrangeViewport({ timebase, transport, displayTickRef });
  const analyses = useWaveformAnalyses(arrangement.audioClips);
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
      `repeating-linear-gradient(90deg, var(--timeline-grid-bar) 0 1px, transparent 1px ${barWidth}px)`,
    ];
    if (density.showBeats) {
      layers.push(
        `repeating-linear-gradient(90deg, var(--timeline-grid-beat) 0 1px, transparent 1px ${beatWidth}px)`,
      );
    }
    if (density.subdivisionTicks) {
      const subdivisionWidth = density.subdivisionTicks * pixelsPerTick;
      layers.push(
        `repeating-linear-gradient(90deg, var(--timeline-grid-subdivision) 0 1px, transparent 1px ${subdivisionWidth}px)`,
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
    commit,
    pixelsPerTick,
    snapTick: editor.snapTick,
    setMessage,
  });
  const sendMidiPreview = useCallback(
    (trackId: string, bytes: number[]) => api.sendMidiToTrack(trackId, bytes),
    [api],
  );
  const panicMidiPreview = useCallback((trackId: string) => api.panicMidiTrack(trackId), [api]);
  const projectionDiagnostics = props.runtimeProjectionStatus.activeDiagnostics;
  const unavailableClipIds = projectionDiagnostics?.unavailableClipIds ?? [];
  const missingDeviceIds = useMemo(
    () => [
      ...new Set([
        ...(props.missingDeviceIds ?? []),
        ...(projectionDiagnostics?.missingDeviceIds ?? []),
      ]),
    ],
    [projectionDiagnostics?.missingDeviceIds, props.missingDeviceIds],
  );
  const { playbackOutOfSync } = useArrangeStatusToast({
    runtimeProjectionStatus: props.runtimeProjectionStatus,
    runtimeProjectionFailure: props.runtimeProjectionFailure ?? null,
    runtimeProjectionRetrying: props.runtimeProjectionRetrying,
    onRetryRuntimeProjection: props.onRetryRuntimeProjection,
    editorMessage: editor.message,
  });
  const selectedClipIds = props.selection.kind === 'clips' ? props.selection.clipIds : [];
  const selectedTrackId = props.selection.kind === 'track' ? props.selection.trackId : null;
  const deviceTarget = useMemo(() => {
    const owner = selectionOwner(props.selection);
    return owner ? resolveDeviceTarget(arrangement, owner) : null;
  }, [arrangement, props.selection]);

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
    commit: editor.commit,
    snapTick: editor.snapTick,
    pixelsPerTick,
    displayTickRef,
    selectedClipCount: selectedClipIds.length,
    seekLocally,
    setMessage: editor.setMessage,
  });
  const { lower } = props;
  const { activeMidiClip } = lower;
  const { handleKeyboard: handleRulerKeyboard, timeSelection: rulerTimeSelection } = ruler;
  const soloActive = arrangement.tracks.some((track) => track.solo);
  const activeMidiTrackIndex = activeMidiClip
    ? arrangement.tracks.findIndex((track) => track.id === activeMidiClip.trackId)
    : -1;
  const activeMidiTrack = arrangement.tracks[activeMidiTrackIndex] ?? null;
  const activeMidiTrackColor = activeMidiTrack
    ? resolveTrackColor(activeMidiTrack, activeMidiTrackIndex)
    : null;
  // Notes from other clips that sound at the same arrangement time, rebased
  // to the active clip's local time so the editor can show them behind it.
  const midiGhostNotes = useMemo<MidiGhostNote[]>(() => {
    if (!activeMidiClip) return [];
    const trackColors = new Map(
      arrangement.tracks.map((track, index) => [track.id, resolveTrackColor(track, index)]),
    );
    return arrangement.midiClips.flatMap((clip) => {
      const trackColor = trackColors.get(clip.trackId);
      if (clip.id === activeMidiClip.id || !trackColor) return [];
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
            trackColor,
          },
        ];
      });
    });
  }, [arrangement.midiClips, arrangement.tracks, activeMidiClip]);
  const runtimeReady =
    !playbackOutOfSync &&
    props.audio.state !== 'starting' &&
    props.audio.state !== 'faulted' &&
    props.audio.state !== 'offline';
  const livePlay = useLivePlay({
    track: focusedTrack,
    runtimeReady,
    hostConnected: props.hostConnected,
    missingDeviceIds,
    surfaceOpen: lower.view === 'playSurface' && !lower.collapsed,
  });
  const { toggleComputerKeyboard } = livePlay;
  const activeInstrumentUnavailable = Boolean(
    activeMidiTrack?.instrument?.source.type === 'vst3' &&
    (activeMidiTrack.instrument.source.disabledPlaceholder ||
      missingDeviceIds.includes(activeMidiTrack.instrument.id)),
  );
  const midiPreviewAvailable = Boolean(
    runtimeReady &&
    activeMidiTrack?.kind === 'instrument' &&
    activeMidiTrack.instrument &&
    !activeInstrumentUnavailable,
  );

  const lowerControls = (
    <>
      <ToolbarButton
        icon={lower.collapsed ? 'expand' : 'collapse'}
        ariaLabel={lower.collapsed ? 'Restore lower area' : 'Collapse lower area'}
        title={lower.collapsed ? 'Restore lower area' : 'Collapse lower area'}
        onClick={() => lower.setCollapsed(!lower.collapsed)}
      />
      <ToolbarButton
        icon={lower.maximized ? 'restore' : 'maximize'}
        ariaLabel={lower.maximized ? 'Restore lower area size' : 'Maximize lower area'}
        title={lower.maximized ? 'Restore lower area size' : 'Maximize lower area'}
        onClick={() => lower.setMaximized(!lower.maximized)}
      />
      <ToolbarButton
        icon="close"
        ariaLabel="Close lower area"
        title="Close lower area"
        onClick={lower.close}
      />
    </>
  );

  // One shell-level keyboard boundary coordinates transport, zoom, and the
  // ruler controller without adding competing window listeners in child hooks.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (handleRulerKeyboard(event) || event.defaultPrevented) return;
      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        // A held shortcut toggles once; the repeats of the key belong to nothing.
        if (event.repeat) return;
        toggleComputerKeyboard();
        return;
      }
      if (event.key === ' ' && !isEditableTypingTarget(event.target)) {
        event.preventDefault();
        onToggleTransport();
        return;
      }
      if (event.key.toLowerCase() === 'z' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        if (!rulerTimeSelection || isEditableTypingTarget(event.target)) return;
        event.preventDefault();
        zoomToRange(rulerTimeSelection.startTick, rulerTimeSelection.endTick);
        return;
      }
      if (event.key.toLowerCase() === 'f' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        if (isEditableTypingTarget(event.target)) return;
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
    toggleComputerKeyboard,
    zoomToRange,
  ]);

  const setTrackSizeForTrack = (trackId: string, size: TrackSize) => {
    setTrackSizes((value) => ({ ...value, [trackId]: size }));
  };
  const toggleAutomation = (trackId: string) =>
    setAutomationTargets((current) => ({
      ...current,
      [trackId]: current[trackId] ? undefined : { kind: 'trackMix', trackId, parameter: 'volume' },
    }));

  const createEmptyMidiClip = async (trackId: string, rawTick: number) => {
    const track = arrangement.tracks.find((item) => item.id === trackId);
    if (!track || track.kind !== 'instrument') return;
    const startTick = ruler.timeSelection
      ? ruler.timeSelection.startTick
      : editor.snapTick(rawTick);
    const durationTicks = ruler.timeSelection
      ? Math.max(1, ruler.timeSelection.endTick - ruler.timeSelection.startTick)
      : Math.max(1, barTicks);
    const next = await editor.commit(api.createMidiClip(trackId, startTick, durationTicks));
    if (!next) return;
    const createdId = next.createdEntityIds.midiClips?.[0];
    const created = next.canonical.session.arrangement.midiClips.find(
      (clip) => clip.id === createdId,
    );
    if (!created) return;
    ruler.clearTimeSelection();
    lower.openMidiEditor(created);
  };

  const seekMidiEditor = (tick: number) => {
    const nextTick = Math.max(0, Math.round(tick));
    seekLocally(nextTick);
    void api.seekTimeline(nextTick).catch((error) => {
      if (error instanceof HostConnectionChangedError) return;
      setMessage(String(error));
    });
  };

  const addTrack = (kind: TrackKind) =>
    editor.commit(
      api.addTrack(
        `${kind === 'audio' ? 'Audio' : 'Instrument'} ${arrangement.tracks.filter((track) => track.kind === kind).length + 1}`,
        kind,
      ),
    );

  const performDeleteTrack = async (trackId: string) => {
    const deletedTrack = arrangement.tracks.find((track) => track.id === trackId);
    if (
      props.focusedTrackId === trackId &&
      deletedTrack?.kind === 'instrument' &&
      deletedTrack.instrument
    ) {
      try {
        const status = await api.panicMidiTrack(trackId);
        if (status) editor.setMessage(status.message);
      } catch (error) {
        if (error instanceof HostConnectionChangedError) return;
        editor.setMessage(String(error));
      }
    }
    const next = await editor.commit(api.removeTrack(trackId));
    if (next) {
      if (selectedTrackId === trackId && lower.view === 'devices') lower.close();
      if (props.focusedTrackId === trackId) {
        props.onFocusTrack(null);
      }
      const remaining = new Set([
        ...next.canonical.session.arrangement.audioClips.map((clip) => clip.id),
        ...next.canonical.session.arrangement.midiClips.map((clip) => clip.id),
      ]);
      const clipIds = selectedClipIds.filter((id) => remaining.has(id));
      if (props.selection.kind !== 'track' || selectedTrackId === trackId)
        props.setSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
    }
  };

  const deleteTrack = (trackId: string, name: string, clipCount: number) => {
    const clipDetail = clipCount
      ? ` This also removes ${clipCount} Clip${clipCount === 1 ? '' : 's'} from the Timeline.`
      : '';
    setConfirmRequest({
      title: `Delete ${name}`,
      message: `${clipDetail}\n\nSource assets will be kept.`,
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
    editor,
    ruler,
    lower,
    snap,
    timebase,
    displayTick,
    onOpenDevices: (trackId) => {
      props.setSelection({ kind: 'track', trackId });
      lower.openDevices();
    },
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
        automationOpen={selectedTrackId !== null && Boolean(automationTargets[selectedTrackId])}
        onToggleAutomation={() => {
          if (selectedTrackId) toggleAutomation(selectedTrackId);
        }}
        playSurfaceOpen={lower.view === 'playSurface'}
        onTogglePlaySurface={lower.togglePlaySurface}
        mixerOpen={lower.view === 'mixer'}
        onToggleMixer={lower.toggleMixer}
        computerKeyboardOn={livePlay.computerKeyboard}
        computerKeyboardRange={
          livePlay.settings.mode === 'pads'
            ? 'Pads'
            : midiNoteName(livePlay.settings.octave * 12 + 12)
        }
        computerKeyboardBlockedReason={livePlay.blockedReason}
        onToggleComputerKeyboard={toggleComputerKeyboard}
      />

      <ArrangeOverlays
        ruler={ruler}
        contextMenu={menus.contextMenu}
        onCloseContextMenu={menus.closeContextMenu}
        confirmRequest={confirmRequest}
        onDismissConfirm={() => setConfirmRequest(null)}
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
                if (!isBrowserItemDrag(event) && !isOsFileDrag(event)) return;
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
                  soloActive={soloActive}
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
                  unavailableClipIds={unavailableClipIds}
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
                    if (selectedMidiClip && !append && lower.view === 'midiEditor')
                      lower.keepSelectedMidiClipVisible(clipId);
                  }}
                  onTrim={editor.beginTrim}
                  onFade={editor.beginFade}
                  onOpenMidiEditor={(clip) => {
                    lower.openMidiEditor(clip);
                  }}
                  onAudioClipContextMenu={menus.openAudioClipContextMenu}
                  onMidiClipContextMenu={menus.openMidiClipContextMenu}
                  onRename={(name) => void editor.commit(api.updateTrack(track.id, { name }))}
                  onDuplicate={() => void editor.commit(api.duplicateTrack(track.id))}
                  onDelete={() =>
                    void deleteTrack(track.id, track.name, trackClipCounts.get(track.id) ?? 0)
                  }
                  onOpenDevices={() => {
                    props.setSelection({ kind: 'track', trackId: track.id });
                    lower.openDevices();
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
                    void editor.commit(api.reorderTrack(sourceTrackId, targetIndex));
                  }}
                  onSetTrackSize={(size) => setTrackSizeForTrack(track.id, size)}
                />
                {automationTargets[track.id] && (
                  <AutomationLaneEditor
                    warnings={props.audio.diagnostics.automationWarnings}
                    session={props.session}
                    target={automationTargets[track.id]!}
                    timelineWidth={timelineWidth}
                    pixelsPerTick={pixelsPerTick}
                    playheadTick={displayTick}
                    playheadTickRef={displayTickRef}
                    previewScope={previewScope}
                    snapTick={editor.snapTick}
                    onPreview={previewAutomation}
                    onTarget={openAutomation}
                    onClose={() =>
                      setAutomationTargets((current) => ({ ...current, [track.id]: undefined }))
                    }
                    onCommit={(target, interpolation, points) =>
                      editor.commit(api.setAutomation(target, interpolation, points))
                    }
                  />
                )}
              </Fragment>
            ))
          )}
          {mixerAutomationTarget && (
            <AutomationLaneEditor
              warnings={props.audio.diagnostics.automationWarnings}
              session={props.session}
              target={mixerAutomationTarget}
              timelineWidth={timelineWidth}
              pixelsPerTick={pixelsPerTick}
              playheadTick={displayTick}
              playheadTickRef={displayTickRef}
              previewScope={previewScope}
              snapTick={editor.snapTick}
              onPreview={previewAutomation}
              onTarget={openAutomation}
              onClose={() => setMixerAutomationTarget(null)}
              onCommit={(target, interpolation, points) =>
                editor.commit(api.setAutomation(target, interpolation, points))
              }
            />
          )}
          {arrangement.tracks.length > 0 && (
            <div
              className={`${styles.newTrackDrop} ${newTrackDragOver ? styles.newTrackDropActive : ''}`}
              onDragOver={(event) => {
                if (!isBrowserItemDrag(event) && !isOsFileDrag(event)) return;
                event.preventDefault();
                event.dataTransfer.dropEffect = 'copy';
              }}
              onDragEnter={() => setNewTrackDragOver(true)}
              onDragLeave={(event) => {
                if (!event.currentTarget.contains(event.relatedTarget as Node))
                  setNewTrackDragOver(false);
              }}
              onDrop={(event) => {
                setNewTrackDragOver(false);
                handleDrop(event);
              }}
            >
              {newTrackDragOver && <span>Drop to add to the Arrangement</span>}
            </div>
          )}
        </div>
      </div>

      <ArrangeLowerArea
        view={lower.view}
        height={lower.height}
        minimumHeight={lower.view === 'mixer' ? 240 : lower.view === 'playSurface' ? 220 : 180}
        collapsed={lower.collapsed}
        maximized={lower.maximized}
        onCollapsedChange={lower.setCollapsed}
        onHeightChange={lower.setHeight}
        controls={lowerControls}
        playSurface={
          <PlaySurface
            play={livePlay}
            trackColor={
              focusedTrack
                ? resolveTrackColor(focusedTrack, arrangement.tracks.indexOf(focusedTrack))
                : null
            }
            audio={props.audio}
            onChooseInstrument={() => {
              if (focusedTrack) {
                props.setSelection({ kind: 'track', trackId: focusedTrack.id });
                lower.openDevices();
              }
            }}
          />
        }
        midiEditor={
          <ArrangeMidiEditor
            clip={activeMidiClip}
            trackColor={activeMidiTrackColor}
            timebase={timebase}
            ghostNotes={midiGhostNotes}
            playheadTick={displayTick}
            playheadTickRef={displayTickRef}
            playing={transport?.state === 'playing'}
            onSeek={seekMidiEditor}
            previewAvailable={midiPreviewAvailable}
            onSendMidi={sendMidiPreview}
            onPanicMidi={panicMidiPreview}
            commit={commit}
          />
        }
        devices={
          <DevicesPanel
            automationLanes={arrangement.automationLanes}
            arrangement={arrangement}
            playing={transport?.state === 'playing'}
            currentTick={displayTick}
            onOpenAutomation={openAutomation}
            key={`${props.session.sessionId}:${JSON.stringify(deviceTarget?.owner)}:${hostGeneration}`}
            target={deviceTarget}
            sessionId={props.session.sessionId}
            plugins={props.plugins ?? []}
            instruments={props.instruments ?? []}
            missingDeviceIds={missingDeviceIds}
            onDisableMissingPlugin={props.onDisableMissingPlugin}
            onReplaceMissingPlugin={props.onReplaceMissingPlugin}
            onRescanMissingPlugins={props.onRescanMissingPlugins}
          />
        }
        mixer={
          <MixerPanel
            onOpenAutomation={openAutomation}
            session={props.session}
            selectedTrackId={selectedTrackId}
            selectedOwner={deviceTarget?.owner ?? null}
            onSelectOwner={(owner) => props.setSelection(ownerSelection(owner))}
            onOpenOwnerDevices={(owner) => {
              props.setSelection(ownerSelection(owner));
              lower.openDevices();
            }}
            missingDeviceIds={missingDeviceIds}
            onSelectTrack={(trackId) => {
              ruler.clearSelectedRange();
              props.setSelection({ kind: 'track', trackId });
            }}
            onOpenDevices={(trackId) => {
              props.setSelection({ kind: 'track', trackId });
              lower.openDevices();
            }}
            onError={setMessage}
          />
        }
      />
    </section>
  );
}
