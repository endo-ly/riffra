import { useState } from 'react';
import type { MidiClip, MidiNote, RecordingAsset, Session } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { notesFromMidiEvents } from '@/lib/midi';

export function MidiClipEditor({
  session,
  setSession,
  recordings,
  api,
}: {
  session: Session;
  setSession: (value: Session) => void;
  recordings: RecordingAsset[];
  api: NativeApi;
}) {
  const [message, setMessage] = useState(
    'Recorded MIDI sidecars can be imported as editable clips.',
  );
  const [exportMessage, setExportMessage] = useState('');
  const importRecording = async (recording: RecordingAsset) => {
    if (!recording.midiPath) return;
    const events = await api.readMidiEvents(recording.midiPath);
    const notes = notesFromMidiEvents(events);
    if (!notes.length) {
      setMessage('No note-on/note-off pairs were found in that MIDI sidecar.');
      return;
    }
    const startMs = Math.max(
      0,
      ...session.timeline.map((clip) => clip.startMs + clip.durationMs),
      ...session.midiClips.map((clip) => clip.startMs + clip.durationMs),
    );
    const durationMs = Math.max(1, ...notes.map((note) => note.startMs + note.durationMs));
    const clip: MidiClip = {
      id: `midi:${recording.id}`,
      name: recording.name,
      startMs,
      durationMs,
      notes,
      muted: false,
    };
    setSession({
      ...session,
      midiClips: [...session.midiClips.filter((item) => item.id !== clip.id), clip],
      workspace: 'arrange',
    });
    setMessage(`${notes.length} notes imported from ${recording.name}.`);
  };
  const updateNote = (
    clipId: string,
    noteId: string,
    field: keyof Pick<MidiNote, 'note' | 'startMs' | 'durationMs' | 'velocity' | 'channel'>,
    value: number,
  ) => {
    const safeValue = Number.isFinite(value) ? Math.round(value) : 0;
    setSession({
      ...session,
      midiClips: session.midiClips.map((clip) =>
        clip.id !== clipId
          ? clip
          : {
              ...clip,
              notes: clip.notes.map((note) =>
                note.id !== noteId
                  ? note
                  : {
                      ...note,
                      [field]:
                        field === 'note' || field === 'velocity' || field === 'channel'
                          ? Math.max(
                              field === 'channel' ? 1 : 0,
                              Math.min(field === 'channel' ? 16 : 127, safeValue),
                            )
                          : Math.max(field === 'durationMs' ? 1 : 0, safeValue),
                    },
              ),
            },
      ),
    });
  };
  const removeNote = (clipId: string, noteId: string) =>
    setSession({
      ...session,
      midiClips: session.midiClips.map((clip) =>
        clip.id !== clipId
          ? clip
          : { ...clip, notes: clip.notes.filter((note) => note.id !== noteId) },
      ),
    });
  const removeClip = (clipId: string) =>
    setSession({ ...session, midiClips: session.midiClips.filter((clip) => clip.id !== clipId) });
  const exportSessionMidi = async () => {
    const result = await api.exportMidi();
    setExportMessage(
      result
        ? `${result.noteCount} notes exported: ${result.path}`
        : 'MIDI export failed; the session remains unchanged.',
    );
  };
  return (
    <section className="section-card midi-clip-editor">
      <header>
        <div>
          <span className="eyebrow">MIDI CLIPS</span>
          <h2>Basic piano-roll editing</h2>
        </div>
        <div>
          <button
            className="text-button"
            disabled={!session.midiClips.some((clip) => !clip.muted && clip.notes.length)}
            onClick={() => void exportSessionMidi()}
          >
            Export .mid
          </button>
          <small>{message}</small>
        </div>
      </header>
      {exportMessage && <small className="render-message">{exportMessage}</small>}
      <div className="midi-import-list">
        {recordings
          .filter((recording) => recording.midiPath)
          .slice(0, 8)
          .map((recording) => (
            <button
              className="text-button"
              key={recording.id}
              onClick={() => void importRecording(recording)}
            >
              Import {recording.name}
            </button>
          ))}
        {!recordings.some((recording) => recording.midiPath) && (
          <small className="inspector-copy">
            Quick Record a MIDI input to create an editable sidecar.
          </small>
        )}
      </div>
      {session.midiClips.map((clip) => (
        <article className={`midi-clip-card ${clip.muted ? 'muted' : ''}`} key={clip.id}>
          <header>
            <div>
              <strong>{clip.name}</strong>
              <small>
                {clip.notes.length} notes · {clip.durationMs} ms
              </small>
            </div>
            <button className="text-button danger" onClick={() => removeClip(clip.id)}>
              Remove
            </button>
          </header>
          <div className="piano-roll" aria-label={`${clip.name} piano roll`}>
            {clip.notes.map((note) => (
              <i
                key={note.id}
                style={{
                  left: `${Math.min(98, (note.startMs / Math.max(1, clip.durationMs)) * 100)}%`,
                  width: `${Math.max(1, (note.durationMs / Math.max(1, clip.durationMs)) * 100)}%`,
                  top: `${((127 - note.note) / 128) * 100}%`,
                }}
              />
            ))}
          </div>
          <div className="midi-note-list">
            {clip.notes.slice(0, 64).map((note) => (
              <div className="midi-note-row" key={note.id}>
                <label>
                  <span>Note</span>
                  <input
                    type="number"
                    min="0"
                    max="127"
                    value={note.note}
                    onChange={(event) =>
                      updateNote(clip.id, note.id, 'note', Number(event.target.value))
                    }
                  />
                </label>
                <label>
                  <span>Start</span>
                  <input
                    type="number"
                    min="0"
                    value={note.startMs}
                    onChange={(event) =>
                      updateNote(clip.id, note.id, 'startMs', Number(event.target.value))
                    }
                  />
                </label>
                <label>
                  <span>Length</span>
                  <input
                    type="number"
                    min="1"
                    value={note.durationMs}
                    onChange={(event) =>
                      updateNote(clip.id, note.id, 'durationMs', Number(event.target.value))
                    }
                  />
                </label>
                <label>
                  <span>Velocity</span>
                  <input
                    type="number"
                    min="1"
                    max="127"
                    value={note.velocity}
                    onChange={(event) =>
                      updateNote(clip.id, note.id, 'velocity', Number(event.target.value))
                    }
                  />
                </label>
                <label>
                  <span>Channel</span>
                  <input
                    type="number"
                    min="1"
                    max="16"
                    value={note.channel}
                    onChange={(event) =>
                      updateNote(clip.id, note.id, 'channel', Number(event.target.value))
                    }
                  />
                </label>
                <button className="text-button danger" onClick={() => removeNote(clip.id, note.id)}>
                  ×
                </button>
              </div>
            ))}
          </div>
          {clip.notes.length > 64 && (
            <small className="inspector-copy">
              Showing first 64 notes; edits remain bounded and persisted.
            </small>
          )}
        </article>
      ))}
    </section>
  );
}
