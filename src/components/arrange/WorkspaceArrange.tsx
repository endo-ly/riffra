import { useMemo, useRef, useState } from 'react';
import type { CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { ArrangeRuler } from './ArrangeRuler';
import { ArrangeToolbar } from './ArrangeToolbar';
import { ArrangeTrack } from './ArrangeTrack';
import {
  BASE_PIXELS_PER_QUARTER,
  clipDurationTicks,
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
  const scrollerRef = useRef<HTMLDivElement>(null);
  const { transport, displayTick, seekLocally } = useArrangeTransport(props.api, timebase);
  const analyses = useWaveformAnalyses(props.api, arrangement.audioClips);
  const pixelsPerTick = (BASE_PIXELS_PER_QUARTER * zoom) / timebase.ppq;
  const barTicks = ticksPerBar(timebase);
  const timelineTicks = useMemo(() => {
    const contentEnd = arrangement.audioClips.reduce(
      (end, clip) => Math.max(end, clip.startTick + clipDurationTicks(clip, timebase)),
      0,
    );
    return Math.max(barTicks * 16, contentEnd + barTicks * 2);
  }, [arrangement.audioClips, barTicks, timebase]);
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
      scroller.scrollLeft = Math.max(0, TRACK_HEADER_WIDTH + tick * nextPixels - cursor);
    });
  };

  const seekFromRuler = (event: React.PointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const tick = editor.snapTick((event.clientX - bounds.left) / pixelsPerTick, event.altKey);
    seekLocally(tick);
    void props.api.seekTimeline(tick).catch((error) => editor.setMessage(String(error)));
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
        position={formatMusicalPosition(displayTick, timebase)}
        clock={formatClock(displayTick, timebase)}
        bpm={timebase.bpm}
        signature={`${timebase.timeSignatureNumerator}/${timebase.timeSignatureDenominator}`}
        onTool={setTool}
        onSnap={setSnap}
        onZoom={applyZoom}
        onTrackSize={setTrackSize}
        onRulerMode={setRulerMode}
        onAddTrack={() =>
          void editor.commit(
            props.api.addTrack(`Audio ${arrangement.tracks.length + 1}`),
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
            loopRange={arrangement.loopRange}
            onPointerDown={seekFromRuler}
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
              <button
                onClick={() =>
                  void editor.commit(props.api.addTrack('Audio 1'), 'Audio Track added.')
                }
              >
                ＋ Add Audio Track
              </button>
            </div>
          ) : (
            arrangement.tracks.map((track, trackIndex) => (
              <ArrangeTrack
                key={track.id}
                track={track}
                clips={arrangement.audioClips.filter((clip) => clip.trackId === track.id)}
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
    </section>
  );
}
