import { useEffect, useState } from 'react';
import type { MonitoringState } from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { useChannelMixValue } from '@/shared/session/useChannelMixValue';
import { MIX_GAIN_DB, MIX_PAN } from '@/model/mix-range';
import { ValueSlider } from '@/shared/ui/ValueSlider';
import { formatGainDb, formatPan } from '@/shared/audio/mix-format';
import { Icon } from '@/shared/ui/primitives';
import type { ContextMenuItem } from '@/shared/ui/ContextMenu';
import { MenuButton } from '@/shared/ui/MenuButton';
import { usePointerGesture } from '@/shared/usePointerGesture';
import type { TrackSize } from './model/timeline-layout';
import type { TimelineTrackProps } from './TimelineTrack';
import styles from './TrackHeader.module.css';

interface PendingTrackValues {
  muted?: boolean;
  solo?: boolean;
  armed?: boolean;
  monitoring?: MonitoringState;
}

export function TrackHeader(props: TimelineTrackProps) {
  const { api } = useSessionContext();
  const gain = useChannelMixValue(props.track, 'gainDb', { sessionId: props.sessionId });
  const pan = useChannelMixValue(props.track, 'pan', { sessionId: props.sessionId });
  const startGesture = usePointerGesture();
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
    void props.onCommit(api.updateTrack(props.track.id, patch)).then(() => {
      setPendingTrackValues((current) => {
        if (current[field] !== value) return current;
        const next = { ...current };
        delete next[field];
        return next;
      });
    });
  };

  // Volume shows from the default height up; pan joins it on large tracks.
  const showVolume = props.trackSize !== 'compact';
  const showPan = props.trackSize === 'large';

  const onResizePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.stopPropagation();
    const setTrackSize = props.onSetTrackSize;
    if (!setTrackSize) return;
    const sizes: TrackSize[] = ['compact', 'normal', 'large'];
    const startIndex = sizes.indexOf(props.trackSize);
    const startY = event.clientY;
    let lastIndex = startIndex;
    startGesture(event, {
      onMove: (pointer) => {
        const delta = pointer.clientY - startY;
        const targetIndex = Math.max(
          0,
          Math.min(sizes.length - 1, startIndex + Math.round(delta / 25)),
        );
        if (targetIndex !== lastIndex) {
          lastIndex = targetIndex;
          setTrackSize(sizes[targetIndex]);
        }
      },
      onEnd: () => undefined,
    });
  };

  const activeMonitoring = pendingTrackValues.monitoring ?? props.track.monitoring;
  const muted = pendingTrackValues.muted ?? props.track.muted;
  const solo = pendingTrackValues.solo ?? props.track.solo;
  const monitoringClass =
    activeMonitoring === 'auto' ? styles.monAuto : activeMonitoring === 'on' ? styles.monOn : '';

  const trackMenuItems: ContextMenuItem[] = [
    { label: 'Open Devices', onClick: props.onOpenDevices },
    { separator: true },
    { label: 'Duplicate', onClick: props.onDuplicate },
    { separator: true },
    { label: 'Delete', danger: true, onClick: props.onDelete },
  ];

  return (
    <>
      <aside
        className={styles.trackHeader}
        data-selected={props.selected || undefined}
        data-focused={props.focused || undefined}
        data-inaudible={props.track.muted || (props.soloActive && !props.track.solo) || undefined}
        data-drop={dropHint ?? undefined}
        onClick={(event) => {
          if (!(event.target as HTMLElement).closest('button, input')) {
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
                title={`${props.track.name} — double-click to rename`}
              >
                {props.track.name}
              </span>
            )}
          </div>
        </div>
        <div className={styles.trackSwitches}>
          <button
            className={muted ? styles.muteActive : ''}
            data-state={muted ? 'active' : 'idle'}
            aria-pressed={muted}
            aria-label={`Mute ${props.track.name}`}
            title="Mute"
            onClick={() => commitTrackValue('muted', !muted)}
          >
            M
          </button>
          <button
            className={solo ? styles.soloActive : ''}
            data-state={solo ? 'active' : 'idle'}
            aria-pressed={solo}
            aria-label={`Solo ${props.track.name}`}
            title="Solo"
            onClick={() => commitTrackValue('solo', !solo)}
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
        <MenuButton label={`${props.track.name} track menu`} items={trackMenuItems} />
        {showVolume && (
          <label className={styles.trackControl}>
            <span>VOL</span>
            <ValueSlider
              control={gain}
              range={MIX_GAIN_DB}
              fillOrigin={MIX_GAIN_DB.min}
              ariaLabel={`${props.track.name} gain`}
            />
            <output>{formatGainDb(gain.value)}</output>
          </label>
        )}
        {showPan && (
          <label className={styles.trackControl}>
            <span>PAN</span>
            <ValueSlider
              control={pan}
              range={MIX_PAN}
              fillOrigin={0}
              ariaLabel={`${props.track.name} pan`}
            />
            <output>{formatPan(pan.value)}</output>
          </label>
        )}
      </aside>
      <div className={styles.resizeHandle} onPointerDown={onResizePointerDown} />
    </>
  );
}
