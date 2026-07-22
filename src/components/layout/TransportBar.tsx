import { useEffect, useRef, useState } from 'react';
import type { AudioStatus, CreativeSession } from '@/lib/domain';
import clsx from 'clsx';
import type { NativeApi } from '@/native/native-api';
import { Icon, Meter } from '../shared/ui';
import styles from './TransportBar.module.css';

interface TransportBarProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  audio: AudioStatus;
  setAudio: (audio: AudioStatus) => void;
  transportPlaying: boolean;
  onPlay: () => void;
  onStop: () => void;
  onGoToStart: () => void;
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
    onGoToStart,
    recordingCommandPending,
    onToggleRecording,
    recordCountdown,
    autosaveError,
    audioPreferenceMessage,
    api,
  } = props;
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
        .then(async () => setAudio(await api.previewMasterGainDb(gainDb)));
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
      setSession(result.session);
      setAudio(result.audio);
    } catch {
      lastCommittedMasterDb.current = session.settings.masterDb;
      setMasterDraftDb(session.settings.masterDb);
    }
  };

  const statusDotState =
    audio.recording.active || recordCountdown !== null ? 'recording' : audio.state;
  return (
    <footer className="transport">
      <div className={styles.transportLeft}>
        <button
          className={
            (
              session.workspace === 'arrange'
                ? session.arrangement.loopRange.enabled
                : session.settings.loopEnabled
            )
              ? 'active'
              : ''
          }
          aria-label="Toggle loop"
          onClick={() => {
            if (session.workspace === 'arrange') {
              const range = session.arrangement.loopRange;
              const barTicks =
                (session.arrangement.timebase.ppq *
                  4 *
                  session.arrangement.timebase.timeSignatureNumerator) /
                session.arrangement.timebase.timeSignatureDenominator;
              void api
                .updateTimelineLoopRange(
                  !range.enabled,
                  range.startTick,
                  range.endTick > range.startTick ? range.endTick : barTicks * 4,
                )
                .then(setSession);
            } else {
              void api
                .updateSessionSettings({ loopEnabled: !session.settings.loopEnabled })
                .then(setSession);
            }
          }}
        >
          <Icon name="loop" />
        </button>
        <button
          className={styles.playButton}
          aria-label={transportPlaying ? 'Stop playback' : 'Play'}
          onClick={() => void (transportPlaying ? onStop() : onPlay())}
        >
          <Icon name={transportPlaying ? 'stop' : 'play'} />
        </button>
        <button aria-label="Stop and go to start" onClick={() => void onGoToStart()}>
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
        <button
          className={session.settings.metronomeEnabled ? 'active' : ''}
          aria-pressed={session.settings.metronomeEnabled}
          aria-label="Toggle metronome"
          title="Metronome"
          onClick={() =>
            void api
              .updateSessionSettings({
                metronomeEnabled: !session.settings.metronomeEnabled,
              })
              .then(setSession)
          }
        >
          <Icon name="metronome" />
        </button>
        <button
          className={session.settings.countInBeats > 0 ? 'active' : ''}
          aria-label={`Count-in: ${describeCountIn(session)}`}
          title={`Count-in: ${describeCountIn(session)}`}
          onClick={() =>
            void api
              .updateSessionSettings({ countInBeats: nextCountInBeats(session) })
              .then(setSession)
          }
        >
          {describeCountIn(session)}
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

function describeCountIn(session: CreativeSession): string {
  const beats = session.settings.countInBeats;
  if (!beats) return 'Count-in: Off';
  const beatsPerBar =
    session.workspace === 'arrange' ? session.arrangement.timebase.timeSignatureNumerator : 4;
  if (beats >= beatsPerBar * 2) return 'Count-in: 2 Bars';
  if (beats >= beatsPerBar) return 'Count-in: 1 Bar';
  return `Count-in: ${beats}`;
}

function nextCountInBeats(session: CreativeSession): number {
  const beatsPerBar =
    session.workspace === 'arrange' ? session.arrangement.timebase.timeSignatureNumerator : 4;
  const current = session.settings.countInBeats;
  if (current === 0) return beatsPerBar;
  if (current < beatsPerBar * 2) return beatsPerBar * 2;
  return 0;
}
