import { useState } from 'react';
import type { MixBusPatch } from '@/model/generated';
import type { MixBus } from '@/model/generated';
import { MIX_GAIN_DB, MIX_PAN } from '@/model/mix';
import { formatGainDb, formatPan } from '@/model/mix';
import { useSessionContext } from '@/session/session-context';
import { useChannelMixValue } from '@/session/hooks/useChannelMixValue';
import { Icon } from '@/shared/ui/Icon';
import controls from '@/shared/ui/controls.module.css';
import { ValueSlider } from '@/shared/ui/ValueSlider';
import styles from './Mixer.module.css';
import { MixerEffectSlots } from './MixerEffectSlots';
import { MixerMeter } from './MixerMeter';

interface Props {
  sessionId: string;
  bus: MixBus;
  automated: boolean;
  selected: boolean;
  missingDeviceIds: readonly string[];
  onSelect: () => void;
  onOpenDevices: () => void;
  onOpenAutomation: () => void;
  onRemove: () => void;
  onError: (message: string) => void;
  disabled?: boolean;
}

export function MixerBusChannelStrip(props: Props) {
  const { api, commit: sessionCommit } = useSessionContext();
  const { bus } = props;
  const [renaming, setRenaming] = useState(false);
  const gain = useChannelMixValue(bus, 'gainDb', props);
  const pan = useChannelMixValue(bus, 'pan', props);
  const commit = async (patch: MixBusPatch) => {
    await sessionCommit(api.updateMixBus(bus.id, patch), { onError: props.onError });
  };
  return (
    <article
      className={`${styles.channel} ${styles.busChannel} ${props.selected ? styles.selected : ''}`}
      aria-label={`${bus.name} mixer channel`}
      onPointerDown={props.onSelect}
    >
      <div className={styles.busIdentityRow}>
        {renaming ? (
          <input
            autoFocus
            className={controls.control}
            aria-label="Channel name"
            defaultValue={bus.name}
            onBlur={(e) => {
              const next = e.currentTarget.value.trim();
              if (next && next !== bus.name) void commit({ name: next });
              setRenaming(false);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                const next = e.currentTarget.value.trim();
                if (next && next !== bus.name) void commit({ name: next });
                setRenaming(false);
              }
              if (e.key === 'Escape') setRenaming(false);
            }}
          />
        ) : (
          <button
            type="button"
            className={styles.channelIdentity}
            onClick={props.onSelect}
            onDoubleClick={() => setRenaming(true)}
            title={`${bus.name} — double-click to rename`}
          >
            <span className={styles.trackColor} style={{ background: 'var(--color-accent)' }} />
            <strong>{bus.name}</strong>
            <small>{bus.kind.toUpperCase()}</small>
          </button>
        )}
        <button
          type="button"
          className={`${controls.iconButton} ${controls.danger} ${styles.busRemove}`}
          aria-label={`Remove ${bus.name}`}
          title="Remove channel"
          onClick={(e) => {
            e.stopPropagation();
            props.onRemove();
          }}
        >
          <Icon name="close" />
        </button>
      </div>
      <MixerEffectSlots
        owner={{ kind: 'bus', id: bus.id }}
        effects={bus.effects}
        missingDeviceIds={props.missingDeviceIds}
        onOpenDevices={props.onOpenDevices}
        onError={props.onError}
      />
      <button
        type="button"
        className={`${styles.routeButton}${props.automated ? ` ${styles.autoActive}` : ''}`}
        onClick={props.onOpenAutomation}
      >
        <Icon name="curve" /> Automation
      </button>
      <label className={styles.panControl}>
        <span>
          PAN <output>{formatPan(pan.value)}</output>
        </span>
        <ValueSlider
          control={pan}
          range={MIX_PAN}
          fillOrigin={0}
          ariaLabel={`${bus.name} pan`}
          disabled={props.disabled}
        />
      </label>
      <div className={styles.mixControl}>
        <MixerMeter owner={{ kind: 'bus', id: bus.id }} />
        <label className={styles.faderControl}>
          <span className={styles.faderLabel}>GAIN</span>
          <ValueSlider
            control={gain}
            range={MIX_GAIN_DB}
            fillOrigin={MIX_GAIN_DB.min}
            orientation="vertical"
            ariaLabel={`${bus.name} gain`}
            disabled={props.disabled}
          />
          <output>{formatGainDb(gain.value)}</output>
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
