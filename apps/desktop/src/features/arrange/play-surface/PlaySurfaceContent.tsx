import { useCallback, useEffect, useMemo, useState } from 'react';
import type { AudioStatus, Track } from '@/model/domain';
import type { AudioApi } from '@/native/native-api';
import { drumPadByNote } from '@/features/arrange/play-surface/drum-map';
import {
  MUSICAL_TYPING_DEFAULT_OCTAVE,
  MUSICAL_TYPING_DEFAULT_VELOCITY,
  MUSICAL_TYPING_MAX_OCTAVE,
  MUSICAL_TYPING_MIN_OCTAVE,
  midiNoteName,
} from '@/features/arrange/play-surface/musical-typing';
import { useDrumPads } from '@/features/arrange/play-surface/useDrumPads';
import { useMusicalTyping } from '@/features/arrange/play-surface/useMusicalTyping';
import { DrumPadGrid } from './DrumPadGrid';
import { MusicalTypingKeyboard } from './MusicalTypingKeyboard';
import styles from './PlaySurfaceContent.module.css';

type SurfaceMode = 'keys' | 'pads';

interface SurfaceState {
  mode: SurfaceMode;
  octave: number;
  velocity: number;
}

interface PlaySurfaceContentProps {
  track: Track | null;
  audio: AudioStatus;
  api: Pick<AudioApi, 'sendMidiToTrack'>;
  runtimeReady: boolean;
  missingDeviceIds: string[];
  onChooseInstrument: () => void;
  onSummaryChange: (summary: string) => void;
}

const DEFAULT_SURFACE_STATE: SurfaceState = {
  mode: 'keys',
  octave: MUSICAL_TYPING_DEFAULT_OCTAVE,
  velocity: MUSICAL_TYPING_DEFAULT_VELOCITY,
};

export function PlaySurfaceContent({
  track,
  audio,
  api,
  runtimeReady,
  missingDeviceIds,
  onChooseInstrument,
  onSummaryChange,
}: PlaySurfaceContentProps) {
  const [surfaceStates, setSurfaceStates] = useState<Record<string, SurfaceState>>({});
  const [computerKeys, setComputerKeys] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const currentState = track
    ? (surfaceStates[track.id] ?? DEFAULT_SURFACE_STATE)
    : DEFAULT_SURFACE_STATE;
  const targetTrackId = track?.kind === 'instrument' ? track.id : null;
  useEffect(() => {
    setComputerKeys(false);
  }, [targetTrackId]);
  const instrumentMissing = Boolean(
    track?.instrument?.source.type === 'vst3' &&
    (track.instrument.source.disabledPlaceholder || missingDeviceIds.includes(track.instrument.id)),
  );
  const instrumentReady = Boolean(
    track?.kind === 'instrument' && track.instrument && !instrumentMissing,
  );
  const canPlay = Boolean(runtimeReady && instrumentReady && targetTrackId);

  useEffect(() => {
    const trackLabel = track?.name ?? 'No instrument selected';
    const octaveLabel = midiNoteName(currentState.octave * 12 + 12);
    const modeLabel = currentState.mode === 'keys' ? `Keyboard ${octaveLabel}` : 'Drum pads';
    onSummaryChange(
      `${trackLabel} · ${modeLabel} · Computer keyboard ${computerKeys ? 'on' : 'off'}`,
    );
  }, [
    computerKeys,
    currentState.mode,
    currentState.octave,
    onSummaryChange,
    track?.id,
    track?.name,
  ]);

  const updateState = useCallback(
    (patch: Partial<SurfaceState>) => {
      if (!track) return;
      setSurfaceStates((current) => ({
        ...current,
        [track.id]: { ...(current[track.id] ?? DEFAULT_SURFACE_STATE), ...patch },
      }));
    },
    [track],
  );

  const sendMidi = useCallback(
    async (trackId: string, bytes: number[]) => {
      const status = await api.sendMidiToTrack(trackId, bytes);
      if (status) setMessage(status.message);
    },
    [api],
  );

  const melodic = useMusicalTyping({
    enabled: canPlay && computerKeys && currentState.mode === 'keys',
    targetTrackId,
    octave: currentState.octave,
    velocity: currentState.velocity,
    sendMidi,
    onOctaveChange: (delta) =>
      updateState({
        octave: Math.max(
          MUSICAL_TYPING_MIN_OCTAVE,
          Math.min(MUSICAL_TYPING_MAX_OCTAVE, currentState.octave + delta),
        ),
      }),
  });
  const drums = useDrumPads({
    enabled: canPlay && computerKeys && currentState.mode === 'pads',
    targetTrackId,
    velocity: currentState.velocity,
    sendMidi,
  });

  const activeNotes = currentState.mode === 'pads' ? drums.activeNotes : melodic.activeNotes;
  const activeSummary = useMemo(() => {
    if (currentState.mode === 'pads') {
      return Array.from(activeNotes)
        .sort((a, b) => a - b)
        .map((note) => drumPadByNote(note)?.shortName ?? midiNoteName(note))
        .join(' ');
    }
    return Array.from(activeNotes)
      .sort((a, b) => a - b)
      .map((note) => midiNoteName(note))
      .join(' ');
  }, [activeNotes, currentState.mode]);

  if (!track) {
    return <div className={styles.state}>Select an Instrument Track to use its Play Surface.</div>;
  }
  if (track.kind !== 'instrument') {
    return (
      <div className={styles.state}>Play Surface is available only for Instrument Tracks.</div>
    );
  }
  if (!track.instrument) {
    return (
      <div className={styles.state}>
        <strong>No instrument assigned.</strong>
        <span>Choose an instrument from the Library or Track menu.</span>
        <button type="button" onClick={onChooseInstrument}>
          Choose Instrument
        </button>
      </div>
    );
  }
  if (instrumentMissing) {
    return (
      <div className={styles.state}>
        <strong>Instrument unavailable.</strong>
        <span>The assigned plugin is missing or disabled.</span>
        <button type="button" onClick={onChooseInstrument}>
          Replace Instrument
        </button>
      </div>
    );
  }
  if (!runtimeReady) {
    return (
      <div className={styles.state}>
        <strong>Audio runtime unavailable.</strong>
        <span>{audio.message || 'Wait for the Arrange runtime to become ready.'}</span>
      </div>
    );
  }

  const inputLabel = computerKeys
    ? currentState.mode === 'keys'
      ? 'On-screen keyboard + computer keyboard'
      : 'On-screen pads + computer keyboard'
    : currentState.mode === 'keys'
      ? 'On-screen keyboard'
      : 'On-screen pads';

  return (
    <div className={styles.surface}>
      <div className={styles.instrumentInfo}>
        <strong>{track.instrument.name}</strong>
        <span>
          {track.armed && audio.recording.active
            ? `Recording MIDI to ${track.name}`
            : 'Live input only'}
        </span>
      </div>
      <div className={styles.controls}>
        <div className={styles.mode} role="tablist" aria-label="Play Surface mode">
          <button
            type="button"
            role="tab"
            aria-selected={currentState.mode === 'keys'}
            className={currentState.mode === 'keys' ? styles.active : undefined}
            onClick={() => {
              melodic.releaseHeldNotes();
              drums.releaseAll?.();
              updateState({ mode: 'keys' });
            }}
          >
            Keyboard
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={currentState.mode === 'pads'}
            className={currentState.mode === 'pads' ? styles.active : undefined}
            onClick={() => {
              melodic.releaseHeldNotes();
              drums.releaseAll?.();
              updateState({ mode: 'pads' });
            }}
          >
            Drum Pads
          </button>
        </div>
        <label className={styles.velocity}>
          Velocity <strong>{currentState.velocity}</strong>
          <input
            type="range"
            min="1"
            max="127"
            value={currentState.velocity}
            onChange={(event) => updateState({ velocity: Number(event.target.value) })}
          />
        </label>
        {currentState.mode === 'keys' && (
          <div className={styles.octave}>
            <button
              type="button"
              aria-label="Octave down"
              disabled={currentState.octave <= MUSICAL_TYPING_MIN_OCTAVE}
              onClick={() => updateState({ octave: currentState.octave - 1 })}
            >
              −
            </button>
            <strong>{midiNoteName(currentState.octave * 12 + 12)}</strong>
            <button
              type="button"
              aria-label="Octave up"
              disabled={currentState.octave >= MUSICAL_TYPING_MAX_OCTAVE}
              onClick={() => updateState({ octave: currentState.octave + 1 })}
            >
              +
            </button>
          </div>
        )}
        <button
          type="button"
          className={`${styles.toggleButton}${computerKeys ? ` ${styles.computerKeysActive}` : ''}`}
          aria-pressed={computerKeys}
          onClick={() => setComputerKeys((value) => !value)}
        >
          Computer Keyboard: {computerKeys ? 'On' : 'Off'}
        </button>
      </div>
      <div className={styles.sourceLine}>
        <span>
          <strong>Input</strong> {inputLabel}
        </span>
        {track.armed && audio.recording.active && <span>Recording enabled</span>}
        {activeNotes.size > 0 && (
          <span className={styles.activeNotes}>Playing: {activeSummary}</span>
        )}
      </div>
      <div className={styles.instrumentSurface}>
        {currentState.mode === 'keys' ? (
          <MusicalTypingKeyboard
            octave={currentState.octave}
            activeNotes={melodic.activeNotes}
            onNoteDown={canPlay ? melodic.triggerNoteDown : undefined}
            onNoteUp={canPlay ? melodic.triggerNoteUp : undefined}
          />
        ) : (
          <DrumPadGrid
            activeNotes={drums.activeNotes}
            onPadDown={canPlay ? drums.triggerPadDown : () => undefined}
            onPadUp={canPlay ? drums.triggerPadUp : () => undefined}
          />
        )}
      </div>
      {message && (
        <small className={styles.message} role="status">
          {message}
        </small>
      )}
    </div>
  );
}
