import { useEffect, useState, type FormEvent } from 'react';
import type {
  InstrumentMidiSettings,
  InstrumentMidiWarning,
  InstrumentRecommendedRange,
  PlayableNotes,
} from '@/model/domain';
import { midiNoteName } from '@/features/arrange/play-surface/musical-typing';
import styles from './Devices.module.css';

interface Props {
  settings: InstrumentMidiSettings;
  recommendedRange: InstrumentRecommendedRange | null;
  warnings: InstrumentMidiWarning[];
  onSave: (settings: InstrumentMidiSettings) => void;
}

function playableMode(playable: PlayableNotes | null): 'none' | 'range' | 'set' {
  return playable?.type ?? 'none';
}

export function InstrumentMidiSettingsEditor({
  settings,
  recommendedRange,
  warnings,
  onSave,
}: Props) {
  const [transpose, setTranspose] = useState(settings.transposeSemitones);
  const [mode, setMode] = useState<'none' | 'range' | 'set'>(playableMode(settings.playableNotes));
  const [rangeStart, setRangeStart] = useState(
    settings.playableNotes?.type === 'range' ? settings.playableNotes.start : 0,
  );
  const [rangeEnd, setRangeEnd] = useState(
    settings.playableNotes?.type === 'range' ? settings.playableNotes.end : 127,
  );
  const [noteSet, setNoteSet] = useState(
    settings.playableNotes?.type === 'set' ? settings.playableNotes.notes.join(', ') : '',
  );
  const [keyswitches, setKeyswitches] = useState(settings.keyswitches);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setTranspose(settings.transposeSemitones);
    setMode(playableMode(settings.playableNotes));
    setRangeStart(settings.playableNotes?.type === 'range' ? settings.playableNotes.start : 0);
    setRangeEnd(settings.playableNotes?.type === 'range' ? settings.playableNotes.end : 127);
    setNoteSet(
      settings.playableNotes?.type === 'set' ? settings.playableNotes.notes.join(', ') : '',
    );
    setKeyswitches(settings.keyswitches);
  }, [settings]);

  const updateKeyswitch = (index: number, patch: Partial<(typeof keyswitches)[number]>) =>
    setKeyswitches((current) =>
      current.map((keyswitch, item) => (item === index ? { ...keyswitch, ...patch } : keyswitch)),
    );

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    let playableNotes: PlayableNotes | null = null;
    if (mode === 'range') {
      if (
        !Number.isInteger(rangeStart) ||
        !Number.isInteger(rangeEnd) ||
        rangeStart < 0 ||
        rangeEnd > 127 ||
        rangeStart > rangeEnd
      ) {
        setError('Playable note range must have integer bounds in ascending order.');
        return;
      }
      playableNotes = { type: 'range', start: rangeStart, end: rangeEnd };
    }
    if (mode === 'set') {
      const values = noteSet.split(',').map((value) => value.trim());
      const notes = values.map((value) => Number(value));
      if (
        values.some((value) => !value) ||
        notes.some((note) => !Number.isInteger(note) || note < 0 || note > 127) ||
        new Set(notes).size !== notes.length
      ) {
        setError('Enter a comma-separated set of MIDI notes from 0 to 127.');
        return;
      }
      notes.sort((left, right) => left - right);
      playableNotes = { type: 'set', notes };
    }
    onSave({
      transposeSemitones: transpose,
      playableNotes,
      keyswitches: keyswitches.map((keyswitch) => ({
        note: keyswitch.note,
        name: keyswitch.name.trim(),
      })),
    });
  };

  return (
    <section className={styles.midiSettings} aria-label="Instrument MIDI settings">
      <h4>Instrument MIDI</h4>
      {recommendedRange && (
        <p>
          Library recommendation: {midiNoteName(recommendedRange.minMidi)}–
          {midiNoteName(recommendedRange.maxMidi)}
        </p>
      )}
      <form onSubmit={submit}>
        <label>
          Transpose (semitones)
          <input
            type="number"
            min={-127}
            max={127}
            step={1}
            value={transpose}
            onChange={(event) => setTranspose(event.currentTarget.valueAsNumber)}
            required
          />
        </label>
        <label>
          Playable notes
          <select
            value={mode}
            onChange={(event) => setMode(event.currentTarget.value as typeof mode)}
          >
            <option value="none">Any MIDI note</option>
            <option value="range">Inclusive range</option>
            <option value="set">Explicit note set</option>
          </select>
        </label>
        {mode === 'range' && (
          <div className={styles.midiRange}>
            <label>
              From
              <input
                type="number"
                min={0}
                max={127}
                value={rangeStart}
                onChange={(event) => setRangeStart(event.currentTarget.valueAsNumber)}
                required
              />
            </label>
            <label>
              To
              <input
                type="number"
                min={0}
                max={127}
                value={rangeEnd}
                onChange={(event) => setRangeEnd(event.currentTarget.valueAsNumber)}
                required
              />
            </label>
          </div>
        )}
        {mode === 'set' && (
          <label>
            MIDI note numbers
            <input
              type="text"
              value={noteSet}
              placeholder="36, 48, 60, 72"
              onChange={(event) => setNoteSet(event.currentTarget.value)}
              required
            />
          </label>
        )}
        <fieldset>
          <legend>Keyswitches</legend>
          {keyswitches.map((keyswitch, index) => (
            <div className={styles.keyswitchRow} key={`${index}:${keyswitch.note}`}>
              <label>
                Note
                <input
                  aria-label={`Keyswitch ${index + 1} note`}
                  type="number"
                  min={0}
                  max={127}
                  value={keyswitch.note}
                  onChange={(event) =>
                    updateKeyswitch(index, { note: event.currentTarget.valueAsNumber })
                  }
                />
              </label>
              <label>
                Name
                <input
                  aria-label={`Keyswitch ${index + 1} name`}
                  value={keyswitch.name}
                  onChange={(event) => updateKeyswitch(index, { name: event.currentTarget.value })}
                />
              </label>
              <button
                type="button"
                aria-label={`Remove keyswitch ${index + 1}`}
                onClick={() =>
                  setKeyswitches((current) => current.filter((_, item) => item !== index))
                }
              >
                Remove
              </button>
            </div>
          ))}
          <button
            type="button"
            onClick={() => setKeyswitches((current) => [...current, { note: 24, name: '' }])}
          >
            Add keyswitch
          </button>
        </fieldset>
        <button type="submit">Apply MIDI settings</button>
      </form>
      {error && <p role="alert">{error}</p>}
      {warnings.length > 0 && (
        <div className={styles.midiWarnings} role="status" aria-label="MIDI playability warnings">
          <strong>
            {warnings.length} note{warnings.length === 1 ? '' : 's'} need attention
          </strong>
          <ul>
            {warnings.map((warning) => (
              <li key={warning.noteId}>
                {midiNoteName(warning.sourceNote)} →{' '}
                {warning.outputNote === null
                  ? 'outside MIDI range'
                  : midiNoteName(warning.outputNote)}
                {warning.reason === 'outsidePlayableRange' ? ' is outside the playable range' : ''}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
