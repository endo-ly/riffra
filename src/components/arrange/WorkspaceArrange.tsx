import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import type { AutomationParameter, CreativeSession, Marker } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { ArrangeRuler } from './ArrangeRuler';
import { ArrangeToolbar } from './ArrangeToolbar';
import { ArrangeTrack } from './ArrangeTrack';
import { AutomationLaneView } from './AutomationLaneView';
import { MidiEditorPanel } from './MidiEditorPanel';
import { ContextMenu, type ContextMenuItem } from '../shared/ContextMenu';
import {
  BASE_PIXELS_PER_QUARTER,
  timelineObjectEndTick,
  formatClock,
  formatMusicalPosition,
  ticksPerBar,
  TRACK_HEADER_WIDTH,
  type ArrangeTool,
  type SnapGrid,
  type TrackSize,
} from '@/lib/arrange-timeline';
import { useArrangeEditor, type ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';
import { useArrangeTransport } from '@/hooks/arrange/useArrangeTransport';
import { useWaveformAnalyses } from '@/hooks/arrange/useWaveformAnalyses';
import styles from './WorkspaceArrange.module.css';

interface WorkspaceArrangeProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selection: ArrangeSelection;
  setSelection: (selection: ArrangeSelection) => void;
  api: NativeApi;
  onRecord?: () => void;
  recordingActive?: boolean;
}

export function WorkspaceArrange(props: WorkspaceArrangeProps) {
  const { arrangement } = props.session;
  const { timebase } = arrangement;
  const { api, setSession } = props;
  const [tool, setTool] = useState<ArrangeTool>('select');
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [zoom, setZoom] = useState(1);
  const [trackSize, setTrackSize] = useState<TrackSize>('normal');
  const [trackSizes, setTrackSizes] = useState<Record<string, TrackSize>>({});
  const [automationParameters, setAutomationParameters] = useState<
    Partial<Record<string, AutomationParameter>>
  >({});
  const [rulerMode, setRulerMode] = useState<'bars' | 'time'>('bars');
  const [follow, setFollow] = useState(true);
  const [selectedMarkerId, setSelectedMarkerId] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    items: ContextMenuItem[];
  } | null>(null);
  const [timeSelection, setTimeSelection] = useState<{ startTick: number; endTick: number } | null>(
    null,
  );
  const [loopPreview, setLoopPreview] = useState<{
    enabled: boolean;
    startTick: number;
    endTick: number;
  } | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [activeMidiClipId, setActiveMidiClipId] = useState<string | null>(null);
  const [midiEditorOpen, setMidiEditorOpen] = useState(false);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const programmaticScrollRef = useRef(false);
  const { transport, displayTick, seekLocally } = useArrangeTransport(props.api, timebase);
  const analyses = useWaveformAnalyses(props.api, arrangement.audioClips);
  const pixelsPerTick = (BASE_PIXELS_PER_QUARTER * zoom) / timebase.ppq;
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
  const editor = useArrangeEditor({
    ...props,
    tool,
    snap,
    pixelsPerTick,
    displayTick,
    analyses,
  });
  const playbackOutOfSync =
    editor.runtimeOutOfSync || Boolean(transport && transport.revision !== arrangement.revision);
  const unavailableClipCount = transport?.unavailableClipIds?.length ?? 0;
  const missingDeviceCount = transport?.missingDeviceIds?.length ?? 0;
  const selectedClipIds = props.selection.kind === 'clips' ? props.selection.clipIds : [];
  const selectedTrackId = props.selection.kind === 'track' ? props.selection.trackId : null;

  const applyZoom = (next: number, clientX?: number) => {
    const bounded = Math.min(4, Math.max(0.35, next));
    const scroller = scrollerRef.current;
    if (!scroller) return setZoom(bounded);
    const bounds = scroller.getBoundingClientRect();
    const cursor = (clientX ?? bounds.left + bounds.width / 2) - bounds.left;
    const tick = Math.max(0, (scroller.scrollLeft + cursor - TRACK_HEADER_WIDTH) / pixelsPerTick);
    setZoom(bounded);
    requestAnimationFrame(() => {
      const nextPixels = (BASE_PIXELS_PER_QUARTER * bounded) / timebase.ppq;
      programmaticScrollRef.current = true;
      scroller.scrollLeft = Math.max(0, TRACK_HEADER_WIDTH + tick * nextPixels - cursor);
    });
  };

  // Track vertical scroll so the ruler and ruler corner stay sticky to the top
  // of the scrolling viewport without leaving the timeline's horizontal flow.
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let frame = 0;
    const onScroll = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setScrollTop(scroller.scrollTop));
    };
    scroller.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      scroller.removeEventListener('scroll', onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);

  // Follow Playhead: during playback, keep the playhead in view. Manual scroll
  // pauses follow until the user seeks via the ruler or re-enables the toggle.
  useEffect(() => {
    if (!follow || transport?.state !== 'playing') return;
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const playheadX = TRACK_HEADER_WIDTH + displayTick * pixelsPerTick;
    const left = scroller.scrollLeft;
    const right = left + scroller.clientWidth;
    const margin = Math.min(160, scroller.clientWidth * 0.18);
    if (playheadX < left + margin || playheadX > right - margin) {
      const target = Math.max(0, playheadX - scroller.clientWidth * 0.32);
      programmaticScrollRef.current = true;
      scroller.scrollLeft = target;
    }
  }, [displayTick, follow, pixelsPerTick, transport?.state]);

  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let frame = 0;
    const onScroll = () => {
      if (programmaticScrollRef.current) {
        programmaticScrollRef.current = false;
        return;
      }
      if (frame) {
        cancelAnimationFrame(frame);
        frame = 0;
      }
      frame = requestAnimationFrame(() => {
        if (transport?.state === 'playing') setFollow(false);
      });
    };
    scroller.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      scroller.removeEventListener('scroll', onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [transport?.state]);

  // Keyboard: Delete removes the selected Marker (when no Clips are selected);
  // Escape clears the Time Selection and Marker selection (deselect, not delete).
  useEffect(() => {
    const isEditable = (target: EventTarget | null) => {
      const el = target as HTMLElement | null;
      return (
        !!el &&
        (el.tagName === 'INPUT' ||
          el.tagName === 'TEXTAREA' ||
          el.tagName === 'SELECT' ||
          el.isContentEditable)
      );
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === 'Escape') {
        if (isEditable(event.target)) return;
        setTimeSelection(null);
        setSelectedMarkerId(null);
        return;
      }
      if (event.key === 'Delete' && selectedMarkerId && selectedClipIds.length === 0) {
        if (isEditable(event.target)) return;
        const marker = arrangement.markers.find((item) => item.id === selectedMarkerId);
        if (!marker) return;
        event.preventDefault();
        void api.removeMarker(marker.id).then((next) => {
          setSession(next);
          setSelectedMarkerId(null);
        });
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [selectedMarkerId, selectedClipIds.length, arrangement.markers, api, setSession]);

  const seekFromRuler = (event: React.PointerEvent<HTMLDivElement>) => {
    if (
      (event.target as HTMLElement).closest(
        '[data-marker-id], [data-loop-handle], [data-range-close]',
      )
    )
      return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const originTick = editor.snapTick((event.clientX - bounds.left) / pixelsPerTick, event.altKey);
    const originX = event.clientX;
    let seeking = true;
    seekLocally(originTick);
    void props.api.seekTimeline(originTick).catch((error) => editor.setMessage(String(error)));
    setFollow(true);
    const handle = (move: PointerEvent) => {
      const tick = editor.snapTick((move.clientX - bounds.left) / pixelsPerTick, move.altKey);
      if (seeking && Math.abs(move.clientX - originX) > 4) {
        seeking = false;
        setTimeSelection({
          startTick: Math.min(originTick, tick),
          endTick: Math.max(originTick, tick),
        });
        return;
      }
      if (seeking) {
        seekLocally(tick);
        void props.api.seekTimeline(tick).catch((error) => editor.setMessage(String(error)));
      } else {
        setTimeSelection((current) =>
          current
            ? { startTick: Math.min(originTick, tick), endTick: Math.max(originTick, tick) }
            : null,
        );
      }
    };
    const finish = () => {
      window.removeEventListener('pointermove', handle);
      window.removeEventListener('pointerup', finish);
      if (seeking) setTimeSelection(null);
    };
    window.addEventListener('pointermove', handle);
    window.addEventListener('pointerup', finish);
  };

  const dragLoopHandle = (
    event: React.PointerEvent<HTMLSpanElement>,
    boundary: 'start' | 'end',
  ) => {
    event.stopPropagation();
    const handle = event.currentTarget;
    const originX = event.clientX;
    const range = arrangement.loopRange;
    const origin = boundary === 'start' ? range.startTick : range.endTick;
    handle.setPointerCapture?.(event.pointerId);
    const move = (pointer: PointerEvent) => {
      const next = editor.snapTick(
        origin + (pointer.clientX - originX) / pixelsPerTick,
        pointer.altKey,
      );
      setLoopPreview({
        enabled: range.enabled,
        startTick: boundary === 'start' ? next : range.startTick,
        endTick: boundary === 'end' ? next : range.endTick,
      });
    };
    const finish = (pointer: PointerEvent) => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', finish);
      const next = editor.snapTick(
        origin + (pointer.clientX - originX) / pixelsPerTick,
        pointer.altKey,
      );
      setLoopPreview(null);
      if (next !== origin) {
        void editor.commit(
          props.api.updateTimelineLoopRange(
            range.enabled,
            boundary === 'start' ? next : range.startTick,
            boundary === 'end' ? next : range.endTick,
          ),
          'Loop range updated.',
        );
      }
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', finish);
  };

  const setLoopToSelection = () => {
    if (!timeSelection) return;
    void props.api
      .updateTimelineLoopRange(true, timeSelection.startTick, timeSelection.endTick)
      .then(props.setSession);
  };

  const setPunchToSelection = () => {
    if (!timeSelection) return;
    void props.api
      .updateTimelinePunchRange(true, timeSelection.startTick, timeSelection.endTick)
      .then(props.setSession);
  };

  const clearLoop = () => {
    void props.api
      .updateTimelineLoopRange(
        false,
        arrangement.loopRange.startTick,
        arrangement.loopRange.endTick,
      )
      .then(props.setSession);
  };

  const clearPunch = () => {
    void props.api
      .updateTimelinePunchRange(
        false,
        arrangement.punchRange?.startTick ?? 0,
        arrangement.punchRange?.endTick ?? 0,
      )
      .then(props.setSession);
  };

  const addMarkerAt = (tick: number) => {
    void editor.commit(
      props.api.addMarker(editor.snapTick(tick), `Marker ${arrangement.markers.length + 1}`),
      'Marker added.',
    );
  };

  const renameMarker = (marker: Marker) => {
    const next = window.prompt('Marker name', marker.name)?.trim();
    if (next && next !== marker.name)
      void editor.commit(props.api.updateMarker(marker.id, { name: next }), 'Marker renamed.');
  };

  const removeMarker = (marker: Marker) => {
    if (!window.confirm(`Delete marker "${marker.name}"?`)) return;
    void editor.commit(props.api.removeMarker(marker.id), 'Marker removed.').then(() => {
      if (selectedMarkerId === marker.id) setSelectedMarkerId(null);
    });
  };

  const openRulerContextMenu = (event: React.MouseEvent<HTMLDivElement>, tick: number) => {
    event.preventDefault();
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Add Marker Here', onClick: () => addMarkerAt(tick) },
        { label: 'Set Loop to Selection', onClick: setLoopToSelection, disabled: !timeSelection },
        { label: 'Set Punch Range', onClick: setPunchToSelection, disabled: !timeSelection },
        { separator: true },
        { label: 'Clear Loop', onClick: clearLoop, disabled: !arrangement.loopRange.enabled },
        { label: 'Clear Punch', onClick: clearPunch, disabled: !arrangement.punchRange },
      ],
    });
  };

  const openMarkerContextMenu = (event: React.MouseEvent, marker: Marker) => {
    event.preventDefault();
    setSelectedMarkerId(marker.id);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Rename', onClick: () => renameMarker(marker) },
        { label: 'Delete', danger: true, onClick: () => removeMarker(marker) },
      ],
    });
  };

  const deleteTrack = async (trackId: string, name: string, clipCount: number) => {
    const detail = clipCount
      ? ` This also removes ${clipCount} Clip${clipCount === 1 ? '' : 's'} from the Timeline.`
      : '';
    if (!window.confirm(`Delete ${name}?${detail}\n\nSource Audio Assets will be kept.`)) return;
    const next = await editor.commit(props.api.removeTrack(trackId), `${name} deleted.`);
    if (next) {
      const remaining = new Set(next.arrangement.audioClips.map((clip) => clip.id));
      const clipIds = selectedClipIds.filter((id) => remaining.has(id));
      props.setSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
    }
  };

  const cycleTrackSize = (trackId: string) => {
    const sizes: TrackSize[] = ['compact', 'normal', 'large'];
    const current = trackSizes[trackId] ?? trackSize;
    setTrackSizes((value) => ({
      ...value,
      [trackId]: sizes[(sizes.indexOf(current) + 1) % sizes.length],
    }));
  };
  const toggleAutomation = (trackId: string) =>
    setAutomationParameters((current) => ({
      ...current,
      [trackId]: current[trackId] ? undefined : 'volume',
    }));

  return (
    <section className={styles.workspace} aria-label="Arrange timeline">
      <ArrangeToolbar
        tool={tool}
        snap={snap}
        zoom={zoom}
        trackSize={trackSize}
        rulerMode={rulerMode}
        follow={follow}
        position={formatMusicalPosition(displayTick, timebase)}
        clock={formatClock(displayTick, timebase)}
        bpm={timebase.bpm}
        signature={`${timebase.timeSignatureNumerator}/${timebase.timeSignatureDenominator}`}
        onTool={setTool}
        onSnap={setSnap}
        onZoom={applyZoom}
        onTrackSize={setTrackSize}
        onRulerMode={setRulerMode}
        onFollow={setFollow}
        onTimebase={(bpm, numerator, denominator) =>
          void editor.commit(
            props.api.updateArrangementTimebase({
              ...timebase,
              bpm,
              timeSignatureNumerator: numerator,
              timeSignatureDenominator: denominator,
            }),
            'Project timebase updated.',
          )
        }
        onAddTrack={() =>
          void editor.commit(
            props.api.addTrack(`Audio ${arrangement.tracks.length + 1}`, 'audio'),
            'Audio Track added.',
          )
        }
        automationAvailable={selectedTrackId !== null}
        automationOpen={selectedTrackId !== null && Boolean(automationParameters[selectedTrackId])}
        onToggleAutomation={() => {
          if (selectedTrackId) toggleAutomation(selectedTrackId);
        }}
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
        >
          <ArrangeRuler
            timebase={timebase}
            timelineTicks={timelineTicks}
            timelineWidth={timelineWidth}
            pixelsPerTick={pixelsPerTick}
            mode={rulerMode}
            scrollTop={scrollTop}
            loopRange={loopPreview ?? arrangement.loopRange}
            punchRange={arrangement.punchRange}
            markers={arrangement.markers}
            selectedMarkerId={selectedMarkerId}
            timeSelection={timeSelection}
            onPointerDown={seekFromRuler}
            onLoopHandle={dragLoopHandle}
            onClearLoop={clearLoop}
            onClearPunch={clearPunch}
            onRulerContextMenu={openRulerContextMenu}
            onMarkerContextMenu={openMarkerContextMenu}
            onAddMarker={addMarkerAt}
            onMoveMarker={(marker, tick) =>
              void editor.commit(
                props.api.updateMarker(marker.id, { tick: editor.snapTick(tick) }),
                `${marker.name} moved.`,
              )
            }
            onRenameMarker={renameMarker}
            onRemoveMarker={removeMarker}
            onSelectMarker={setSelectedMarkerId}
          />
          {transport && transport.recordingPhase !== 'idle' && (
            <div
              className={styles.recordingPreview}
              style={{
                left: TRACK_HEADER_WIDTH + transport.recordingStartTick * pixelsPerTick,
                width:
                  Math.max(1, transport.recordingCurrentTick - transport.recordingStartTick) *
                  pixelsPerTick,
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
          <div
            className={styles.playhead}
            style={{ left: TRACK_HEADER_WIDTH + displayTick * pixelsPerTick }}
          >
            <span />
          </div>
          {timeSelection && (
            <div
              className={styles.selectionChip}
              style={{
                left:
                  TRACK_HEADER_WIDTH +
                  ((timeSelection.startTick + timeSelection.endTick) / 2) * pixelsPerTick,
              }}
            >
              <span>
                {formatMusicalPosition(timeSelection.startTick, timebase)} →{' '}
                {formatMusicalPosition(timeSelection.endTick, timebase)}
              </span>
              <button onClick={setLoopToSelection}>Set Loop</button>
              <button onClick={setPunchToSelection}>Set Punch</button>
              <button
                className={styles.selectionChipClose}
                aria-label="Clear time selection"
                title="Clear time selection (Esc)"
                onClick={() => setTimeSelection(null)}
              >
                ×
              </button>
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
              className={styles.empty}
              onDragOver={(event) => event.preventDefault()}
              onDrop={(event) => void editor.dropAsset(event)}
            >
              <span className={styles.emptyIcon}>≋</span>
              <strong>Start arranging</strong>
              <p>Drag audio or MIDI here, or start from an empty track.</p>
              <div className={styles.emptyActions}>
                <button
                  onClick={() =>
                    void editor.commit(props.api.addTrack('Audio 1', 'audio'), 'Audio Track added.')
                  }
                >
                  ＋ Add Audio Track
                </button>
                <button
                  onClick={() =>
                    void editor.commit(
                      props.api.addTrack('Instrument 1', 'instrument'),
                      'Instrument Track added.',
                    )
                  }
                >
                  ＋ Add Instrument Track
                </button>
                {props.onRecord && (
                  <button
                    className={styles.emptyRecord}
                    onClick={() => props.onRecord?.()}
                    title="Arm a Track or drop an Asset to start recording"
                  >
                    ● Record
                  </button>
                )}
              </div>
            </div>
          ) : (
            arrangement.tracks.map((track, trackIndex) => (
              <Fragment key={track.id}>
                <ArrangeTrack
                  track={track}
                  clips={arrangement.audioClips.filter((clip) => clip.trackId === track.id)}
                  midiClips={arrangement.midiClips.filter((clip) => clip.trackId === track.id)}
                  session={props.session}
                  analyses={analyses}
                  selectedClipIds={selectedClipIds}
                  unavailableClipIds={transport?.unavailableClipIds ?? []}
                  selected={
                    props.selection.kind === 'track' && props.selection.trackId === track.id
                  }
                  onSelectTrack={() => props.setSelection({ kind: 'track', trackId: track.id })}
                  timelineWidth={timelineWidth}
                  timelineTicks={timelineTicks}
                  pixelsPerTick={pixelsPerTick}
                  trackSize={trackSizes[track.id] ?? trackSize}
                  api={props.api}
                  onCommit={editor.commit}
                  onDrop={(event, trackId) => void editor.dropAsset(event, trackId)}
                  onMove={editor.beginMove}
                  onMoveMidi={editor.beginMidiMove}
                  onTrimMidi={editor.beginMidiTrim}
                  onSelect={editor.selectClip}
                  onTrim={editor.beginTrim}
                  onFade={editor.beginFade}
                  onOpenMidiEditor={(clip) => {
                    setActiveMidiClipId(clip.id);
                    setMidiEditorOpen(true);
                  }}
                  onRename={(name) =>
                    void editor.commit(
                      props.api.updateTrack(track.id, { name }),
                      `Track renamed to ${name}.`,
                    )
                  }
                  onDuplicate={() =>
                    void editor.commit(
                      props.api.duplicateTrack(track.id),
                      `${track.name} duplicated.`,
                    )
                  }
                  onDelete={() =>
                    void deleteTrack(
                      track.id,
                      track.name,
                      arrangement.audioClips.filter((clip) => clip.trackId === track.id).length,
                    )
                  }
                  onReorder={(sourceTrackId) =>
                    void editor.commit(
                      props.api.reorderTrack(sourceTrackId, trackIndex),
                      'Track order updated.',
                    )
                  }
                  onResize={() => cycleTrackSize(track.id)}
                  automationOpen={Boolean(automationParameters[track.id])}
                  onToggleAutomation={() => toggleAutomation(track.id)}
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
                        `${track.name} automation updated.`,
                      )
                    }
                  />
                )}
              </Fragment>
            ))
          )}
        </div>
      </div>

      <div className={styles.statusToast} role="status">
        <span className={transport?.state === 'playing' ? styles.playingDot : ''} />
        {playbackOutOfSync
          ? 'Playback runtime is out of sync'
          : unavailableClipCount || missingDeviceCount
            ? `Playback skipped ${unavailableClipCount} missing source${unavailableClipCount === 1 ? '' : 's'} and ${missingDeviceCount} missing device${missingDeviceCount === 1 ? '' : 's'}.`
            : editor.message}
        {playbackOutOfSync && <button onClick={() => void editor.retryRuntimeSync()}>Retry</button>}
        <small>REV {arrangement.revision}</small>
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenu.items}
          onClose={() => setContextMenu(null)}
        />
      )}

      {midiEditorOpen && (
        <MidiEditorPanel
          clip={arrangement.midiClips.find((clip) => clip.id === activeMidiClipId) ?? null}
          timebase={timebase}
          onClose={() => {
            setMidiEditorOpen(false);
            setActiveMidiClipId(null);
          }}
          onAddNote={(clipId, startTick, pitch) =>
            void editor.commit(
              props.api.addMidiNote(clipId, Math.max(0, Math.round(startTick)), pitch, 240, 96, 1),
              'Note added.',
            )
          }
          onUpdateNote={(clipId, note) =>
            void editor.commit(
              props.api.updateMidiNote(clipId, note.id, {
                note: note.note,
                startTick: note.startTick,
                durationTicks: note.durationTicks,
                velocity: note.velocity,
              }),
              'Note updated.',
            )
          }
          onUpdateNotes={(clipId, updates) =>
            void editor.commit(
              props.api.updateMidiNotes(
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
              'MIDI notes updated.',
            )
          }
          onRemoveNote={(clipId, noteId) =>
            void editor.commit(props.api.removeMidiNote(clipId, noteId), 'Note removed.')
          }
          onQuantize={(clipId, noteIds, gridTicks) =>
            void editor.commit(
              props.api.quantizeMidiNotes(clipId, noteIds, gridTicks),
              'MIDI notes quantized.',
            )
          }
          onDuplicateNotes={(clipId, noteIds, offsetTicks) =>
            void editor.commit(
              props.api.duplicateMidiNotes(clipId, noteIds, offsetTicks),
              'MIDI notes duplicated.',
            )
          }
        />
      )}
    </section>
  );
}
