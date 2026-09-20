import type { AudioStatus, CanonicalState, CreativeSession } from '@/model/domain';
import { useAudioMeters } from '@/shared/audio/audio-meters';
import { Meter } from '@/shared/ui/primitives';
import type { AudioMonitorApi } from './audio-api';
import { useMasterGainControl } from './hooks/useMasterGainControl';
import styles from './AudioMonitor.module.css';

interface AudioMonitorProps {
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  setAudio: (audio: AudioStatus) => void;
  api: AudioMonitorApi;
  disabled?: boolean;
}

export function AudioMonitor(props: AudioMonitorProps) {
  const { session, applyCanonicalState, setAudio, api } = props;
  const meters = useAudioMeters();
  const {
    draftDb: masterDraftDb,
    setDraftDb: setMasterDraftDb,
    beginEditing,
    preview,
    commit,
  } = useMasterGainControl({
    session,
    applyCanonicalState,
    setAudio,
    api,
    disabled: props.disabled,
  });

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
          disabled={props.disabled}
          type="range"
          min="-90"
          max="0"
          step="0.5"
          value={masterDraftDb}
          onPointerDown={beginEditing}
          onPointerUp={(event) => void commit(Number(event.currentTarget.value))}
          onBlur={(event) => void commit(Number(event.currentTarget.value))}
          onKeyUp={(event) => {
            if (
              ['ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown'].includes(event.key)
            )
              void commit(Number(event.currentTarget.value));
          }}
          onChange={(event) => {
            const gainDb = Number(event.target.value);
            setMasterDraftDb(gainDb);
            preview(gainDb);
          }}
        />
      </label>
    </div>
  );
}
