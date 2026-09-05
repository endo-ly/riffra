import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import type {
  AudioAnalysis,
  AudioClip,
  ArrangementMutationResult,
  CreativeSession,
  MidiClip,
  MonitoringState,
  ProjectTimebase,
  Track,
} from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import { AudioClipView } from './AudioClipView';
import { MidiClipView } from './MidiClipView';
import {
  trackLaneHeight,
  type ArrangeAudioTimelineItem,
  type ArrangeMidiTimelineItem,
  type TrackTimeline,
  type TrackSize,
} from '@/features/arrange/model/arrange-timeline';
import { RIFFRA_ASSET_MIME } from '@/shared/asset-drag';
import { resolveTrackColor } from '../inspector/track-colors';
import { Icon } from '@/shared/ui/primitives';
import controls from '@/shared/ui/controls.module.css';
import styles from '../WorkspaceArrange.module.css';

interface ArrangeTrackProps {
  track: Track;
  trackIndex?: number;
  timeline: TrackTimeline;
  timebase: ProjectTimebase;
  analyses: Record<string, AudioAnalysis | null>;
  selectedClipIds: string[];
  selected: boolean;
  focused: boolean;
  unavailableClipIds: string[];
  timelineWidth: number;
  pixelsPerTick: number;
  trackSize: TrackSize;
  api: ArrangeApi;
  onCommit: (
    operation: Promise<ArrangementMutationResult | null>,
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
  onSelect: (clipId: string, append?: boolean) => void;
  onSelectTrack: () => void;
  onTrim: (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: AudioClip,
    side: 'left' | 'right',
  ) => void;
  onFade: (event: React.PointerEvent<HTMLSpanElement>, clip: AudioClip, side: 'in' | 'out') => void;
  onOpenMidiEditor?: (clip: MidiClip) => void;
  onDoubleClickLane?: (event: React.MouseEvent, trackId: string, tick: number) => void;
  onAudioClipContextMenu?: (event: React.MouseEvent, clip: AudioClip) => void;
  onMidiClipContextMenu?: (event: React.MouseEvent, clip: MidiClip) => void;
  onRename: (name: string) => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onReorder: (sourceTrackId: string, insertAfter: boolean) => void;
  onSetTrackSize?: (size: TrackSize) => void;
  missingDeviceIds: string[];
  onAddDevice: () => void;
  onOpenPluginEditor: (deviceId: string) => void;
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
  const [dropHint, setDropHint] = useState<'before' | 'after' | null>(null);
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
    void props.onCommit(props.api.updateTrack(props.track.id, patch)).then(() => {
      setPendingTrackValues((current) => {
        if (current[field] !== value) return current;
        const next = { ...current };
        delete next[field];
        return next;
      });
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

  const closeMenu = () => detailsRef.current?.removeAttribute('open');
  const availableDevices = [
    ...(props.track.instrument?.source.type === 'vst3'
      ? [
          {
            id: props.track.instrument.id,
            name: props.track.instrument.name,
            unavailable:
              props.track.instrument.source.disabledPlaceholder ||
              props.missingDeviceIds.includes(props.track.instrument.id),
          },
        ]
      : []),
    ...props.track.rack.devices.map((device) => ({
      id: device.id,
      name: device.name,
      unavailable: device.disabledPlaceholder || props.missingDeviceIds.includes(device.id),
    })),
  ].filter((device) => !device.unavailable);
  const trackMenuItems: ReactNode[] = [];
  if (availableDevices.length > 0) {
    for (const device of availableDevices) {
      trackMenuItems.push(
        <button
          key={device.id}
          onClick={() => {
            closeMenu();
            props.onOpenPluginEditor(device.id);
          }}
        >
          Open {device.name}
        </button>,
      );
    }
  } else {
    trackMenuItems.push(
      <button
        key="add-device"
        onClick={() => {
          closeMenu();
          props.onAddDevice();
        }}
      >
        {props.track.kind === 'audio' ? 'Add Effect' : 'Choose Instrument'}
      </button>,
    );
  }
  trackMenuItems.push(
    <hr key="separator-actions" className={styles.menuSeparator} />,
    <button
      key="duplicate"
      onClick={() => {
        closeMenu();
        props.onDuplicate();
      }}
    >
      Duplicate
    </button>,
    <hr key="separator-delete" className={styles.menuSeparator} />,
    <button
      key="delete"
      className={styles.deleteTrack}
      onClick={() => {
        closeMenu();
        props.onDelete();
      }}
    >
      Delete
    </button>,
  );

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
        style={
          {
            '--track-color': resolveTrackColor(props.track, props.trackIndex ?? 0),
          } as CSSProperties
        }
        data-drop={dropHint ?? undefined}
        onClick={(event) => {
          if (!(event.target as HTMLElement).closest('button, input, details, summary')) {
            props.onSelectTrack();
          }
        }}
        onDragOver={(event) => {
          if (!event.dataTransfer.types.includes('application/x-riffra-track')) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = 'move';
          const bounds = event.currentTarget.getBoundingClientRect();
          setDropHint(event.clientY < bounds.top + bounds.height / 2 ? 'before' : 'after');
        }}
        onDragLeave={(event) => {
          if (event.currentTarget.contains(event.relatedTarget as Node)) return;
          setDropHint(null);
        }}
        onDrop={(event) => {
          const sourceTrackId = event.dataTransfer.getData('application/x-riffra-track');
          setDropHint(null);
          if (!sourceTrackId) return;
          event.preventDefault();
          event.stopPropagation();
          const bounds = event.currentTarget.getBoundingClientRect();
          props.onReorder(sourceTrackId, event.clientY >= bounds.top + bounds.height / 2);
        }}
      >
        <div
          className={styles.trackIdentity}
          draggable
          title="Drag to reorder track"
          onDragStart={(event) => {
            event.dataTransfer.effectAllowed = 'move';
            event.dataTransfer.setData('application/x-riffra-track', props.track.id);
          }}
          onDragEnd={() => setDropHint(null)}
        >
          <div className={styles.trackNameRow}>
            <Icon
              name={props.track.kind === 'instrument' ? 'note' : 'wave'}
              className={styles.trackKindIcon}
            />
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
        </div>
        <div className={styles.trackSwitches}>
          <button
            className={(pendingTrackValues.muted ?? props.track.muted) ? styles.muteActive : ''}
            data-state={(pendingTrackValues.muted ?? props.track.muted) ? 'active' : 'idle'}
            aria-pressed={pendingTrackValues.muted ?? props.track.muted}
            aria-label={`Mute ${props.track.name}`}
            title="Mute"
            onClick={() =>
              commitTrackValue('muted', !(pendingTrackValues.muted ?? props.track.muted))
            }
          >
            M
          </button>
          <button
            className={(pendingTrackValues.solo ?? props.track.solo) ? styles.soloActive : ''}
            data-state={(pendingTrackValues.solo ?? props.track.solo) ? 'active' : 'idle'}
            aria-pressed={pendingTrackValues.solo ?? props.track.solo}
            aria-label={`Solo ${props.track.name}`}
            title="Solo"
            onClick={() => commitTrackValue('solo', !(pendingTrackValues.solo ?? props.track.solo))}
          >
            S
          </button>
          <button
            className={`${styles.armButton} ${(pendingTrackValues.armed ?? props.track.armed) ? styles.armActive : ''}`}
            data-state={(pendingTrackValues.armed ?? props.track.armed) ? 'active' : 'idle'}
            aria-pressed={Boolean(pendingTrackValues.armed ?? props.track.armed)}
            aria-label={`${(pendingTrackValues.armed ?? props.track.armed) ? 'Disarm' : 'Arm'} ${props.track.name} for recording`}
            title={
              (pendingTrackValues.armed ?? props.track.armed)
                ? 'Disarm for recording'
                : 'Arm for recording'
            }
            onClick={() =>
              commitTrackValue('armed', !(pendingTrackValues.armed ?? props.track.armed))
            }
          >
            R
          </button>
          {props.track.kind === 'audio' && (
            <button
              className={`${styles.monitoringButton} ${monitoringClass}`}
              data-state={activeMonitoring}
              aria-label={`Cycle input monitoring for ${props.track.name}`}
              title={`Input monitoring: ${activeMonitoring.toUpperCase()} (click to cycle)`}
              onClick={() => {
                const next =
                  activeMonitoring === 'off' ? 'auto' : activeMonitoring === 'auto' ? 'on' : 'off';
                commitTrackValue('monitoring', next);
              }}
            >
              {activeMonitoring === 'off' ? 'IN' : activeMonitoring === 'auto' ? 'A' : 'ON'}
            </button>
          )}
        </div>
        <details ref={detailsRef} className={styles.trackMenu}>
          <summary aria-label={`${props.track.name} track menu`}>
            <Icon name="more" />
          </summary>
          <div>{trackMenuItems}</div>
        </details>
        {showMix && (
          <>
            <label className={styles.trackControl}>
              <span>VOL</span>
              <input
                key={`${props.track.id}:gain:${props.track.gainDb}`}
                className={controls.slider}
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
                  )
                }
              />
              <output>{props.track.gainDb.toFixed(1)}</output>
            </label>
            <label className={styles.trackControl}>
              <span>PAN</span>
              <input
                key={`${props.track.id}:pan:${props.track.pan}`}
                className={controls.slider}
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
        data-midi-empty-lane={props.track.kind === 'instrument' ? true : undefined}
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
        onDoubleClick={(event) => {
          if ((event.target as HTMLElement).closest('[data-clip-id]')) return;
          const bounds = event.currentTarget.getBoundingClientRect();
          const tick = Math.max(0, (event.clientX - bounds.left) / props.pixelsPerTick);
          props.onDoubleClickLane?.(event, props.track.id, tick);
        }}
      >
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
