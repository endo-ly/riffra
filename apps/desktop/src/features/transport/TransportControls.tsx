import { useEffect, useState } from 'react';
import type { CanonicalState, CreativeSession } from '@/model/domain';
import clsx from 'clsx';
import { formatClock, formatMusicalPosition } from '@/features/arrange/model/arrange-timeline';
import { TransportIcon } from './TransportIcon';
import type { TransportControlsApi } from './transport-api';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
import { tickToMusicalPosition } from '@/shared/session/musical-position';
import { toast } from '@/shared/toasts';
import styles from './TransportControls.module.css';

const TIME_SIGNATURES = ['2/4', '3/4', '4/4', '5/4', '6/8', '7/8', '9/8', '12/8'];

interface TransportControlsProps {
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  recordingActive: boolean;
  transportPlaying: boolean;
  transportStarting?: boolean;
  onPlay: () => void;
  onStop: () => void;
  onGoToStart: () => void;
  recordingCommandPending: boolean;
  onToggleRecording: () => void;
  /** Arrangement Transport playhead shown as the current position. */
  positionTick: number;
  api: TransportControlsApi;
}

export function TransportControls(props: TransportControlsProps) {
  const {
    session,
    applyCanonicalState,
    recordingActive,
    transportPlaying,
    transportStarting = false,
    onPlay,
    onStop,
    onGoToStart,
    recordingCommandPending,
    onToggleRecording,
    positionTick,
    api,
  } = props;
  const initialBpm = session.arrangement.timebase.tempoChanges[0].bpm;
  const initialNumerator = session.arrangement.timebase.timeSignatureChanges[0].numerator;
  const initialDenominator = session.arrangement.timebase.timeSignatureChanges[0].denominator;
  const [tempoDraft, setTempoDraft] = useState(String(initialBpm));
  const [signatureDraft, setSignatureDraft] = useState(`${initialNumerator}/${initialDenominator}`);
  useEffect(() => {
    setTempoDraft(String(initialBpm));
    setSignatureDraft(`${initialNumerator}/${initialDenominator}`);
  }, [initialBpm, initialNumerator, initialDenominator]);

  const commitTimebase = (nextSignature = signatureDraft) => {
    const bpm = Number(tempoDraft);
    const [numerator, denominator] = nextSignature.split('/').map(Number);
    if (
      !Number.isFinite(bpm) ||
      bpm <= 0 ||
      !Number.isInteger(numerator) ||
      numerator <= 0 ||
      !Number.isInteger(denominator) ||
      denominator <= 0
    ) {
      setTempoDraft(String(session.arrangement.timebase.tempoChanges[0].bpm));
      setSignatureDraft(
        `${session.arrangement.timebase.timeSignatureChanges[0].numerator}/${session.arrangement.timebase.timeSignatureChanges[0].denominator}`,
      );
      return;
    }
    const current = session.arrangement.timebase;
    if (
      bpm === current.tempoChanges[0].bpm &&
      numerator === current.timeSignatureChanges[0].numerator &&
      denominator === current.timeSignatureChanges[0].denominator
    )
      return;
    void api
      .updateArrangementTimebase({
        ...current,
        tempoChanges: [{ tick: 0, bpm }, ...current.tempoChanges.slice(1)],
        timeSignatureChanges: [
          { tick: 0, numerator, denominator },
          ...current.timeSignatureChanges.slice(1),
        ],
      })
      .then((result) =>
        applyArrangementMutation(result, applyCanonicalState, (message) =>
          toast(message, { kind: 'error' }),
        ),
      )
      .catch(() => {
        setTempoDraft(String(current.tempoChanges[0].bpm));
        setSignatureDraft(
          `${current.timeSignatureChanges[0].numerator}/${current.timeSignatureChanges[0].denominator}`,
        );
      });
  };

  const recordingControlLabel = recordingCommandPending
    ? 'Recording command pending'
    : recordingActive
      ? 'Stop recording'
      : 'Start recording';
  const transportActive = transportPlaying || transportStarting;

  return (
    <div className={styles.transport}>
      <div className={styles.transportGroup}>
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
          className={clsx(styles.playButton, transportActive && styles.playing)}
          aria-label={transportActive ? 'Stop playback' : 'Play'}
          title={transportActive ? 'Stop playback' : 'Play'}
          onClick={() => void (transportActive ? onStop() : onPlay())}
        >
          <TransportIcon name={transportActive ? 'stop' : 'play'} />
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
      <output className={styles.position} aria-label="Playhead position">
        <strong>{formatMusicalPosition(positionTick, session.arrangement.timebase)}</strong>
        <small>{formatClock(positionTick, session.arrangement.timebase)}</small>
      </output>
      <div className={styles.timebase} aria-label="Project timebase">
        <label className={styles.tempo}>
          <input
            aria-label="Project BPM"
            title="Project BPM"
            type="number"
            min="0"
            step="0.1"
            value={tempoDraft}
            onChange={(event) => setTempoDraft(event.currentTarget.value)}
            onBlur={() => commitTimebase()}
            onKeyDown={(event) => {
              if (event.key === 'Enter') event.currentTarget.blur();
            }}
          />
          <span>BPM</span>
        </label>
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
      </div>
      <div className={styles.transportGroup}>
        <button
          type="button"
          className={session.arrangement.loopRange.enabled ? styles.toggleActive : undefined}
          aria-pressed={session.arrangement.loopRange.enabled}
          aria-label="Toggle loop"
          title={session.arrangement.loopRange.enabled ? 'Disable loop' : 'Enable loop'}
          onClick={() => {
            const range = session.arrangement.loopRange;
            const timebase = session.arrangement.timebase;
            const barTicks =
              (session.arrangement.timebase.ppq *
                4 *
                session.arrangement.timebase.timeSignatureChanges[0].numerator) /
              session.arrangement.timebase.timeSignatureChanges[0].denominator;
            void api
              .updateTimelineLoopRange(
                !range.enabled,
                tickToMusicalPosition(range.startTick, timebase),
                tickToMusicalPosition(
                  range.endTick > range.startTick ? range.endTick : barTicks * 4,
                  timebase,
                ),
              )
              .then((result) =>
                applyArrangementMutation(result, applyCanonicalState, (message) =>
                  toast(message, { kind: 'error' }),
                ),
              )
              .catch(() => undefined);
          }}
        >
          <TransportIcon name="loop" />
        </button>
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
                applyArrangementMutation(result, applyCanonicalState, (message) =>
                  toast(message, { kind: 'error' }),
                ),
              )
              .catch(() => undefined)
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
                applyArrangementMutation(result, applyCanonicalState, (message) =>
                  toast(message, { kind: 'error' }),
                ),
              )
              .catch(() => undefined)
          }
        >
          <TransportIcon name="countIn" />
          {session.settings.countInBeats > 0 && (
            <span className={styles.countInBadge} aria-hidden="true">
              {countInBadge(session)}
            </span>
          )}
        </button>
      </div>
    </div>
  );
}

function describeCountIn(session: CreativeSession): string {
  const beats = session.settings.countInBeats;
  if (!beats) return 'Off';
  const beatsPerBar = session.arrangement.timebase.timeSignatureChanges[0].numerator;
  if (beats >= beatsPerBar * 2) return '2 Bars';
  if (beats >= beatsPerBar) return '1 Bar';
  return String(beats);
}

function countInBadge(session: CreativeSession): string {
  const beats = session.settings.countInBeats;
  const beatsPerBar = session.arrangement.timebase.timeSignatureChanges[0].numerator;
  return beats % beatsPerBar === 0 ? String(beats / beatsPerBar) : `${beats}b`;
}

function nextCountInBeats(session: CreativeSession): number {
  const beatsPerBar = session.arrangement.timebase.timeSignatureChanges[0].numerator;
  const current = session.settings.countInBeats;
  if (current === 0) return beatsPerBar;
  if (current < beatsPerBar * 2) return beatsPerBar * 2;
  return 0;
}
