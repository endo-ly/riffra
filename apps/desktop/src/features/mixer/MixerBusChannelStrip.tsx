import type { MixBusPatch } from '@/model/domain';
import type { MixBus } from '@/model/domain';
import { formatGainDb, formatPan } from '@/shared/audio/mix-format';
import { useSessionContext } from '@/shared/session/session-context';
import { ChannelMixSlider } from './ChannelMixSlider';
import styles from './Mixer.module.css';
import { MixerMeter } from './MixerMeter';
import { useChannelMixControl } from './hooks/useChannelMixControl';

interface Props {
  sessionId: string;
  bus: MixBus;
  automated: boolean;
  selected: boolean;
  missingDeviceIds: readonly string[];
  onSelect: () => void;
  onOpenDevices: () => void;
  onOpenAutomation: () => void;
  onError: (message: string) => void;
  disabled?: boolean;
}

export function MixerBusChannelStrip(props: Props) {
  const { api, commit: sessionCommit } = useSessionContext();
  const { bus } = props;
  const mix = useChannelMixControl({ ...props, channel: bus });
  const commit = async (patch: MixBusPatch) => {
    await sessionCommit(api.updateMixBus(bus.id, patch), { onError: props.onError });
  };
  const missing = bus.effects.some(
    (device) => device.plugin.disabledPlaceholder || props.missingDeviceIds.includes(device.id),
  );
  return (
    <article
      className={`${styles.channel} ${styles.busChannel} ${props.selected ? styles.selected : ''}`}
      aria-label={`${bus.name} mixer channel`}
      onPointerDown={props.onSelect}
    >
      <button type="button" className={styles.channelIdentity} onClick={props.onSelect}>
        <span className={styles.trackColor} style={{ background: 'var(--color-accent)' }} />
        <strong>{bus.name}</strong>
        <small>{bus.kind.toUpperCase()}</small>
      </button>
      <button
        type="button"
        className={`${styles.fxSummary} ${missing ? styles.warning : ''}`}
        onClick={props.onOpenDevices}
      >
        FX {bus.effects.length}
        {missing ? ' · MISSING' : ''}
      </button>
      <button type="button" className={styles.routeButton} onClick={props.onOpenAutomation}>
        Automation {props.automated ? '· READ' : ''}
      </button>
      <label className={styles.panControl}>
        <span>
          PAN <output>{formatPan(mix.pan)}</output>
        </span>
        <ChannelMixSlider
          mix={mix}
          parameter="pan"
          ariaLabel={`${bus.name} pan`}
          min={-1}
          max={1}
          step={0.01}
          value={mix.pan}
          disabled={props.disabled}
        />
      </label>
      <div className={styles.mixControl}>
        <MixerMeter busId={bus.id} />
        <label className={styles.faderControl}>
          <span className={styles.faderLabel}>GAIN</span>
          <ChannelMixSlider
            mix={mix}
            parameter="gainDb"
            ariaLabel={`${bus.name} gain`}
            min={-90}
            max={24}
            step={0.1}
            value={mix.gainDb}
            disabled={props.disabled}
          />
          <output>{formatGainDb(mix.gainDb)}</output>
        </label>
      </div>
      <div className={styles.switches}>
        <button
          type="button"
          aria-label={`${bus.name} mute`}
          aria-pressed={bus.muted}
          disabled={props.disabled}
          onClick={() => void commit({ muted: !bus.muted })}
        >
          M
        </button>
        <button
          type="button"
          aria-label={`${bus.name} solo`}
          aria-pressed={bus.solo}
          disabled={props.disabled}
          onClick={() => void commit({ solo: !bus.solo })}
        >
          S
        </button>
      </div>
    </article>
  );
}
