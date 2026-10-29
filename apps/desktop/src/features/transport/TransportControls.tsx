import { useEffect, useState } from 'react';
import type { CreativeSession, MetronomeSound } from '@/model/generated';
import clsx from 'clsx';
import { formatClock, formatMusicalPosition } from '@/model/time-format';
import { TransportIcon } from './TransportIcon';
import type { SessionSettingsPatch } from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { tickToMusicalPosition } from '@/model/time-format';
import { ContextMenu } from '@/shared/ui/ContextMenu';
import styles from './TransportControls.module.css';

const TIME_SIGNATURES = ['2/4', '3/4', '4/4', '5/4', '6/8', '7/8', '9/8', '12/8'];
const METRONOME_SOUNDS: { sound: MetronomeSound; label: string }[] = [
  { sound: 'beep', label: 'Beep' },
  { sound: 'wood', label: 'Wood' },
];

interface TransportControlsProps {
  session: CreativeSession;
  recordingActive: boolean;
  transportPlaying: boolean;
  transportStarting?: boolean;
  transportPaused?: boolean;
  onPlay: () => void;
  onPause: () => void;
  onStop: () => void;
  onGoToStart: () => void;
  recordingCommandPending: boolean;
  onToggleRecording: () => void;
  /** Arrangement Transport playhead shown as the current position. */
  positionTick: number;
}

export function TransportControls(props: TransportControlsProps) {
  const {
    session,
    recordingActive,
    transportPlaying,
    transportStarting = false,
    transportPaused = false,
    onPlay,
    onPause,
    onStop,
    onGoToStart,
    recordingCommandPending,
    onToggleRecording,
    positionTick,
  } = props;
  const { api, commit } = useSessionContext();
  const initialBpm = session.arrangement.timebase.tempoChanges[0].bpm;
  const initialNumerator = session.arrangement.timebase.timeSignatureChanges[0].numerator;
  const initialDenominator = session.arrangement.timebase.timeSignatureChanges[0].denominator;
  const [tempoDraft, setTempoDraft] = useState(String(initialBpm));
  const [signatureDraft, setSignatureDraft] = useState(`${initialNumerator}/${initialDenominator}`);
  const [metronomeMenu, setMetronomeMenu] = useState<{ x: number; y: number } | null>(null);
  useEffect(() => {
    setTempoDraft(String(initialBpm));
    setSignatureDraft(`${initialNumerator}/${initialDenominator}`);
  }, [initialBpm, initialNumerator, initialDenominator]);

  const updateSettings = (patch: SessionSettingsPatch) =>
    void commit(api.updateSessionSettings(patch));

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
    void commit(
      api.updateArrangementTimebase({
        ...current,
        tempoChanges: [{ tick: 0, bpm }, ...current.tempoChanges.slice(1)],
        timeSignatureChanges: [
          { tick: 0, numerator, denominator },
          ...current.timeSignatureChanges.slice(1),
        ],
      }),
    ).then((result) => {
      if (result) return;
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
  const transportActive = transportPlaying || transportStarting || transportPaused;

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
          aria-label="Stop playback"
          title="Stop playback"
          disabled={!transportActive}
          onClick={onStop}
        >
          <TransportIcon name="stop" />
        </button>
        <button
          type="button"
          className={clsx(styles.playButton, transportPlaying && styles.playing)}
          aria-label={
            transportPlaying ? 'Pause playback' : transportPaused ? 'Resume playback' : 'Play'
          }
          title={transportPlaying ? 'Pause playback' : transportPaused ? 'Resume playback' : 'Play'}
          onClick={() => void (transportPlaying ? onPause() : onPlay())}
          disabled={transportStarting}
        >
          <TransportIcon name={transportPlaying ? 'pause' : 'play'} />
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
            void commit(
              api.updateTimelineLoopRange(
                !range.enabled,
                tickToMusicalPosition(range.startTick, timebase),
                tickToMusicalPosition(
                  range.endTick > range.startTick ? range.endTick : barTicks * 4,
                  timebase,
                ),
              ),
            );
          }}
        >
          <TransportIcon name="loop" />
        </button>
        <button
          type="button"
          className={session.settings.metronomeEnabled ? styles.toggleActive : undefined}
          aria-pressed={session.settings.metronomeEnabled}
          aria-label="Toggle metronome"
          title={`${session.settings.metronomeEnabled ? 'Disable' : 'Enable'} metronome (right-click for sound)`}
          onClick={() => updateSettings({ metronomeEnabled: !session.settings.metronomeEnabled })}
          onContextMenu={(event) => {
            event.preventDefault();
            setMetronomeMenu({ x: event.clientX, y: event.clientY });
          }}
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
          onClick={() => updateSettings({ countInBeats: nextCountInBeats(session) })}
        >
          <TransportIcon name="countIn" />
          {session.settings.countInBeats > 0 && (
            <span className={styles.countInBadge} aria-hidden="true">
              {countInBadge(session)}
            </span>
          )}
        </button>
      </div>
      {metronomeMenu && (
        <ContextMenu
          x={metronomeMenu.x}
          y={metronomeMenu.y}
          items={METRONOME_SOUNDS.map(({ sound, label }) => ({
            label,
            checked: session.settings.metronomeSound === sound,
            onClick: () => updateSettings({ metronomeSound: sound }),
          }))}
          onClose={() => setMetronomeMenu(null)}
        />
      )}
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
