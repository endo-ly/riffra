import { useState, type CSSProperties } from 'react';
import type { AutomationLane, CanonicalState, Track } from '@/model/domain';
import type { ArrangeApi, AudioApi } from '@/native/native-api';
import { Icon } from '@/shared/ui/primitives';
import { resolveTrackColor } from '@/features/arrange/model/track-colors';
import { MixerMeter } from './MixerMeter';
import { useTrackMixControl } from './hooks/useTrackMixControl';
import styles from './Mixer.module.css';
type MixerTrackApi = Pick<ArrangeApi, 'updateTrack'> & Pick<AudioApi, 'previewTrackMix'>;
interface MixerTrackChannelStripProps {
  sessionId: string;
  track: Track;
  trackIndex: number;
  volumeAutomation: AutomationLane | undefined;
  panAutomation: AutomationLane | undefined;
  selected: boolean;
  missingDeviceIds: readonly string[];
  api: MixerTrackApi;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  onSelect: () => void;
  onOpenDevices: () => void;
  onError?: (message: string) => void;
  disabled?: boolean;
}
export function MixerTrackChannelStrip(props: MixerTrackChannelStripProps) {
  const { track } = props;
  const [pendingSwitch, setPendingSwitch] = useState<'muted' | 'solo' | 'armed' | null>(null);
  const mix = useTrackMixControl({
    sessionId: props.sessionId,
    track,
    api: props.api,
    applyCanonicalState: props.applyCanonicalState,
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
      const result = await props.api.updateTrack(track.id, { [field]: !track[field] });
      if (!props.applyCanonicalState(result.canonical)) return;
      if (result.projection.state === 'failed') props.onError?.(result.projection.message);
    } catch (error) {
      props.onError?.(error instanceof Error ? error.message : String(error));
    } finally {
      setPendingSwitch(null);
    }
  };
  const commitValue = (parameter: 'gainDb' | 'pan', value: number) => {
    void mix.commit(parameter, value);
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

      <label className={styles.panControl}>
        <span>
          PAN <output>{formatPan(mix.pan)}</output>
          {props.panAutomation?.points.length ? <b>AUTO</b> : null}
        </span>
        <input
          type="range"
          min="-1"
          max="1"
          step="0.01"
          value={mix.pan}
          disabled={props.disabled}
          aria-label={`${track.name} pan`}
          onPointerDown={() => mix.beginInteraction('pan')}
          onChange={(event) => {
            const value = Number(event.currentTarget.value);
            mix.setPan(value);
            mix.schedulePreview('pan', value);
          }}
          onPointerUp={(event) => commitValue('pan', Number(event.currentTarget.value))}
          onPointerCancel={() => mix.cancel('pan')}
          onKeyDown={(event) => {
            if (event.key === 'Escape') mix.cancel('pan');
            else if (isMixAdjustmentKey(event.key)) mix.beginInteraction('pan');
          }}
          onKeyUp={(event) => {
            if (isMixAdjustmentKey(event.key))
              commitValue('pan', Number(event.currentTarget.value));
          }}
          onDoubleClick={() => {
            mix.setPan(0);
            mix.schedulePreview('pan', 0);
            commitValue('pan', 0);
          }}
          onBlur={(event) => commitValue('pan', Number(event.currentTarget.value))}
        />
      </label>

      <div className={styles.mixControl}>
        <MixerMeter trackId={track.id} />

        <label className={styles.faderControl}>
          <span className={styles.faderLabel}>
            <span>GAIN</span>
            {props.volumeAutomation?.points.length ? <b>AUTO</b> : null}
          </span>
          <input
            type="range"
            min="-90"
            max="24"
            step="0.1"
            value={mix.gainDb}
            disabled={props.disabled}
            aria-label={`${track.name} gain`}
            onPointerDown={() => mix.beginInteraction('gainDb')}
            onChange={(event) => {
              const value = Number(event.currentTarget.value);
              mix.setGainDb(value);
              mix.schedulePreview('gainDb', value);
            }}
            onPointerUp={(event) => commitValue('gainDb', Number(event.currentTarget.value))}
            onPointerCancel={() => mix.cancel('gainDb')}
            onKeyDown={(event) => {
              if (event.key === 'Escape') mix.cancel('gainDb');
              else if (isMixAdjustmentKey(event.key)) mix.beginInteraction('gainDb');
            }}
            onKeyUp={(event) => {
              if (isMixAdjustmentKey(event.key))
                commitValue('gainDb', Number(event.currentTarget.value));
            }}
            onDoubleClick={() => {
              mix.setGainDb(0);
              mix.schedulePreview('gainDb', 0);
              commitValue('gainDb', 0);
            }}
            onBlur={(event) => commitValue('gainDb', Number(event.currentTarget.value))}
          />
          <output>{formatDb(mix.gainDb)}</output>
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
function isMixAdjustmentKey(key: string): boolean {
  return ['ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown'].includes(key);
}
function formatPan(pan: number): string {
  if (Math.abs(pan) < 0.005) return 'C';
  return `${pan < 0 ? 'L' : 'R'} ${Math.round(Math.abs(pan) * 100)}`;
}
function formatDb(gainDb: number): string {
  if (gainDb <= -90) return '−∞ dB';
  return `${gainDb >= 0 ? '+' : ''}${gainDb.toFixed(1)} dB`;
}
