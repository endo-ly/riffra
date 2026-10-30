import { Fragment, useState } from 'react';
import type { AudioAnalysis, AutomationTarget } from '@/model/generated';
import { useSession, useSessionContext } from '@/shared/session/session-context';
import { AutomationLaneEditor } from './AutomationLaneEditor';
import { TimelineTrack } from './TimelineTrack';
import { buildTrackTimeline, type TrackSize } from './model/timeline-layout';
import { isBrowserItemDrag, type useTimelineDrop } from './hooks/useTimelineDrop';
import type { useTimelineEditor } from './hooks/useTimelineEditor';
import type { useTimelineContextMenus } from './hooks/useTimelineContextMenus';
import type { useTimelineTrackOperations } from './hooks/useTimelineTrackOperations';
import styles from './TimelineView.module.css';

interface TimelineTrackListProps {
  editor: ReturnType<typeof useTimelineEditor>;
  menus: ReturnType<typeof useTimelineContextMenus>;
  trackOperations: ReturnType<typeof useTimelineTrackOperations>;
  drop: ReturnType<typeof useTimelineDrop>;
  clearSelectedRange(): void;
  analyses: Record<string, AudioAnalysis | null>;
  pixelsPerTick: number;
  timelineWidth: number;
  unavailableClipIds: string[];
  previewScope: string;
  previewAutomation(target: AutomationTarget, value: number | null, token: number): Promise<void>;
  trackSizes: Record<string, TrackSize>;
  onSetTrackSize(trackId: string, size: TrackSize): void;
}

/** Track rows, automation lanes, and the two drop regions of the arrangement. */
export function TimelineTrackList(props: TimelineTrackListProps) {
  const { editor, trackOperations, drop } = props;
  const { api } = useSessionContext();
  const session = useSession((state) => state.project.boot!.canonical.session);
  const arrangement = session.arrangement;
  const selection = useSession((state) => state.workspace.selection);
  const focusedTrackId = useSession((state) => state.workspace.focusedTrackId);
  const automation = useSession((state) => state.workspace.automation);
  const lowerView = useSession((state) => state.workspace.lower.view);
  const setSelection = useSession((state) => state.workspace.setSelection);
  const focusTrack = useSession((state) => state.workspace.focusTrack);
  const openMidiEditor = useSession((state) => state.workspace.openMidiEditor);
  const keepMidiClipVisible = useSession((state) => state.workspace.keepMidiClipVisible);
  const openDevices = useSession((state) => state.workspace.openDevices);
  const openAutomation = useSession((state) => state.workspace.openAutomation);
  const closeTrackAutomation = useSession((state) => state.workspace.closeTrackAutomation);
  const closeMixerAutomation = useSession((state) => state.workspace.closeMixerAutomation);
  const automationWarnings = useSession(
    (state) => state.audio.status.diagnostics.automationWarnings,
  );
  const displayTick = useSession((state) => state.transport.displayTick);
  const playhead = useSession((state) => state.transport.playhead);
  const [emptyDragOver, setEmptyDragOver] = useState(false);
  const [newTrackDragOver, setNewTrackDragOver] = useState(false);
  const { sessionId } = session;
  const { timebase } = arrangement;
  const { pixelsPerTick, timelineWidth, analyses, unavailableClipIds } = props;
  const selectedClipIds = selection.kind === 'clips' ? selection.clipIds : [];
  const soloActive = arrangement.tracks.some((track) => track.solo);

  const acceptsDrop = (event: React.DragEvent<HTMLDivElement>) =>
    isBrowserItemDrag(event) || drop.isOsFileDrag(event);

  const automationLane = (target: AutomationTarget, onClose: () => void) => (
    <AutomationLaneEditor
      warnings={automationWarnings}
      session={session}
      target={target}
      timelineWidth={timelineWidth}
      pixelsPerTick={pixelsPerTick}
      playheadTick={displayTick}
      playheadTickRef={playhead}
      previewScope={props.previewScope}
      snapTick={editor.snapTick}
      onPreview={props.previewAutomation}
      onTarget={openAutomation}
      onClose={onClose}
      onCommit={(target_, interpolation, points) =>
        editor.commit(api.setAutomation(target_, interpolation, points))
      }
    />
  );

  return (
    <>
      {arrangement.tracks.length === 0 ? (
        <div
          className={`${styles.empty} ${emptyDragOver ? styles.emptyDragOver : ''}`}
          onDragOver={(event) => {
            if (!acceptsDrop(event)) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
          }}
          onDragEnter={() => setEmptyDragOver(true)}
          onDragLeave={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node)) setEmptyDragOver(false);
          }}
          onDrop={(event) => {
            setEmptyDragOver(false);
            drop.handleDrop(event);
          }}
        >
          <span className={styles.emptyIcon}>≋</span>
          <strong>Start arranging</strong>
          <div className={styles.emptyActions}>
            <button onClick={() => void trackOperations.addTrack('audio')}>
              ＋ Add Audio Track
            </button>
            <button onClick={() => void trackOperations.addTrack('instrument')}>
              ＋ Add Instrument Track
            </button>
          </div>
        </div>
      ) : (
        arrangement.tracks.map((track, trackIndex) => (
          <Fragment key={track.id}>
            <TimelineTrack
              sessionId={sessionId}
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
              selected={selection.kind === 'track' && selection.trackId === track.id}
              focused={focusedTrackId === track.id}
              onSelectTrack={() => {
                props.clearSelectedRange();
                setSelection({ kind: 'track', trackId: track.id });
                focusTrack(track.kind === 'instrument' ? track.id : null);
              }}
              timelineWidth={timelineWidth}
              pixelsPerTick={pixelsPerTick}
              trackSize={props.trackSizes[track.id] ?? 'normal'}
              onCommit={editor.commit}
              onDrop={(event, trackId, trackKind) => {
                drop.handleDrop(event, trackId, trackKind);
              }}
              onContextMenu={props.menus.openTrackAreaContextMenu}
              onDoubleClickLane={
                track.kind === 'instrument'
                  ? (event, trackId, tick) => {
                      event.preventDefault();
                      void trackOperations.createEmptyMidiClip(trackId, tick);
                    }
                  : undefined
              }
              onMove={editor.beginMove}
              onMoveMidi={editor.beginMidiMove}
              onTrimMidi={editor.beginMidiTrim}
              onSelect={(clipId, append = false) => {
                props.clearSelectedRange();
                editor.selectClip(clipId, append);
                const selectedMidiClip = arrangement.midiClips.find((clip) => clip.id === clipId);
                if (selectedMidiClip && !append && lowerView === 'midiEditor') {
                  keepMidiClipVisible(clipId);
                }
              }}
              onTrim={editor.beginTrim}
              onFade={editor.beginFade}
              onOpenMidiEditor={(clip) => openMidiEditor(clip.id)}
              onAudioClipContextMenu={props.menus.openAudioClipContextMenu}
              onMidiClipContextMenu={props.menus.openMidiClipContextMenu}
              onRename={(name) => void editor.commit(api.updateTrack(track.id, { name }))}
              onDuplicate={() => void editor.commit(api.duplicateTrack(track.id))}
              onDelete={() =>
                trackOperations.deleteTrack(
                  track.id,
                  track.name,
                  trackOperations.clipCounts.get(track.id) ?? 0,
                )
              }
              onOpenDevices={() => {
                setSelection({ kind: 'track', trackId: track.id });
                openDevices();
              }}
              onReorder={(sourceTrackId, insertAfter) =>
                trackOperations.reorderTrack(sourceTrackId, trackIndex, insertAfter)
              }
              onSetTrackSize={(size) => props.onSetTrackSize(track.id, size)}
            />
            {automation.trackTargets[track.id] &&
              automationLane(automation.trackTargets[track.id]!, () =>
                closeTrackAutomation(track.id),
              )}
          </Fragment>
        ))
      )}
      {automation.mixerTarget && automationLane(automation.mixerTarget, closeMixerAutomation)}
      {arrangement.tracks.length > 0 && (
        <div
          className={`${styles.newTrackDrop} ${newTrackDragOver ? styles.newTrackDropActive : ''}`}
          onDragOver={(event) => {
            if (!acceptsDrop(event)) return;
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
            drop.handleDrop(event);
          }}
        >
          {newTrackDragOver && <span>Drop to add to the Arrangement</span>}
        </div>
      )}
    </>
  );
}
