import { useEffect, useMemo, useRef, useState } from 'react';
import type { CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { ArrangeRuler } from './ArrangeRuler';
import { ArrangeToolbar } from './ArrangeToolbar';
import { ArrangeTrack } from './ArrangeTrack';
import { MidiEditorPanel } from './MidiEditorPanel';
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
import { useArrangeEditor } from '@/hooks/arrange/useArrangeEditor';
import { useArrangeTransport } from '@/hooks/arrange/useArrangeTransport';
import { useWaveformAnalyses } from '@/hooks/arrange/useWaveformAnalyses';
import styles from './WorkspaceArrange.module.css';

interface WorkspaceArrangeProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: NativeApi;
  onRecord?: () => void;
}

export function WorkspaceArrange(props: WorkspaceArrangeProps) {
  const { arrangement } = props.session;
  const { timebase } = arrangement;
  const [tool, setTool] = useState<ArrangeTool>('select');
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [zoom, setZoom] = useState(1);
  const [trackSize, setTrackSize] = useState<TrackSize>('normal');
  const [trackSizes, setTrackSizes] = useState<Record<string, TrackSize>>({});
  const [rulerMode, setRulerMode] = useState<'bars' | 'time'>('bars');
  const [follow, setFollow] = useState(true);
  const [selectedMarkerId, setSelectedMarkerId] = useState<string | null>(null);
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
      ...arrangement.markers.map((marker) => marker.tick),
      arrangement.loopRange.startTick,
      arrangement.loopRange.endTick,
      0,
    );
    return Math.max(barTicks * 16, contentEnd + barTicks * 2);
  }, [
    arrangement.audioClips,
    arrangement.loopRange,
    arrangement.markers,
    arrangement.midiClips,
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

  const seekFromRuler = (event: React.PointerEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest('[data-marker-id], [data-loop-handle]')) return;
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

  const deleteTrack = async (trackId: string, name: string, clipCount: number) => {
    const detail = clipCount
      ? ` This also removes ${clipCount} Clip${clipCount === 1 ? '' : 's'} from the Timeline.`
      : '';
    if (!window.confirm(`Delete ${name}?${detail}\n\nSource Audio Assets will be kept.`)) return;
    const next = await editor.commit(props.api.removeTrack(trackId), `${name} deleted.`);
    if (next) {
      const remaining = new Set(next.arrangement.audioClips.map((clip) => clip.id));
      props.setSelectedClipIds(props.selectedClipIds.filter((id) => remaining.has(id)));
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
            markers={arrangement.markers}
            selectedMarkerId={selectedMarkerId}
            timeSelection={timeSelection}
            onPointerDown={seekFromRuler}
            onLoopHandle={dragLoopHandle}
            onAddMarker={(tick) =>
              void editor.commit(
                props.api.addMarker(
                  editor.snapTick(tick),
                  `Marker ${arrangement.markers.length + 1}`,
                ),
                'Marker added.',
              )
            }
            onMoveMarker={(marker, tick) =>
              void editor.commit(
                props.api.updateMarker(marker.id, { tick: editor.snapTick(tick) }),
                `${marker.name} moved.`,
              )
            }
            onRenameMarker={(marker) => {
              const next = window.prompt('Marker name', marker.name)?.trim();
              if (next && next !== marker.name)
                void editor.commit(
                  props.api.updateMarker(marker.id, { name: next }),
                  'Marker renamed.',
                );
            }}
            onRemoveMarker={(marker) => {
              if (!window.confirm(`Delete marker "${marker.name}"?`)) return;
              void editor.commit(props.api.removeMarker(marker.id), 'Marker removed.').then(() => {
                if (selectedMarkerId === marker.id) setSelectedMarkerId(null);
              });
            }}
            onSelectMarker={setSelectedMarkerId}
          />
          <div
            className={styles.playhead}
            style={{ left: TRACK_HEADER_WIDTH + displayTick * pixelsPerTick }}
          >
            <span />
          </div>
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
              <ArrangeTrack
                key={track.id}
                track={track}
                clips={arrangement.audioClips.filter((clip) => clip.trackId === track.id)}
                midiClips={arrangement.midiClips.filter((clip) => clip.trackId === track.id)}
                session={props.session}
                analyses={analyses}
                selectedClipIds={props.selectedClipIds}
                timelineWidth={timelineWidth}
                timelineTicks={timelineTicks}
                pixelsPerTick={pixelsPerTick}
                trackSize={trackSizes[track.id] ?? trackSize}
                api={props.api}
                onCommit={editor.commit}
                onDrop={(event, trackId) => void editor.dropAsset(event, trackId)}
                onMove={editor.beginMove}
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
              />
            ))
          )}
        </div>
      </div>

      <div className={styles.statusToast} role="status">
        <span className={transport?.state === 'playing' ? styles.playingDot : ''} />
        {editor.message}
        <small>REV {arrangement.revision}</small>
      </div>

      {timeSelection && (
        <div className={styles.selectionActions}>
          <span>
            Selection · {formatMusicalPosition(timeSelection.startTick, timebase)} →{' '}
            {formatMusicalPosition(timeSelection.endTick, timebase)}
          </span>
          <button onClick={setLoopToSelection}>Set Loop to Selection</button>
          <button onClick={() => setTimeSelection(null)}>Dismiss</button>
        </div>
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
          onRemoveNote={(clipId, noteId) =>
            void editor.commit(props.api.removeMidiNote(clipId, noteId), 'Note removed.')
          }
        />
      )}
    </section>
  );
}
