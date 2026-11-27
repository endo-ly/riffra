import { resolveTrackColor } from '@/model/track-color';
import type { AutomationLane, Track } from '@/model/generated';
import { MIX_GAIN_DB, MIX_PAN } from '@/model/mix';
import { formatGainDb, formatPan } from '@/model/mix';
import { useSessionContext } from '@/session/session-context';
import { Icon } from '@/shared/ui/Icon';
import { ValueSlider } from '@/shared/ui/ValueSlider';
import { useState, type CSSProperties } from 'react';
import { useChannelMixValue } from '@/session/hooks/useChannelMixValue';
import styles from './Mixer.module.css';
import { MixerEffectSlots } from './MixerEffectSlots';
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
  const hasAutomation = Boolean(
    props.volumeAutomation?.points.length || props.panAutomation?.points.length,
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

      <MixerEffectSlots
        owner={{ kind: 'track', id: track.id }}
        effects={track.effects}
        missingDeviceIds={props.missingDeviceIds}
        onOpenDevices={props.onOpenDevices}
        onError={props.onError}
        track={track}
      />

      <div className={styles.stripActions}>
        <button
          type="button"
          className={styles.routeButton}
          onClick={props.onOpenRouting}
          title={`Output: ${props.outputName}, ${props.sendCount} Sends`}
        >
          {props.outputName}
          {props.sendCount > 0 ? ` · ${props.sendCount}` : ''}
        </button>
        <button
          type="button"
          className={`${styles.autoButton}${hasAutomation ? ` ${styles.autoActive}` : ''}`}
          aria-label="Automation"
          title="Open volume automation"
          onClick={props.onOpenAutomation}
        >
          <Icon name="curve" />
        </button>
      </div>
      <label className={styles.panControl}>
        <span>
          PAN <output>{formatPan(pan.value)}</output>
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
        <MixerMeter owner={{ kind: 'track', id: track.id }} />

        <label className={styles.faderControl}>
          <span className={styles.faderLabel}>
            <span>GAIN</span>
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
