import { resolveTrackColor } from '@/model/track-color';
import type { AutomationLane, Track } from '@/model/generated';
import { MIX_GAIN_DB, MIX_PAN } from '@/model/mix-range';
import { formatGainDb, formatPan } from '@/shared/audio/mix-format';
import { useSessionContext } from '@/shared/session/session-context';
import { Icon } from '@/shared/ui/primitives';
import { ValueSlider } from '@/shared/ui/ValueSlider';
import { useState, type CSSProperties } from 'react';
import { useChannelMixValue } from '@/shared/session/useChannelMixValue';
import styles from './Mixer.module.css';
import { MixerMeter } from './MixerMeter';
interface MixerTrackChannelStripProps {
  sessionId: string;
  track: Track;
  trackIndex: number;
  volumeAutomation: AutomationLane | undefined;
  panAutomation: AutomationLane | undefined;
  selected: boolean;
  missingDeviceIds: readonly string[];
  onSelect: () => void;
  onOpenDevices: () => void;
  onOpenAutomation: () => void;
  onOpenRouting: () => void;
  outputName: string;
  sendCount: number;
  onError?: (message: string) => void;
  disabled?: boolean;
}
export function MixerTrackChannelStrip(props: MixerTrackChannelStripProps) {
  const { api, commit: sessionCommit } = useSessionContext();
  const { track } = props;
  const [pendingSwitch, setPendingSwitch] = useState<'muted' | 'solo' | 'armed' | null>(null);
  const gain = useChannelMixValue(track, 'gainDb', {
    sessionId: props.sessionId,
    onError: props.onError,
    disabled: props.disabled,
  });
  const pan = useChannelMixValue(track, 'pan', {
    sessionId: props.sessionId,
    onError: props.onError,
    disabled: props.disabled,
  });
  const color = resolveTrackColor(track, props.trackIndex);
  const fxCount = track.effects.length;
  const hasMissingFx = track.effects.some(
    (device) => device.plugin.disabledPlaceholder || props.missingDeviceIds.includes(device.id),
  );
  const commitSwitch = async (field: 'muted' | 'solo' | 'armed') => {
    if (props.disabled || pendingSwitch !== null) return;
    props.onSelect();
    setPendingSwitch(field);
    try {
      await sessionCommit(api.updateTrack(track.id, { [field]: !track[field] }), {
        onError: props.onError,
      });
    } finally {
      setPendingSwitch(null);
    }
  };
  return (
    <article
      className={`${styles.channel}${props.selected ? ` ${styles.selected}` : ''}`}
      style={{ '--track-color': color } as CSSProperties}
      data-selected={props.selected}
      onPointerDown={props.onSelect}
      aria-label={`${track.name} mixer channel`}
    >
      <button
        className={styles.channelIdentity}
        type="button"
        onClick={props.onSelect}
        title={track.name}
      >
        <span className={styles.trackColor} />
        <strong>{track.name}</strong>
        <small>{track.kind === 'instrument' ? 'INSTRUMENT' : 'AUDIO'}</small>
      </button>

      <button
        type="button"
        className={`${styles.fxSummary}${hasMissingFx ? ` ${styles.warning}` : ''}`}
        onClick={(event) => {
          event.stopPropagation();
          props.onOpenDevices();
        }}
        title={hasMissingFx ? 'Missing effect device' : 'Open Track Devices'}
      >
        <Icon name="module" /> FX {fxCount === 0 ? '—' : fxCount}
      </button>

      <button type="button" className={styles.routeButton} onClick={props.onOpenRouting}>
        {props.outputName} · {props.sendCount} Sends
      </button>
      <button type="button" className={styles.routeButton} onClick={props.onOpenAutomation}>
        Automation
      </button>
      <label className={styles.panControl}>
        <span>
          PAN · STATIC <output>{formatPan(pan.value)}</output>
          {props.panAutomation?.points.length ? <b>AUTO</b> : null}
        </span>
        <ValueSlider
          control={pan}
          range={MIX_PAN}
          fillOrigin={0}
          disabled={props.disabled}
          ariaLabel={`${track.name} pan`}
        />
      </label>

      <div className={styles.mixControl}>
        <MixerMeter trackId={track.id} />

        <label className={styles.faderControl}>
          <span className={styles.faderLabel}>
            <span>GAIN · STATIC</span>
            {props.volumeAutomation?.points.length ? <b>AUTO</b> : null}
          </span>
          <ValueSlider
            control={gain}
            range={MIX_GAIN_DB}
            fillOrigin={MIX_GAIN_DB.min}
            orientation="vertical"
            disabled={props.disabled}
            ariaLabel={`${track.name} gain`}
          />
          <output>{formatGainDb(gain.value)}</output>
        </label>
      </div>

      <div className={styles.switches} role="group" aria-label={`${track.name} mix switches`}>
        {(
          [
            ['muted', 'M', 'Mute'],
            ['solo', 'S', 'Solo'],
            ['armed', 'R', 'Record arm'],
          ] as const
        ).map(([field, label, title]) => {
          const active = pendingSwitch === field ? !track[field] : track[field];
          return (
            <button
              key={field}
              type="button"
              aria-label={`${title} ${track.name}`}
              aria-pressed={active}
              className={active ? styles.switchActive : undefined}
              disabled={props.disabled || pendingSwitch !== null}
              onClick={(event) => {
                event.stopPropagation();
                void commitSwitch(field);
              }}
            >
              {label}
            </button>
          );
        })}
      </div>
    </article>
  );
}
