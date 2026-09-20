import { useCallback, useEffect, useRef, useState } from 'react';
import type { AudioStatus, CanonicalState, CreativeSession } from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { useAudioMeters } from '@/shared/audio/audio-meters';
import { Icon } from '@/shared/ui/primitives';
import { MixerMeter } from './MixerMeter';
import { useMasterGainControl } from '@/features/audio/hooks/useMasterGainControl';
import styles from './Mixer.module.css';

interface MixerMasterChannelStripProps {
  session: CreativeSession;
  api: Pick<ArrangeWorkspaceApi, 'previewMasterGainDb' | 'setMasterGainDb'>;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  setAudio: (audio: AudioStatus) => void;
  disabled?: boolean;
}

export function MixerMasterChannelStrip(props: MixerMasterChannelStripProps) {
  const meters = useAudioMeters();
  const previousClipCount = useRef(meters.hardClipSamples);
  const [clipLatched, setClipLatched] = useState(false);
  useEffect(() => {
    if (meters.hardClipSamples > previousClipCount.current) {
      setClipLatched(true);
      const timer = window.setTimeout(() => setClipLatched(false), 1_500);
      previousClipCount.current = meters.hardClipSamples;
      return () => window.clearTimeout(timer);
    }
    if (meters.hardClipSamples < previousClipCount.current) setClipLatched(false);
    previousClipCount.current = meters.hardClipSamples;
    return undefined;
  }, [meters.hardClipSamples]);
  const master = useMasterGainControl({
    session: props.session,
    applyCanonicalState: props.applyCanonicalState,
    setAudio: props.setAudio,
    api: props.api,
    disabled: props.disabled,
  });
  const commit = useCallback(
    (value: number) => {
      void master.commit(value);
    },
    [master],
  );
  const safetyWarning =
    meters.feedbackSuspected || clipLatched || meters.limiterGainReductionDb > 0;

  return (
    <aside className={styles.masterChannel} aria-label="Master mixer channel">
      <div className={styles.masterHeader}>
        <span className={styles.masterBadge}>
          <Icon name="mixer" />
        </span>
        <strong>MASTER</strong>
        <small>STEREO OUTPUT</small>
      </div>

      <MixerMeter master />

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
        <output>{formatDb(master.draftDb)}</output>
      </label>

      <section className={`${styles.safety} ${safetyWarning ? styles.safetyWarning : ''}`}>
        <span className={styles.safetyTitle}>
          <Icon name={safetyWarning ? 'stop' : 'speaker'} /> SAFETY
        </span>
        <dl className={styles.diagnostics}>
          <div>
            <dt>PRE</dt>
            <dd>{formatMeterDb(meters.preLimiterPeak)} dB</dd>
          </div>
          <div>
            <dt>LIMITER</dt>
            <dd>{meters.limiterGainReductionDb.toFixed(1)} dB</dd>
          </div>
          <div>
            <dt>CLIP</dt>
            <dd>{clipLatched ? 'CLIP' : meters.hardClipSamples}</dd>
          </div>
          <div>
            <dt>FEEDBACK</dt>
            <dd>{meters.feedbackSuspected ? 'CHECK' : 'CLEAR'}</dd>
          </div>
        </dl>
      </section>
    </aside>
  );
}

function formatDb(value: number): string {
  if (value <= -90) return '−∞ dB';
  return `${value >= 0 ? '+' : ''}${value.toFixed(1)} dB`;
}

function formatMeterDb(value: number): string {
  if (value <= 0 || !Number.isFinite(value)) return '−∞';
  const db = 20 * Math.log10(value);
  return `${db >= 0 ? '+' : ''}${db.toFixed(1)}`;
}
