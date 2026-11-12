import { useAudioChannelMeter } from '@/shared/audio/audio-meters';
import type { EffectOwner } from '@/model/generated';
import styles from './Mixer.module.css';

interface MixerMeterProps {
  owner: EffectOwner;
  mode?: 'peak' | 'peak-rms';
}

function dbfs(value: number): number {
  return value > 0 ? 20 * Math.log10(value) : -Infinity;
}

function levelPercent(value: number): number {
  const db = dbfs(value);
  if (!Number.isFinite(db)) return 0;
  return Math.max(0, Math.min(100, ((db + 60) / 66) * 100));
}

function meterTone(value: number): string {
  const db = dbfs(value);
  if (db >= 0) return styles.meterDanger;
  if (db >= -6) return styles.meterWarning;
  return styles.meterNormal;
}

function formatDb(value: number): string {
  const db = dbfs(value);
  if (!Number.isFinite(db) || db <= -60) return '−∞';
  return `${db >= 0 ? '+' : ''}${db.toFixed(1)}`;
}

export function MixerMeter({ mode = 'peak-rms', owner }: MixerMeterProps) {
  const meter = useAudioChannelMeter(owner);
  const label = owner.kind === 'master' ? 'Master stereo meter' : `${owner.id} stereo meter`;
  return (
    <div className={styles.meterGroup} aria-label={label} data-unavailable={meter === null}>
      {(['left', 'right'] as const).map((channel) => {
        const peak = channel === 'left' ? (meter?.peakLeft ?? 0) : (meter?.peakRight ?? 0);
        const rms = channel === 'left' ? (meter?.rmsLeft ?? 0) : (meter?.rmsRight ?? 0);
        return (
          <div className={styles.meterColumn} key={channel}>
            <div className={`${styles.meterTrack} ${meterTone(peak)}`}>
              {mode === 'peak-rms' ? (
                <i className={styles.meterRms} style={{ height: `${levelPercent(rms)}%` }} />
              ) : null}
              <b className={styles.meterPeak} style={{ bottom: `${levelPercent(peak)}%` }} />
            </div>
            <span>{channel === 'left' ? 'L' : 'R'}</span>
            <small>{meter === null ? '—' : formatDb(peak)}</small>
          </div>
        );
      })}
    </div>
  );
}
