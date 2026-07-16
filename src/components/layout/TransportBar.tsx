import type { AudioStatus, Session } from '@/lib/domain';
import clsx from 'clsx';
import { DEFAULT_TEMPO_BPM } from '@/constants';
import type { NativeApi } from '@/native/native-api';
import { Icon, Meter } from '../shared/ui';
import styles from './TransportBar.module.css';

interface TransportBarProps {
  session: Session;
  setSession: (session: Session) => void;
  audio: AudioStatus;
  setAudio: (audio: AudioStatus) => void;
  transportPlaying: boolean;
  onPlay: () => void;
  onStop: () => void;
  recordingCommandPending: boolean;
  onToggleRecording: () => void;
  recordCountdown: number | null;
  autosaveError: string | null;
  audioPreferenceMessage: string | null;
  api: NativeApi;
}

export function TransportBar(props: TransportBarProps) {
  const {
    session,
    setSession,
    audio,
    setAudio,
    transportPlaying,
    onPlay,
    onStop,
    recordingCommandPending,
    onToggleRecording,
    recordCountdown,
    autosaveError,
    audioPreferenceMessage,
    api,
  } = props;
  const statusDotState =
    audio.recording.active || recordCountdown !== null ? 'recording' : audio.state;
  return (
    <footer className="transport">
      <div className={styles.transportLeft}>
        <button
          className={session.loopEnabled ? 'active' : ''}
          aria-label="Toggle loop"
          onClick={() => setSession({ ...session, loopEnabled: !session.loopEnabled })}
        >
          <Icon name="loop" />
        </button>
        <button aria-label="Previous position">◀</button>
        <button
          className={styles.playButton}
          aria-label={transportPlaying ? 'Stop playback' : 'Play'}
          onClick={() => void (transportPlaying ? onStop() : onPlay())}
        >
          <Icon name={transportPlaying ? 'stop' : 'play'} />
        </button>
        <button aria-label="Stop" onClick={() => void onStop()}>
          <Icon name="stop" />
        </button>
        <button
          disabled={recordingCommandPending}
          className={clsx(styles.recordButton, audio.recording.active && styles.active)}
          onClick={() => void onToggleRecording()}
          aria-label={
            recordingCommandPending
              ? 'Recording command pending'
              : audio.recording.active
                ? 'Stop recording'
                : 'Start recording'
          }
        >
          <Icon name="record" />
        </button>
      </div>
      <div className={styles.position}>
        <strong>001 · 01 · 000</strong>
        <small>00:00:00.000</small>
      </div>
      <div className={styles.tempo}>
        <button>
          <strong>{DEFAULT_TEMPO_BPM.toFixed(2)}</strong>
          <small>BPM</small>
        </button>
        <button>
          <strong>4 / 4</strong>
          <small>TIME</small>
        </button>
      </div>
      <div className={styles.transportMeter}>
        <span>IN</span>
        <Meter value={audio.inputPeak * 100} danger={audio.inputPeak >= 0.98} />
        <span>OUT</span>
        <Meter value={audio.outputPeak * 100} danger={audio.outputPeak >= 0.98} />
      </div>
      <div className={styles.master}>
        <span>MASTER</span>
        <strong>{session.masterDb.toFixed(1)} dB</strong>
        <input
          aria-label="Master volume"
          type="range"
          min="-60"
          max="0"
          step="0.5"
          value={session.masterDb}
          onChange={(event) => {
            const gainDb = Number(event.target.value);
            setSession({ ...session, masterDb: gainDb });
            void api.setMasterGainDb(gainDb).then(setAudio);
          }}
        />
      </div>
      <div className="status-line">
        <span className={clsx(styles.statusDot, styles[statusDotState])} />
        {recordCountdown !== null
          ? `Count-in · ${recordCountdown} beats`
          : audio.recording.active
            ? `Recording · ${audio.recording.samplesWritten.toLocaleString()} samples`
            : (autosaveError ?? audioPreferenceMessage ?? audio.message)}
      </div>
    </footer>
  );
}
