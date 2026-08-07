import { useEffect, useRef, useState, type CSSProperties } from 'react';
import type {
  AudioAnalysis,
  AudioClip,
  CreativeSession,
  MidiClip,
  MonitoringState,
  ProjectTimebase,
  Track,
} from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { AudioClipView } from './AudioClipView';
import { MidiClipView } from './MidiClipView';
import {
  ticksPerBar,
  trackLaneHeight,
  type ArrangeAudioTimelineItem,
  type ArrangeMidiTimelineItem,
  type TrackTimeline,
  type TrackSize,
} from '@/lib/arrange-timeline';
import { RIFFRA_ASSET_MIME } from '@/lib/arrange-drag';
import styles from './WorkspaceArrange.module.css';

interface ArrangeTrackProps {
  track: Track;
  timeline: TrackTimeline;
  timebase: ProjectTimebase;
  analyses: Record<string, AudioAnalysis | null>;
  selectedClipIds: string[];
  selected: boolean;
  focused: boolean;
  unavailableClipIds: string[];
  timelineWidth: number;
  timelineTicks: number;
  pixelsPerTick: number;
  trackSize: TrackSize;
  api: NativeApi;
  onCommit: (
    operation: Promise<CreativeSession | null>,
    success: string,
  ) => Promise<CreativeSession | null>;
  onDrop: (event: React.DragEvent, trackId: string, trackKind: Track['kind']) => void;
  onContextMenu?: (event: React.MouseEvent, trackId: string, tick: number) => void;
  onMove: (event: React.PointerEvent<HTMLButtonElement>, clip: AudioClip) => void;
  onMoveMidi: (event: React.PointerEvent<HTMLButtonElement>, clip: MidiClip) => void;
  onTrimMidi: (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: MidiClip,
    side: 'left' | 'right',
  ) => void;
  onSelect: (clipId: string) => void;
  onSelectTrack: () => void;
  onOpenPlaySurface?: () => void;
  onTrim: (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: AudioClip,
    side: 'left' | 'right',
  ) => void;
  onFade: (event: React.PointerEvent<HTMLSpanElement>, clip: AudioClip, side: 'in' | 'out') => void;
  onOpenMidiEditor?: (clip: MidiClip) => void;
  onAudioClipContextMenu?: (event: React.MouseEvent, clip: AudioClip) => void;
  onMidiClipContextMenu?: (event: React.MouseEvent, clip: MidiClip) => void;
  onRename: (name: string) => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onReorder: (sourceTrackId: string) => void;
  onResize: () => void;
  onSetTrackSize?: (size: TrackSize) => void;
  automationOpen: boolean;
  onToggleAutomation: () => void;
}

interface PendingTrackValues {
  muted?: boolean;
  solo?: boolean;
  armed?: boolean;
  monitoring?: MonitoringState;
}

export function ArrangeTrack(props: ArrangeTrackProps) {
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const [dragOver, setDragOver] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [pendingTrackValues, setPendingTrackValues] = useState<PendingTrackValues>({});

  useEffect(() => {
    setPendingTrackValues((current) => {
      const next = { ...current };
      if (next.muted === props.track.muted) delete next.muted;
      if (next.solo === props.track.solo) delete next.solo;
      if (next.armed === props.track.armed) delete next.armed;
      if (next.monitoring === props.track.monitoring) delete next.monitoring;
      return next;
    });
  }, [props.track.armed, props.track.monitoring, props.track.muted, props.track.solo]);

  const commitTrackValue = (
    field: 'muted' | 'solo' | 'armed' | 'monitoring',
    value: boolean | MonitoringState,
    success: string,
  ) => {
    setPendingTrackValues((current) => ({ ...current, [field]: value }));
    const patch =
      field === 'muted'
        ? { muted: value as boolean }
        : field === 'solo'
          ? { solo: value as boolean }
          : field === 'armed'
            ? { armed: value as boolean }
            : { monitoring: value as MonitoringState };
    void props.onCommit(props.api.updateTrack(props.track.id, patch), success).then((result) => {
      if (result == null) {
        setPendingTrackValues((current) => {
          if (current[field] !== value) return current;
          const next = { ...current };
          delete next[field];
          return next;
        });
      }
    });
  };

  useEffect(() => {
    const onClick = (event: MouseEvent) => {
      const details = detailsRef.current;
      if (!details) return;
      if (details.open && !details.contains(event.target as Node)) {
        details.removeAttribute('open');
      }
    };
    document.addEventListener('click', onClick);
    return () => document.removeEventListener('click', onClick);
  }, []);

  const audioItems = props.timeline.items.filter(
    (item): item is ArrangeAudioTimelineItem => item.kind === 'audio',
  );
  const midiItems = props.timeline.items.filter(
    (item): item is ArrangeMidiTimelineItem => item.kind === 'midi',
  );
  const laneCount = props.timeline.laneCount;
  const laneHeight = trackLaneHeight(props.trackSize);
  const barTicks = ticksPerBar(props.timebase);
  const bars = Array.from(
    { length: Math.ceil(props.timelineTicks / barTicks) },
    (_, index) => index,
  );
  const showMix = props.trackSize !== 'compact';

  const onResizePointerDown = (event: React.PointerEvent) => {
    event.preventDefault();
    event.stopPropagation();
    if (!props.onSetTrackSize) return;
    const sizes: TrackSize[] = ['compact', 'normal', 'large'];
    const startIndex = sizes.indexOf(props.trackSize);
    const startY = event.clientY;
    let lastIndex = startIndex;
    const onMove = (e: PointerEvent) => {
      const delta = e.clientY - startY;
      const targetIndex = Math.max(
        0,
        Math.min(sizes.length - 1, startIndex + Math.round(delta / 25)),
      );
      if (targetIndex !== lastIndex) {
        lastIndex = targetIndex;
        props.onSetTrackSize!(sizes[targetIndex]);
      }
    };
    const onUp = () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  };

  const activeMonitoring = pendingTrackValues.monitoring ?? props.track.monitoring;
  const monitoringClass =
    activeMonitoring === 'auto' ? styles.monAuto : activeMonitoring === 'on' ? styles.monOn : '';

  return (
    <div
      className={styles.trackRow}
      style={{ '--track-height': `${laneCount * laneHeight}px` } as CSSProperties}
      data-arrange-track
      data-track-id={props.track.id}
      data-selected={props.selected || undefined}
      data-focused={props.focused || undefined}
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => props.onDrop(event, props.track.id, props.track.kind)}
    >
      <aside
        className={styles.trackHeader}
        onClick={(event) => {
          if (!(event.target as HTMLElement).closest('button, input, details, summary')) {
            props.onSelectTrack();
          }
        }}
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
            {renaming ? (
              <input
                autoFocus
                className={styles.trackNameInput}
                defaultValue={props.track.name}
                onClick={(e) => e.stopPropagation()}
                onBlur={(event) => {
                  const name = event.currentTarget.value.trim();
                  if (name && name !== props.track.name) props.onRename(name);
                  setRenaming(false);
                }}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    const name = event.currentTarget.value.trim();
                    if (name && name !== props.track.name) props.onRename(name);
                    setRenaming(false);
                  }
                  if (event.key === 'Escape') setRenaming(false);
                }}
              />
            ) : (
              <span
                className={styles.trackName}
                onDoubleClick={(event) => {
                  event.stopPropagation();
                  setRenaming(true);
                }}
                title="Double-click to rename"
              >
                {props.track.name}
              </span>
            )}
          </div>
          <small>
            {props.track.kind === 'instrument' ? 'INSTRUMENT' : 'AUDIO'} ·{' '}
            {props.timeline.items.length} CLIP
            {props.timeline.items.length === 1 ? '' : 'S'}
          </small>
        </div>
        <div className={styles.trackSwitches}>
          {props.track.kind === 'instrument' && props.onOpenPlaySurface && (
            <button
              className={styles.playSurfaceButton}
              aria-label={`Open Play Surface for ${props.track.name}`}
              title="Open Play Surface"
              onClick={(event) => {
                event.stopPropagation();
                props.onOpenPlaySurface?.();
              }}
            >
              ⌨
            </button>
          )}
          <button
            className={(pendingTrackValues.muted ?? props.track.muted) ? styles.muteActive : ''}
            aria-pressed={pendingTrackValues.muted ?? props.track.muted}
            aria-label={`Mute ${props.track.name}`}
            title="Mute"
            onClick={() =>
              commitTrackValue(
                'muted',
                !(pendingTrackValues.muted ?? props.track.muted),
                `${props.track.name} mute updated.`,
              )
            }
          >
            M
          </button>
          <button
            className={(pendingTrackValues.solo ?? props.track.solo) ? styles.soloActive : ''}
            aria-pressed={pendingTrackValues.solo ?? props.track.solo}
            aria-label={`Solo ${props.track.name}`}
            title="Solo"
            onClick={() =>
              commitTrackValue(
                'solo',
                !(pendingTrackValues.solo ?? props.track.solo),
                `${props.track.name} solo updated.`,
              )
            }
          >
            S
          </button>
          <button
            className={`${styles.armButton} ${(pendingTrackValues.armed ?? props.track.armed) ? styles.armActive : ''}`}
            aria-pressed={Boolean(pendingTrackValues.armed ?? props.track.armed)}
            aria-label={`${(pendingTrackValues.armed ?? props.track.armed) ? 'Disarm' : 'Arm'} ${props.track.name} for recording`}
            title={
              (pendingTrackValues.armed ?? props.track.armed)
                ? 'Disarm for recording'
                : 'Arm for recording'
            }
            onClick={() =>
              commitTrackValue(
                'armed',
                !(pendingTrackValues.armed ?? props.track.armed),
                `${props.track.name} ${(pendingTrackValues.armed ?? props.track.armed) ? 'disarmed' : 'armed'}.`,
              )
            }
          >
            ●
          </button>
          {props.track.kind === 'audio' && (
            <button
              className={`${styles.monitoringButton} ${monitoringClass}`}
              aria-label={`Cycle input monitoring for ${props.track.name}`}
              title={`Input monitoring: ${activeMonitoring.toUpperCase()} (click to cycle)`}
              onClick={() => {
                const next =
                  activeMonitoring === 'off' ? 'auto' : activeMonitoring === 'auto' ? 'on' : 'off';
                commitTrackValue(
                  'monitoring',
                  next,
                  `${props.track.name} monitoring set to ${next.toUpperCase()}.`,
                );
              }}
            >
              {activeMonitoring === 'off' ? 'IN' : activeMonitoring === 'auto' ? 'A' : 'ON'}
            </button>
          )}
        </div>
        <details ref={detailsRef} className={styles.trackMenu}>
          <summary aria-label={`${props.track.name} track menu`}>•••</summary>
          <div>
            <button
              onClick={(event) => {
                setRenaming(true);
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
            <button
              onClick={(event) => {
                props.onToggleAutomation();
                event.currentTarget.closest('details')?.removeAttribute('open');
              }}
            >
              {props.automationOpen ? 'Hide' : 'Show'} Automation
            </button>
            <button className={styles.deleteTrack} onClick={props.onDelete}>
              Delete
            </button>
          </div>
        </details>
        {props.trackSize === 'large' && (
          <div className={styles.trackPlugins}>
            {props.track.kind === 'instrument' ? (
              <span>INSTRUMENT · {props.track.instrument?.name ?? 'None'}</span>
            ) : (
              <>
                <span>FX · {props.track.rack.devices.length}</span>
                {props.track.rack.devices.slice(0, 3).map((device) => (
                  <span key={device.id}>{device.name}</span>
                ))}
                {props.track.rack.devices.length > 3 && <span>...</span>}
              </>
            )}
          </div>
        )}
        {showMix && (
          <>
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
                    props.api.updateTrack(props.track.id, {
                      pan: Number(event.currentTarget.value),
                    }),
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
          </>
        )}
      </aside>
      <div
        className={`${styles.lane} ${dragOver ? styles.laneDragOver : ''}`}
        style={{ width: props.timelineWidth }}
        onDragOver={(event) => {
          if (!event.dataTransfer.types.includes(RIFFRA_ASSET_MIME)) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = 'copy';
        }}
        onDragEnter={() => setDragOver(true)}
        onDragLeave={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node)) setDragOver(false);
        }}
        onDrop={(event) => {
          event.stopPropagation();
          setDragOver(false);
          props.onDrop(event, props.track.id, props.track.kind);
        }}
        onContextMenu={(event) => {
          if ((event.target as HTMLElement).closest('[data-clip-id]')) return;
          const bounds = event.currentTarget.getBoundingClientRect();
          const tick = Math.max(0, (event.clientX - bounds.left) / props.pixelsPerTick);
          props.onContextMenu?.(event, props.track.id, tick);
        }}
      >
        {bars.map((bar) => (
          <i key={bar} style={{ left: bar * barTicks * props.pixelsPerTick }} />
        ))}
        {audioItems.map(({ clip, key }) => (
          <AudioClipView
            key={key}
            clip={clip}
            analysis={props.analyses[clip.assetId]}
            timebase={props.timebase}
            pixelsPerTick={props.pixelsPerTick}
            lane={props.timeline.lanes.get(key) ?? 0}
            laneHeight={laneHeight}
            selected={props.selectedClipIds.includes(clip.id)}
            missing={props.unavailableClipIds.includes(clip.id)}
            onSelect={props.onSelect}
            onMove={props.onMove}
            onTrim={props.onTrim}
            onFade={props.onFade}
            onContextMenu={props.onAudioClipContextMenu}
          />
        ))}
        {midiItems.map(({ clip, key }) => (
          <MidiClipView
            key={key}
            clip={clip}
            pixelsPerTick={props.pixelsPerTick}
            lane={props.timeline.lanes.get(key) ?? 0}
            laneHeight={laneHeight}
            selected={props.selectedClipIds.includes(clip.id)}
            onSelect={props.onSelect}
            onMove={props.onMoveMidi}
            onTrim={props.onTrimMidi}
            onOpenEditor={props.onOpenMidiEditor}
            onContextMenu={props.onMidiClipContextMenu}
          />
        ))}
      </div>
      <div className={styles.resizeHandle} onPointerDown={onResizePointerDown} />
    </div>
  );
}
