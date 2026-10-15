import type { CanonicalState, MixBus } from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { MixerMeter } from './MixerMeter';
import { useChannelMixControl } from './hooks/useChannelMixControl';
import { formatGainDb, formatPan } from '@/shared/audio/mix-format';
import styles from './Mixer.module.css';

interface Props {
  sessionId: string;
  bus: MixBus;
  automated: boolean;
  selected: boolean;
  api: ArrangeWorkspaceApi;
  missingDeviceIds: readonly string[];
  applyCanonicalState: (state: CanonicalState) => boolean;
  onSelect: () => void;
  onOpenDevices: () => void;
  onOpenAutomation: () => void;
  onError: (message: string) => void;
  disabled?: boolean;
}

export function MixerBusChannelStrip(props: Props) {
  const { bus } = props;
  const mix = useChannelMixControl({ ...props, channel: bus });
  const commit = async (patch: Parameters<ArrangeWorkspaceApi['updateMixBus']>[1]) => {
    try {
      const result = await props.api.updateMixBus(bus.id, patch);
      if (props.applyCanonicalState(result.canonical) && result.projection.state === 'failed')
        props.onError(result.projection.message);
    } catch (error) {
      props.onError(error instanceof Error ? error.message : String(error));
    }
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
        <input
          aria-label={`${bus.name} pan`}
          type="range"
          min={-1}
          max={1}
          step={0.01}
          value={mix.pan}
          disabled={props.disabled}
          onPointerDown={() => mix.beginInteraction('pan')}
          onChange={(e) => {
            const value = Number(e.currentTarget.value);
            mix.setPan(value);
            mix.schedulePreview('pan', value);
          }}
          onPointerUp={(e) => void mix.commit('pan', Number(e.currentTarget.value))}
          onPointerCancel={() => mix.cancel('pan')}
          onKeyDown={(e) => {
            if (e.key === 'Escape') mix.cancel('pan');
            else mix.beginInteraction('pan');
          }}
          onKeyUp={(e) => {
            if (e.key !== 'Escape') void mix.commit('pan', Number(e.currentTarget.value));
          }}
          onBlur={(e) => void mix.commit('pan', Number(e.currentTarget.value))}
        />
      </label>
      <div className={styles.mixControl}>
        <MixerMeter busId={bus.id} />
        <label className={styles.faderControl}>
          <span className={styles.faderLabel}>GAIN</span>
          <input
            aria-label={`${bus.name} gain`}
            type="range"
            min={-90}
            max={24}
            step={0.1}
            value={mix.gainDb}
            disabled={props.disabled}
            onPointerDown={() => mix.beginInteraction('gainDb')}
            onChange={(e) => {
              const value = Number(e.currentTarget.value);
              mix.setGainDb(value);
              mix.schedulePreview('gainDb', value);
            }}
            onPointerUp={(e) => void mix.commit('gainDb', Number(e.currentTarget.value))}
            onPointerCancel={() => mix.cancel('gainDb')}
            onKeyDown={(e) => {
              if (e.key === 'Escape') mix.cancel('gainDb');
              else mix.beginInteraction('gainDb');
            }}
            onKeyUp={(e) => {
              if (e.key !== 'Escape') void mix.commit('gainDb', Number(e.currentTarget.value));
            }}
            onBlur={(e) => void mix.commit('gainDb', Number(e.currentTarget.value))}
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
