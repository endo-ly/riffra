import type { CSSProperties } from 'react';
import type { AudioStatus } from '@/model/domain';
import { drumPadByNote } from './drum-map';
import {
  MUSICAL_TYPING_MAX_OCTAVE,
  MUSICAL_TYPING_MIN_OCTAVE,
  midiNoteName,
} from './musical-typing';
import type { LivePlay } from './useLivePlay';
import {
  Toolbar,
  ToolbarButton,
  ToolbarDivider,
  ToolbarSegmented,
  ToolbarSlider,
} from '@/shared/ui/Toolbar';
import { DrumPadGrid } from './DrumPadGrid';
import { MusicalTypingKeyboard } from './MusicalTypingKeyboard';
import styles from './PlaySurface.module.css';

interface PlaySurfaceProps {
  play: LivePlay;
  trackColor: string | null;
  audio: AudioStatus;
  onChooseInstrument: () => void;
}

/** The on-screen keyboard and drum pads for the Focused Instrument Track. */
export function PlaySurface({ play, trackColor, audio, onChooseInstrument }: PlaySurfaceProps) {
  const { track, settings } = play;
  if (!track || play.availability === 'noTrack') {
    return <div className={styles.state}>Select an Instrument Track to play it here.</div>;
  }
  if (play.availability === 'notInstrument') {
    return <div className={styles.state}>Only Instrument Tracks can be played live.</div>;
  }
  if (play.availability === 'noInstrument' || play.availability === 'instrumentMissing') {
    const missing = play.availability === 'instrumentMissing';
    return (
      <div className={styles.state}>
        <strong>{missing ? 'Instrument unavailable.' : 'No instrument assigned.'}</strong>
        <span>
          {missing
            ? 'The assigned plugin is missing or disabled.'
            : 'Choose an instrument from the Library or Track menu.'}
        </span>
        <button type="button" onClick={onChooseInstrument}>
          {missing ? 'Replace Instrument' : 'Choose Instrument'}
        </button>
      </div>
    );
  }
  if (play.availability === 'runtimeUnavailable') {
    return (
      <div className={styles.state}>
        <strong>Audio runtime unavailable.</strong>
        <span>{audio.message || 'Wait for the Arrange runtime to become ready.'}</span>
      </div>
    );
  }

  const playing = [...play.activeNotes]
    .sort((a, b) => a - b)
    .map((note) =>
      settings.mode === 'pads'
        ? (drumPadByNote(note)?.shortName ?? midiNoteName(note))
        : midiNoteName(note),
    )
    .join(' ');
  const recording = track.armed && audio.recording.active;

  return (
    <div
      className={styles.surface}
      style={trackColor ? ({ '--track-color': trackColor } as CSSProperties) : undefined}
      aria-label="Play Surface"
      role="region"
    >
      <Toolbar label="Play Surface toolbar">
        <span className={styles.identity} title={`${track.name} · ${track.instrument?.name}`}>
          {track.name}
          <small>{track.instrument?.name}</small>
        </span>
        <ToolbarDivider />
        <ToolbarSegmented
          label="Play Surface mode"
          value={settings.mode}
          onChange={play.setMode}
          options={[
            { value: 'keys', label: 'Keyboard' },
            { value: 'pads', label: 'Drum Pads' },
          ]}
        />
        {settings.mode === 'keys' && (
          <div className={styles.octave} role="group" aria-label="Octave">
            <ToolbarButton
              icon="zoomOut"
              ariaLabel="Octave down"
              disabled={settings.octave <= MUSICAL_TYPING_MIN_OCTAVE}
              onClick={() => play.shiftOctave(-1)}
            />
            <strong>{midiNoteName(settings.octave * 12 + 12)}</strong>
            <ToolbarButton
              icon="zoomIn"
              ariaLabel="Octave up"
              disabled={settings.octave >= MUSICAL_TYPING_MAX_OCTAVE}
              onClick={() => play.shiftOctave(1)}
            />
          </div>
        )}
        <ToolbarSlider
          label="Vel"
          ariaLabel="Play Surface velocity"
          value={settings.velocity}
          min={1}
          max={127}
          onChange={play.setVelocity}
          onCommit={play.setVelocity}
        />
        <ToolbarDivider />
        <span className={styles.status}>
          {recording
            ? `Recording MIDI to ${track.name}`
            : play.computerKeyboard
              ? settings.mode === 'keys'
                ? 'Computer keyboard: A–K play · Z / X octave'
                : 'Computer keyboard plays the pads'
              : 'Computer keyboard off'}
        </span>
        {playing && <span className={styles.playing}>{playing}</span>}
      </Toolbar>
      <div className={styles.instrumentSurface}>
        {settings.mode === 'keys' ? (
          <MusicalTypingKeyboard
            octave={settings.octave}
            activeNotes={play.melodic.activeNotes}
            onNoteDown={play.melodic.triggerNoteDown}
            onNoteUp={play.melodic.triggerNoteUp}
          />
        ) : (
          <DrumPadGrid
            activeNotes={play.drums.activeNotes}
            onPadDown={play.drums.triggerPadDown}
            onPadUp={play.drums.triggerPadUp}
          />
        )}
      </div>
      {play.message && (
        <small className={styles.message} role="status">
          {play.message}
        </small>
      )}
    </div>
  );
}
