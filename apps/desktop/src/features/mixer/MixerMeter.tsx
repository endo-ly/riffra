import { useMemo } from 'react';
import { useAudioMeters } from '@/shared/audio/audio-meters';
import styles from './Mixer.module.css';

interface MixerMeterProps {
  trackId?: string;
  master?: boolean;
  mode?: 'peak' | 'peak-rms';
}

interface MeterValues {
  peakLeft: number;
  peakRight: number;
  rmsLeft?: number;
  rmsRight?: number;
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

function meterValues(
  meters: ReturnType<typeof useAudioMeters>,
  trackId?: string,
  master?: boolean,
): MeterValues | null {
  if (!meters.available) return null;
  if (master) {
    return {
      peakLeft: meters.outputPeakLeft,
      peakRight: meters.outputPeakRight,
    };
  }
  return meters.trackMeters.find((meter) => meter.trackId === trackId) ?? null;
}

export function MixerMeter({ mode = 'peak-rms', ...props }: MixerMeterProps) {
  const meters = useAudioMeters();
  const meter = useMemo(
    () => meterValues(meters, props.trackId, props.master),
    [meters, props.master, props.trackId],
  );
  const label = props.master ? 'Master stereo meter' : `${props.trackId ?? 'Track'} stereo meter`;
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
            <span>{channel.toUpperCase()}</span>
            <small>{meter === null ? '—' : formatDb(peak)}</small>
          </div>
        );
      })}
    </div>
  );
}
