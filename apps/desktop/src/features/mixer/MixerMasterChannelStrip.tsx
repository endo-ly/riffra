import { useMasterGainControl } from '@/features/audio/hooks/useMasterGainControl';
import type { CreativeSession } from '@/model/domain';
import { useAudioMeters } from '@/shared/audio/audio-meters';
import { formatGainDb } from '@/shared/audio/mix-format';
import { Icon } from '@/shared/ui/primitives';
import { useCallback, useEffect, useRef, useState } from 'react';
import styles from './Mixer.module.css';
import { MixerMeter } from './MixerMeter';

interface MixerMasterChannelStripProps {
  session: CreativeSession;
  onOpenDevices: () => void;
  onOpenAutomation: () => void;
  onOpenMixdown: () => void;
  missingDeviceIds: readonly string[];
  disabled?: boolean;
}

export function MixerMasterChannelStrip(props: MixerMasterChannelStripProps) {
  const meters = useAudioMeters();
  const previousClipCount = useRef<number | null>(null);
  const [clipLatched, setClipLatched] = useState(false);
  useEffect(() => {
    if (!meters.available) {
      previousClipCount.current = null;
      setClipLatched(false);
      return undefined;
    }
    if (previousClipCount.current === null) {
      previousClipCount.current = meters.hardClipSamples;
      return undefined;
    }
    if (meters.hardClipSamples > previousClipCount.current) {
      setClipLatched(true);
      const timer = window.setTimeout(() => setClipLatched(false), 1_500);
      previousClipCount.current = meters.hardClipSamples;
      return () => window.clearTimeout(timer);
    }
    if (meters.hardClipSamples < previousClipCount.current) setClipLatched(false);
    previousClipCount.current = meters.hardClipSamples;
    return undefined;
  }, [meters.available, meters.hardClipSamples]);
  const master = useMasterGainControl({
    session: props.session,
    disabled: props.disabled,
  });
  const commit = useCallback(
    (value: number) => {
      void master.commit(value);
    },
    [master],
  );
  const safetyWarning =
    meters.available &&
    (meters.feedbackSuspected || clipLatched || meters.limiterGainReductionDb > 0);

  return (
    <aside className={styles.masterChannel} aria-label="Master mixer channel">
      <div className={styles.masterHeader}>
        <span className={styles.masterBadge}>
          <Icon name="mixer" />
        </span>
        <strong>MASTER</strong>
        <small>STEREO OUTPUT</small>
      </div>

      <div className={styles.masterActions}>
        <button type="button" className={styles.fxSummary} onClick={props.onOpenDevices}>
          FX {props.session.arrangement.mixer.masterEffects.length}
          {props.session.arrangement.mixer.masterEffects.some(
            (device) =>
              device.plugin.disabledPlaceholder || props.missingDeviceIds.includes(device.id),
          )
            ? ' · MISSING'
            : ''}
        </button>
        <button type="button" className={styles.routeButton} onClick={props.onOpenAutomation}>
          Automation
        </button>
        <button type="button" className={styles.mixdownButton} onClick={props.onOpenMixdown}>
          Mixdown…
        </button>
      </div>
      <div className={styles.masterMixControl}>
        <MixerMeter master mode="peak" />

        <label className={styles.masterFader}>
          <span>MASTER GAIN</span>
          <input
            type="range"
            min="-90"
            max="0"
            step="0.5"
            value={master.draftDb}
            disabled={props.disabled}
            aria-label="Master mixer gain"
            onPointerDown={master.beginEditing}
            onKeyDown={(event) => {
              if (
                ['ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown'].includes(event.key)
              )
                master.beginEditing();
            }}
            onChange={(event) => {
              const value = Number(event.currentTarget.value);
              master.setDraftDb(value);
              master.preview(value);
            }}
            onPointerUp={(event) => commit(Number(event.currentTarget.value))}
            onBlur={(event) => commit(Number(event.currentTarget.value))}
            onKeyUp={(event) => {
              if (
                ['ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown'].includes(event.key)
              ) {
                commit(Number(event.currentTarget.value));
              }
            }}
          />
          <output>{formatGainDb(master.draftDb)}</output>
        </label>
      </div>

      <section className={`${styles.safety} ${safetyWarning ? styles.safetyWarning : ''}`}>
        <span className={styles.safetyTitle}>
          <Icon name={safetyWarning ? 'stop' : 'speaker'} /> SAFETY
        </span>
        <dl className={styles.diagnostics}>
          <div>
            <dt>PRE</dt>
            <dd>{meters.available ? `${formatMeterDb(meters.preLimiterPeak)} dB` : '—'}</dd>
          </div>
          <div>
            <dt>LIMITER</dt>
            <dd>{meters.available ? `${meters.limiterGainReductionDb.toFixed(1)} dB` : '—'}</dd>
          </div>
          <div>
            <dt>CLIP</dt>
            <dd>{clipLatched ? 'CLIP' : '—'}</dd>
          </div>
          <div>
            <dt>FEEDBACK</dt>
            <dd>{meters.available ? (meters.feedbackSuspected ? 'CHECK' : 'CLEAR') : '—'}</dd>
          </div>
        </dl>
      </section>
    </aside>
  );
}

function formatMeterDb(value: number): string {
  if (value <= 0 || !Number.isFinite(value)) return '−∞';
  const db = 20 * Math.log10(value);
  return `${db >= 0 ? '+' : ''}${db.toFixed(1)}`;
}
