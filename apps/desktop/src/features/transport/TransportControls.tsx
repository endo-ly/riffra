import { useEffect, useState } from 'react';
import type { CreativeSession } from '@/model/domain';
import clsx from 'clsx';
import { TransportIcon } from './TransportIcon';
import type { TransportControlsApi } from './transport-api';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
import { toast } from '@/shared/toasts';
import styles from './TransportControls.module.css';

const TIME_SIGNATURES = ['2/4', '3/4', '4/4', '5/4', '6/8', '7/8', '9/8', '12/8'];

interface TransportControlsProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  recordingActive: boolean;
  transportPlaying: boolean;
  onPlay: () => void;
  onStop: () => void;
  onGoToStart: () => void;
  recordingCommandPending: boolean;
  onToggleRecording: () => void;
  api: TransportControlsApi;
}

export function TransportControls(props: TransportControlsProps) {
  const {
    session,
    setSession,
    recordingActive,
    transportPlaying,
    onPlay,
    onStop,
    onGoToStart,
    recordingCommandPending,
    onToggleRecording,
    api,
  } = props;
  const [tempoDraft, setTempoDraft] = useState(String(session.arrangement.timebase.bpm));
  const [signatureDraft, setSignatureDraft] = useState(
    `${session.arrangement.timebase.timeSignatureNumerator}/${session.arrangement.timebase.timeSignatureDenominator}`,
  );
  useEffect(() => {
    setTempoDraft(String(session.arrangement.timebase.bpm));
    setSignatureDraft(
      `${session.arrangement.timebase.timeSignatureNumerator}/${session.arrangement.timebase.timeSignatureDenominator}`,
    );
  }, [
    session.arrangement.timebase.bpm,
    session.arrangement.timebase.timeSignatureDenominator,
    session.arrangement.timebase.timeSignatureNumerator,
  ]);

  const commitTimebase = (nextSignature = signatureDraft) => {
    const bpm = Number(tempoDraft);
    const [numerator, denominator] = nextSignature.split('/').map(Number);
    if (
      !Number.isFinite(bpm) ||
      bpm < 20 ||
      bpm > 400 ||
      !Number.isInteger(numerator) ||
      numerator <= 0 ||
      !Number.isInteger(denominator) ||
      denominator <= 0
    ) {
      setTempoDraft(String(session.arrangement.timebase.bpm));
      setSignatureDraft(
        `${session.arrangement.timebase.timeSignatureNumerator}/${session.arrangement.timebase.timeSignatureDenominator}`,
      );
      return;
    }
    const current = session.arrangement.timebase;
    if (
      bpm === current.bpm &&
      numerator === current.timeSignatureNumerator &&
      denominator === current.timeSignatureDenominator
    )
      return;
    void api
      .updateArrangementTimebase({
        ...current,
        bpm,
        timeSignatureNumerator: numerator,
        timeSignatureDenominator: denominator,
      })
      .then((result) =>
        applyArrangementMutation(result, setSession, (message) =>
          toast(message, { kind: 'error' }),
        ),
      )
      .catch(() => {
        setTempoDraft(String(current.bpm));
        setSignatureDraft(`${current.timeSignatureNumerator}/${current.timeSignatureDenominator}`);
      });
  };

  const recordingControlLabel = recordingCommandPending
    ? 'Recording command pending'
    : recordingActive
      ? 'Stop recording'
      : 'Start recording';

  return (
    <div className={styles.transport}>
      <div className={styles.transportActions}>
        <div className={styles.transportGroup}>
          <button
            type="button"
            className={session.arrangement.loopRange.enabled ? styles.toggleActive : undefined}
            aria-pressed={session.arrangement.loopRange.enabled}
            aria-label="Toggle loop"
            title={session.arrangement.loopRange.enabled ? 'Disable loop' : 'Enable loop'}
            onClick={() => {
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
                .then((result) =>
                  applyArrangementMutation(result, setSession, (message) =>
                    toast(message, { kind: 'error' }),
                  ),
                );
            }}
          >
            <TransportIcon name="loop" />
          </button>
        </div>
        <div className={styles.transportGroup}>
          <button
            type="button"
            className={clsx(styles.playButton, transportPlaying && styles.playing)}
            aria-label={transportPlaying ? 'Stop playback' : 'Play'}
            title={transportPlaying ? 'Stop playback' : 'Play'}
            onClick={() => void (transportPlaying ? onStop() : onPlay())}
          >
            <TransportIcon name={transportPlaying ? 'stop' : 'play'} />
          </button>
          <button
            type="button"
            aria-label="Stop and go to start"
            title="Stop and go to start"
            onClick={() => void onGoToStart()}
          >
            <TransportIcon name="rewind" />
          </button>
          <button
            type="button"
            disabled={recordingCommandPending}
            className={clsx(styles.recordButton, recordingActive && styles.active)}
            aria-pressed={recordingActive}
            onClick={() => void onToggleRecording()}
            aria-label={recordingControlLabel}
            title={recordingControlLabel}
          >
            <TransportIcon name="record" />
          </button>
        </div>
        <div className={styles.transportGroup}>
          <button
            type="button"
            className={session.settings.metronomeEnabled ? styles.toggleActive : undefined}
            aria-pressed={session.settings.metronomeEnabled}
            aria-label="Toggle metronome"
            title={session.settings.metronomeEnabled ? 'Disable metronome' : 'Enable metronome'}
            onClick={() =>
              void api
                .updateSessionSettings({
                  metronomeEnabled: !session.settings.metronomeEnabled,
                })
                .then((result) =>
                  applyArrangementMutation(result, setSession, (message) =>
                    toast(message, { kind: 'error' }),
                  ),
                )
            }
          >
            <TransportIcon name="metronome" />
          </button>
          <button
            type="button"
            className={clsx(
              styles.countInButton,
              session.settings.countInBeats > 0 && styles.toggleActive,
            )}
            aria-pressed={session.settings.countInBeats > 0}
            aria-label={`Count-in: ${describeCountIn(session)}`}
            title={`Count-in: ${describeCountIn(session)}`}
            onClick={() =>
              void api
                .updateSessionSettings({ countInBeats: nextCountInBeats(session) })
                .then((result) =>
                  applyArrangementMutation(result, setSession, (message) =>
                    toast(message, { kind: 'error' }),
                  ),
                )
            }
          >
            Count-in: {describeCountIn(session)}
          </button>
        </div>
      </div>
      <div className={styles.timebase} aria-label="Project timebase">
        <label>
          <span>BPM</span>
          <input
            aria-label="Project BPM"
            title="Project BPM"
            type="number"
            min="20"
            max="400"
            step="0.1"
            value={tempoDraft}
            onChange={(event) => setTempoDraft(event.currentTarget.value)}
            onBlur={() => commitTimebase()}
            onKeyDown={(event) => {
              if (event.key === 'Enter') event.currentTarget.blur();
            }}
          />
        </label>
        <label>
          <span>METER</span>
          <select
            aria-label="Project time signature"
            title="Project time signature"
            value={signatureDraft}
            onChange={(event) => {
              setSignatureDraft(event.currentTarget.value);
              commitTimebase(event.currentTarget.value);
            }}
          >
            {TIME_SIGNATURES.map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </select>
        </label>
      </div>
    </div>
  );
}

function describeCountIn(session: CreativeSession): string {
  const beats = session.settings.countInBeats;
  if (!beats) return 'Off';
  const beatsPerBar = session.arrangement.timebase.timeSignatureNumerator;
  if (beats >= beatsPerBar * 2) return '2 Bars';
  if (beats >= beatsPerBar) return '1 Bar';
  return String(beats);
}

function nextCountInBeats(session: CreativeSession): number {
  const beatsPerBar = session.arrangement.timebase.timeSignatureNumerator;
  const current = session.settings.countInBeats;
  if (current === 0) return beatsPerBar;
  if (current < beatsPerBar * 2) return beatsPerBar * 2;
  return 0;
}
