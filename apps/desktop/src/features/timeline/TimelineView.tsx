import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import type { AutomationTarget } from '@/model/generated';
import { useSession, useSessionContext } from '@/shared/session/session-context';
import { TimelineRuler } from '@/features/timeline/TimelineRuler';
import { TimelineToolbar } from '@/features/timeline/TimelineToolbar';
import { TimelineTrackList } from '@/features/timeline/TimelineTrackList';
import { TimelineOverlays } from './TimelineOverlays';
import { TimelinePlayhead } from '@/features/timeline/TimelinePlayhead';
import { formatMusicalPosition } from '@/model/time-format';
import {
  timelineGridBackground,
  timelineLengthTicks,
  TRACK_HEADER_WIDTH,
  type TimelineTool,
  type TrackSize,
} from '@/features/timeline/model/timeline-layout';
import type { SnapGrid } from '@/model/clip-grid';
import { useTimelineEditor } from '@/features/timeline/hooks/useTimelineEditor';
import { useTimelineRulerController } from '@/features/timeline/hooks/useTimelineRulerController';
import { useTimelineViewport } from '@/features/timeline/hooks/useTimelineViewport';
import { useTimelineContextMenus } from '@/features/timeline/hooks/useTimelineContextMenus';
import { useTimelineDrop } from '@/features/timeline/hooks/useTimelineDrop';
import { useTimelineShortcuts } from '@/features/timeline/hooks/useTimelineShortcuts';
import { useTimelineTrackOperations } from '@/features/timeline/hooks/useTimelineTrackOperations';
import { useWaveformAnalyses } from '@/features/timeline/hooks/useWaveformAnalyses';
import styles from './TimelineView.module.css';

export interface TimelineLiveInputControls {
  on: boolean;
  rangeLabel: string;
  blockedReason: string | null;
  onToggle(): void;
}

export function TimelineView({ liveInput }: { liveInput: TimelineLiveInputControls }) {
  const { api } = useSessionContext();
  const session = useSession((state) => state.project.boot!.canonical.session);
  const workspace = useSession((state) => state.workspace);
  const transport = useSession((state) => state.transport);
  const runtime = useSession((state) => state.runtime);
  const hostGeneration = useSession((state) => state.host.connection.generation);
  const [tool, setTool] = useState<TimelineTool>('select');
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [trackSizes, setTrackSizes] = useState<Record<string, TrackSize>>({});
  const [rulerMode, setRulerMode] = useState<'bars' | 'time'>('bars');
  const { arrangement } = session;
  const { timebase } = arrangement;
  const { status: transportStatus, displayTick, playhead } = transport;
  const previewScope = `${hostGeneration}:${session.sessionId}`;
  const previewAutomation = (
    target: AutomationTarget,
    value: number | null,
    token: number,
  ): Promise<void> => {
    if (value !== null && transportStatus?.state !== 'playing') return Promise.resolve();
    return api.previewAutomationValue(target, value, token).catch(() => undefined);
  };
  const { scrollerRef, zoom, pixelsPerTick, applyZoom, zoomToRange, scrollTop } =
    useTimelineViewport({ timebase, transport: transportStatus, displayTickRef: playhead });
  const analyses = useWaveformAnalyses(arrangement.audioClips);
  const timelineTicks = useMemo(
    () => timelineLengthTicks(arrangement, timebase),
    [arrangement, timebase],
  );
  const timelineWidth = timelineTicks * pixelsPerTick;
  const gridBackground = useMemo(
    () => timelineGridBackground(timebase, pixelsPerTick),
    [timebase, pixelsPerTick],
  );
  const editor = useTimelineEditor({
    session,
    selection: workspace.selection,
    setSelection: workspace.setSelection,
    tool,
    snap,
    pixelsPerTick,
    displayTick,
    analyses,
    onSplitToolUsed: () => setTool('select'),
  });
  const drop = useTimelineDrop({
    commit: editor.commit,
    pixelsPerTick,
    snapTick: editor.snapTick,
  });
  const selectedTrackId = workspace.selection.kind === 'track' ? workspace.selection.trackId : null;
  const ruler = useTimelineRulerController({
    arrangement,
    commit: editor.commit,
    snapTick: editor.snapTick,
    pixelsPerTick,
    displayTickRef: playhead,
    selectedClipCount:
      workspace.selection.kind === 'clips' ? workspace.selection.clipIds.length : 0,
  });
  const { clearSelectedRange, clearTimeSelection, handleKeyboard: handleRulerKeyboard } = ruler;
  // Selecting Tracks or Clips anywhere, including the Inspector and the Mixer,
  // releases the Loop and Punch range selection the ruler holds.
  useEffect(() => {
    clearSelectedRange();
  }, [workspace.selection, clearSelectedRange]);
  useTimelineShortcuts({
    handleRulerKeyboard,
    handleEditorKeyboard: editor.handleKeyboard,
    timeSelection: ruler.timeSelection,
    arrangement,
    timebase,
    zoomToRange,
  });
  const trackOperations = useTimelineTrackOperations({
    snapTick: editor.snapTick,
    timeSelection: ruler.timeSelection,
    clearTimeSelection,
  });
  const menus = useTimelineContextMenus({
    arrangement,
    editor,
    ruler,
    lower: { openMidiEditor: (clip) => workspace.openMidiEditor(clip.id) },
    snap,
    timebase,
    displayTick,
    onOpenDevices: (trackId) => {
      workspace.setSelection({ kind: 'track', trackId });
      workspace.openDevices();
    },
    addTrack: trackOperations.addTrack,
    deleteTrack: trackOperations.deleteTrack,
    trackClipCounts: trackOperations.clipCounts,
    createEmptyMidiClip: trackOperations.createEmptyMidiClip,
  });
  const unavailableClipIds = runtime.projection.activeDiagnostics?.unavailableClipIds ?? [];
  const automationOpen =
    selectedTrackId !== null && Boolean(workspace.automation.trackTargets[selectedTrackId]);

  return (
    <>
      <TimelineToolbar
        tool={tool}
        snap={snap}
        zoom={zoom}
        rulerMode={rulerMode}
        onTool={setTool}
        onSnap={setSnap}
        onZoom={applyZoom}
        onRulerMode={setRulerMode}
        automationAvailable={selectedTrackId !== null}
        automationOpen={automationOpen}
        onToggleAutomation={() => {
          if (selectedTrackId) workspace.toggleTrackAutomation(selectedTrackId);
        }}
        playSurfaceOpen={workspace.lower.view === 'playSurface'}
        onTogglePlaySurface={workspace.togglePlaySurface}
        mixerOpen={workspace.lower.view === 'mixer'}
        onToggleMixer={workspace.toggleMixer}
        computerKeyboardOn={liveInput.on}
        computerKeyboardRange={liveInput.rangeLabel}
        computerKeyboardBlockedReason={liveInput.blockedReason}
        onToggleComputerKeyboard={liveInput.onToggle}
      />

      <TimelineOverlays
        ruler={ruler}
        contextMenu={menus.contextMenu}
        onCloseContextMenu={menus.closeContextMenu}
        confirmRequest={trackOperations.confirmRequest}
        onDismissConfirm={trackOperations.dismissConfirm}
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
          <TimelineRuler
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
            style={{ width: timelineWidth, backgroundImage: gridBackground } as CSSProperties}
          />
          {transportStatus && transportStatus.recordingPhase !== 'idle' && (
            <div
              className={styles.recordingPreview}
              style={{
                left: TRACK_HEADER_WIDTH + transportStatus.recordingStartTick * pixelsPerTick,
                width:
                  Math.max(1, displayTick - transportStatus.recordingStartTick) * pixelsPerTick,
              }}
            >
              {transportStatus.recordingPhase.toUpperCase()} ·{' '}
              {transportStatus.armedTrackIds
                .map(
                  (trackId) =>
                    arrangement.tracks.find((track) => track.id === trackId)?.name ?? trackId,
                )
                .join(' · ')}{' '}
              · PASS {transportStatus.recordingPassOrdinal}
            </div>
          )}
          <TimelinePlayhead pixelsPerTick={pixelsPerTick} />
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

          <TimelineTrackList
            editor={editor}
            menus={menus}
            trackOperations={trackOperations}
            drop={drop}
            clearSelectedRange={clearSelectedRange}
            analyses={analyses}
            pixelsPerTick={pixelsPerTick}
            timelineWidth={timelineWidth}
            unavailableClipIds={unavailableClipIds}
            previewScope={previewScope}
            previewAutomation={previewAutomation}
            trackSizes={trackSizes}
            onSetTrackSize={(trackId, size) =>
              setTrackSizes((value) => ({ ...value, [trackId]: size }))
            }
          />
        </div>
      </div>
    </>
  );
}
