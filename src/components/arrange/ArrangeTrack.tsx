import type { CSSProperties } from 'react';
import type { AudioAnalysis, AudioClip, CreativeSession, Track } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { AudioClipView } from './AudioClipView';
import {
  layoutClipLanes,
  ticksPerBar,
  trackLaneHeight,
  type TrackSize,
} from '@/lib/arrange-timeline';
import styles from './WorkspaceArrange.module.css';

interface ArrangeTrackProps {
  track: Track;
  clips: AudioClip[];
  session: CreativeSession;
  analyses: Record<string, AudioAnalysis | null>;
  selectedClipIds: string[];
  timelineWidth: number;
  timelineTicks: number;
  pixelsPerTick: number;
  trackSize: TrackSize;
  api: NativeApi;
  onCommit: (
    operation: Promise<CreativeSession | null>,
    success: string,
  ) => Promise<CreativeSession | null>;
  onDrop: (event: React.DragEvent, trackId: string) => void;
  onMove: (event: React.PointerEvent<HTMLButtonElement>, clip: AudioClip) => void;
  onSelect: (clipId: string) => void;
  onTrim: (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: AudioClip,
    side: 'left' | 'right',
  ) => void;
  onFade: (event: React.PointerEvent<HTMLSpanElement>, clip: AudioClip, side: 'in' | 'out') => void;
  onRename: (name: string) => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onReorder: (sourceTrackId: string) => void;
  onResize: () => void;
}

export function ArrangeTrack(props: ArrangeTrackProps) {
  const layout = layoutClipLanes(props.clips, props.session.arrangement.timebase);
  const laneHeight = trackLaneHeight(props.trackSize);
  const barTicks = ticksPerBar(props.session.arrangement.timebase);
  const bars = Array.from(
    { length: Math.ceil(props.timelineTicks / barTicks) },
    (_, index) => index,
  );
  return (
    <div
      className={styles.trackRow}
      style={{ '--track-height': `${layout.count * laneHeight}px` } as CSSProperties}
      data-arrange-track
      data-track-id={props.track.id}
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => props.onDrop(event, props.track.id)}
    >
      <aside
        className={styles.trackHeader}
        onDragOver={(event) => {
          if (event.dataTransfer.types.includes('application/x-riffra-track')) {
            event.preventDefault();
          }
        }}
        onDrop={(event) => {
          const sourceTrackId = event.dataTransfer.getData('application/x-riffra-track');
          if (!sourceTrackId) return;
          event.preventDefault();
          event.stopPropagation();
          props.onReorder(sourceTrackId);
        }}
      >
        <span className={styles.trackColor} />
        <div className={styles.trackIdentity}>
          <div className={styles.trackNameRow}>
            <span
              className={styles.trackGrip}
              draggable
              title="Reorder track"
              onDragStart={(event) => {
                event.dataTransfer.effectAllowed = 'move';
                event.dataTransfer.setData('application/x-riffra-track', props.track.id);
              }}
            >
              ⠿
            </span>
            <strong>{props.track.name}</strong>
          </div>
          <small>
            AUDIO · {props.clips.length} CLIP{props.clips.length === 1 ? '' : 'S'}
          </small>
        </div>
        <div className={styles.trackSwitches}>
          <button
            className={props.track.muted ? styles.active : ''}
            aria-label={`Mute ${props.track.name}`}
            onClick={() =>
              void props.onCommit(
                props.api.updateTrack(props.track.id, { muted: !props.track.muted }),
                `${props.track.name} mute updated.`,
              )
            }
          >
            M
          </button>
          <button
            className={props.track.solo ? styles.active : ''}
            aria-label={`Solo ${props.track.name}`}
            onClick={() =>
              void props.onCommit(
                props.api.updateTrack(props.track.id, { solo: !props.track.solo }),
                `${props.track.name} solo updated.`,
              )
            }
          >
            S
          </button>
        </div>
        <details className={styles.trackMenu}>
          <summary aria-label={`${props.track.name} track menu`}>•••</summary>
          <div>
            <button
              onClick={(event) => {
                const name = window.prompt('Track name', props.track.name)?.trim();
                if (name && name !== props.track.name) props.onRename(name);
                event.currentTarget.closest('details')?.removeAttribute('open');
              }}
            >
              Rename
            </button>
            <button
              onClick={(event) => {
                props.onDuplicate();
                event.currentTarget.closest('details')?.removeAttribute('open');
              }}
            >
              Duplicate
            </button>
            <button
              onClick={(event) => {
                props.onResize();
                event.currentTarget.closest('details')?.removeAttribute('open');
              }}
            >
              Height: {props.trackSize}
            </button>
            <button className={styles.deleteTrack} onClick={props.onDelete}>
              Delete
            </button>
          </div>
        </details>
        <label className={styles.trackControl}>
          <span>VOL</span>
          <input
            key={`${props.track.id}:gain:${props.track.gainDb}`}
            aria-label={`${props.track.name} gain`}
            type="range"
            min="-60"
            max="12"
            step="0.5"
            defaultValue={props.track.gainDb}
            onPointerUp={(event) =>
              void props.onCommit(
                props.api.updateTrack(props.track.id, {
                  gainDb: Number(event.currentTarget.value),
                }),
                `${props.track.name} gain updated.`,
              )
            }
          />
          <output>{props.track.gainDb.toFixed(1)}</output>
        </label>
        <label className={styles.trackControl}>
          <span>PAN</span>
          <input
            key={`${props.track.id}:pan:${props.track.pan}`}
            aria-label={`${props.track.name} pan`}
            type="range"
            min="-1"
            max="1"
            step="0.05"
            defaultValue={props.track.pan}
            onPointerUp={(event) =>
              void props.onCommit(
                props.api.updateTrack(props.track.id, { pan: Number(event.currentTarget.value) }),
                `${props.track.name} pan updated.`,
              )
            }
          />
          <output>
            {Math.abs(props.track.pan) < 0.01
              ? 'C'
              : `${props.track.pan < 0 ? 'L' : 'R'}${Math.round(Math.abs(props.track.pan) * 100)}`}
          </output>
        </label>
      </aside>
      <div className={styles.lane} style={{ width: props.timelineWidth }}>
        {bars.map((bar) => (
          <i key={bar} style={{ left: bar * barTicks * props.pixelsPerTick }} />
        ))}
        {props.clips.map((clip) => (
          <AudioClipView
            key={clip.id}
            clip={clip}
            analysis={props.analyses[clip.assetId]}
            timebase={props.session.arrangement.timebase}
            pixelsPerTick={props.pixelsPerTick}
            lane={layout.lanes.get(clip.id) ?? 0}
            laneHeight={laneHeight}
            selected={props.selectedClipIds.includes(clip.id)}
            onSelect={props.onSelect}
            onMove={props.onMove}
            onTrim={props.onTrim}
            onFade={props.onFade}
          />
        ))}
      </div>
    </div>
  );
}
