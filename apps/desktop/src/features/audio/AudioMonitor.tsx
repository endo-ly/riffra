import { useEffect, useRef, useState } from 'react';
import type { AudioStatus, CanonicalState, CreativeSession } from '@/model/domain';
import { useAudioMeters } from '@/shared/audio/audio-meters';
import { Meter } from '@/shared/ui/primitives';
import type { AudioMonitorApi } from './audio-api';
import styles from './AudioMonitor.module.css';

interface AudioMonitorProps {
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  setAudio: (audio: AudioStatus) => void;
  api: AudioMonitorApi;
}

export function AudioMonitor(props: AudioMonitorProps) {
  const { session, applyCanonicalState, setAudio, api } = props;
  const meters = useAudioMeters();
  const [masterDraftDb, setMasterDraftDb] = useState(session.settings.masterDb);
  const masterEditing = useRef(false);
  const previewTimer = useRef<number | null>(null);
  const previewChain = useRef<Promise<void>>(Promise.resolve());
  const lastCommittedMasterDb = useRef(session.settings.masterDb);

  useEffect(() => {
    lastCommittedMasterDb.current = session.settings.masterDb;
    if (!masterEditing.current) setMasterDraftDb(session.settings.masterDb);
  }, [session.settings.masterDb]);

  useEffect(
    () => () => {
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
    },
    [],
  );

  const previewMaster = (gainDb: number) => {
    if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
    previewTimer.current = window.setTimeout(() => {
      previewTimer.current = null;
      previewChain.current = previewChain.current
        .catch(() => undefined)
        .then(() => api.previewMasterGainDb(gainDb));
    }, 40);
  };

  const commitMaster = async (gainDb: number) => {
    if (previewTimer.current !== null) {
      window.clearTimeout(previewTimer.current);
      previewTimer.current = null;
    }
    await previewChain.current.catch(() => undefined);
    if (gainDb === lastCommittedMasterDb.current) return;
    lastCommittedMasterDb.current = gainDb;
    try {
      const result = await api.setMasterGainDb(gainDb);
      applyCanonicalState(result.canonical);
      setAudio(result.audio);
    } catch {
      lastCommittedMasterDb.current = session.settings.masterDb;
      setMasterDraftDb(session.settings.masterDb);
    }
  };

  return (
    <div className={styles.monitor} data-audio-monitor aria-label="Audio monitor">
      <div className={styles.levelMeter} aria-label="Input and output levels">
        <span>IN</span>
        <Meter
          value={meters.inputPeak * 100}
          danger={meters.inputPeak >= 0.98}
          className={styles.meter}
        />
        <span>OUT</span>
        <Meter
          value={meters.outputPeak * 100}
          danger={meters.outputPeak >= 0.98}
          className={styles.meter}
        />
      </div>
      <label className={styles.master}>
        <span>MASTER</span>
        <strong>{masterDraftDb.toFixed(1)} dB</strong>
        <input
          aria-label="Master volume"
          type="range"
          min="-60"
          max="0"
          step="0.5"
          value={masterDraftDb}
          onPointerDown={() => {
            masterEditing.current = true;
          }}
          onPointerUp={(event) => {
            masterEditing.current = false;
            void commitMaster(Number(event.currentTarget.value));
          }}
          onBlur={(event) => {
            masterEditing.current = false;
            void commitMaster(Number(event.currentTarget.value));
          }}
          onKeyUp={(event) => {
            if (
              ['ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown'].includes(event.key)
            )
              void commitMaster(Number(event.currentTarget.value));
          }}
          onChange={(event) => {
            const gainDb = Number(event.target.value);
            setMasterDraftDb(gainDb);
            previewMaster(gainDb);
          }}
        />
      </label>
    </div>
  );
}
