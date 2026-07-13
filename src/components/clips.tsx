import { useState } from 'react';
import type {
  MidiClip,
  MidiNote,
  RecordingAsset,
  RenderOptions,
  RenderResult,
  Session,
} from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { notesFromMidiEvents } from '@/lib/midi';

export function SamplePadEditor({
  session,
  setSession,
}: {
  session: Session;
  setSession: (value: Session) => void;
}) {
  if (!session.samplePads.length) return null;
  const updateRange = (id: string, field: 'startMs' | 'endMs', value: number) => {
    const safeValue = Math.max(0, Math.round(Number.isFinite(value) ? value : 0));
    setSession({
      ...session,
      samplePads: session.samplePads.map((pad) => {
        if (pad.id !== id) return pad;
        const startMs = field === 'startMs' ? safeValue : pad.startMs;
        const endMs = field === 'endMs' ? Math.max(1, safeValue) : pad.endMs;
        return field === 'startMs'
          ? { ...pad, startMs, endMs: Math.max(endMs, startMs + 1) }
          : { ...pad, startMs: Math.min(startMs, Math.max(0, endMs - 1)), endMs };
      }),
    });
  };
  const updatePadValue = (id: string, field: 'gainDb', value: number) =>
    setSession({
      ...session,
      samplePads: session.samplePads.map((pad) =>
        pad.id === id
          ? { ...pad, [field]: Math.max(-90, Math.min(24, Number.isFinite(value) ? value : 0)) }
          : pad,
      ),
    });
  const togglePadLoop = (id: string) =>
    setSession({
      ...session,
      samplePads: session.samplePads.map((pad) =>
        pad.id === id ? { ...pad, loopEnabled: !pad.loopEnabled } : pad,
      ),
    });
  const removePad = (id: string) =>
    setSession({ ...session, samplePads: session.samplePads.filter((pad) => pad.id !== id) });
  return (
    <section className="section-card sample-editor">
      <header>
        <div>
          <span className="eyebrow">SLICE RANGES</span>
          <h2>Non-destructive pad regions</h2>
        </div>
        <small>Source files remain untouched</small>
      </header>
      {session.samplePads.map((pad) => (
        <div className="sample-edit-row" key={pad.id}>
          <div className="sample-edit-name">
            <strong>{pad.name}</strong>
            <small>
              MIDI {pad.midiKey} · {pad.endMs - pad.startMs} ms
            </small>
          </div>
          <label>
            <span>Start</span>
            <input
              type="number"
              min="0"
              step="1"
              value={pad.startMs}
              onChange={(event) => updateRange(pad.id, 'startMs', Number(event.target.value))}
            />
          </label>
          <label>
            <span>End</span>
            <input
              type="number"
              min="1"
              step="1"
              value={pad.endMs}
              onChange={(event) => updateRange(pad.id, 'endMs', Number(event.target.value))}
            />
          </label>
          <label>
            <span>Gain dB</span>
            <input
              type="number"
              min="-90"
              max="24"
              step="0.5"
              value={pad.gainDb}
              onChange={(event) => updatePadValue(pad.id, 'gainDb', Number(event.target.value))}
            />
          </label>
          <button className="text-button" onClick={() => togglePadLoop(pad.id)}>
            {pad.loopEnabled ? 'Loop on' : 'Loop'}
          </button>
          <button className="text-button danger" onClick={() => removePad(pad.id)}>
            Remove
          </button>
        </div>
      ))}
    </section>
  );
}

export function SamplePreviewControls({
  session,
  playingId,
  onPreview,
  onStop,
}: {
  session: Session;
  playingId: string | null;
  onPreview: (pad: Session['samplePads'][number]) => void;
  onStop: () => void;
}) {
  if (!session.samplePads.length) return null;
  return (
    <section className="section-card sample-preview">
      <header>
        <div>
          <span className="eyebrow">PREVIEW BUS</span>
          <h2>Audition mapped regions</h2>
        </div>
        <button className="text-button" disabled={!playingId} onClick={onStop}>
          Stop
        </button>
      </header>
      {session.samplePads.map((pad) => (
        <div className="sample-preview-row" key={pad.id}>
          <div>
            <strong>{pad.name}</strong>
            <small>
              MIDI {pad.midiKey} · {pad.startMs}–{pad.endMs} ms
            </small>
          </div>
          <button
            className={`text-button ${playingId === pad.id ? 'active' : ''}`}
            onClick={() => onPreview(pad)}
          >
            {playingId === pad.id ? 'Playing' : 'Preview'}
          </button>
        </div>
      ))}
    </section>
  );
}

export function TimelineClipInspector({
  session,
  setSession,
}: {
  session: Session;
  setSession: (value: Session) => void;
}) {
  if (!session.timeline.length) return null;
  const update = (
    id: string,
    field:
      | 'startMs'
      | 'durationMs'
      | 'sourceInMs'
      | 'sourceOutMs'
      | 'gainDb'
      | 'fadeInMs'
      | 'fadeOutMs'
      | 'pan',
    value: number,
  ) => {
    const safeValue = Number.isFinite(value) ? value : 0;
    setSession({
      ...session,
      timeline: session.timeline.map((clip) => {
        if (clip.id !== id) return clip;
        if (field === 'startMs') return { ...clip, startMs: Math.max(0, Math.round(safeValue)) };
        if (field === 'durationMs')
          return { ...clip, durationMs: Math.max(1, Math.round(safeValue)) };
        if (field === 'sourceInMs')
          return { ...clip, sourceInMs: Math.max(0, Math.round(safeValue)) };
        if (field === 'sourceOutMs')
          return { ...clip, sourceOutMs: Math.max(0, Math.round(safeValue)) };
        if (field === 'gainDb') return { ...clip, gainDb: Math.max(-90, Math.min(24, safeValue)) };
        if (field === 'fadeInMs')
          return {
            ...clip,
            fadeInMs: Math.max(0, Math.min(clip.durationMs, Math.round(safeValue))),
          };
        if (field === 'fadeOutMs')
          return {
            ...clip,
            fadeOutMs: Math.max(0, Math.min(clip.durationMs, Math.round(safeValue))),
          };
        return { ...clip, pan: Math.max(-1, Math.min(1, safeValue)) };
      }),
    });
  };
  const setTrack = (id: string, trackId: string) =>
    setSession({
      ...session,
      timeline: session.timeline.map((clip) => (clip.id === id ? { ...clip, trackId } : clip)),
    });
  const toggleMute = (id: string) =>
    setSession({
      ...session,
      timeline: session.timeline.map((clip) =>
        clip.id === id ? { ...clip, muted: !clip.muted } : clip,
      ),
    });
  const toggleLoop = (id: string) =>
    setSession({
      ...session,
      timeline: session.timeline.map((clip) =>
        clip.id === id ? { ...clip, loopEnabled: !clip.loopEnabled } : clip,
      ),
    });
  const duplicate = (id: string) => {
    const index = session.timeline.findIndex((clip) => clip.id === id);
    if (index < 0) return;
    const clip = session.timeline[index];
    const copy = {
      ...clip,
      id: `${clip.id}:copy:${Date.now()}`,
      name: `${clip.name} copy`,
      startMs: clip.startMs + clip.durationMs,
    };
    const timeline = [...session.timeline];
    timeline.splice(index + 1, 0, copy);
    setSession({ ...session, timeline });
  };
  const split = (id: string) => {
    const index = session.timeline.findIndex((clip) => clip.id === id);
    if (index < 0) return;
    const clip = session.timeline[index];
    const firstDuration = Math.floor(clip.durationMs / 2);
    if (firstDuration < 1) return;
    const secondDuration = clip.durationMs - firstDuration;
    const sourceEnd = clip.sourceOutMs || clip.sourceInMs + clip.durationMs;
    const sourceSplit = Math.min(sourceEnd, clip.sourceInMs + firstDuration);
    const secondSourceOut = clip.loopEnabled
      ? clip.sourceOutMs
      : clip.sourceOutMs > 0 && sourceEnd > sourceSplit
        ? sourceEnd
        : 0;
    const first = {
      ...clip,
      durationMs: firstDuration,
      sourceOutMs: clip.loopEnabled ? clip.sourceOutMs : sourceSplit,
    };
    const second = {
      ...clip,
      id: `${clip.id}:split:${Date.now()}`,
      name: `${clip.name} 2`,
      startMs: clip.startMs + firstDuration,
      durationMs: secondDuration,
      sourceInMs: clip.loopEnabled ? clip.sourceInMs : sourceSplit,
      sourceOutMs: secondSourceOut,
    };
    const timeline = [...session.timeline];
    timeline.splice(index, 1, first, second);
    setSession({ ...session, timeline });
  };
  const remove = (id: string) =>
    setSession({ ...session, timeline: session.timeline.filter((clip) => clip.id !== id) });
  return (
    <section className="section-card timeline-editor">
      <header>
        <div>
          <span className="eyebrow">CLIP INSPECTOR</span>
          <h2>Non-destructive edits</h2>
        </div>
        <small>Source WAVs remain unchanged</small>
      </header>
      {session.timeline.map((clip) => (
        <div
          className={`timeline-edit-row timeline-edit-row-expanded ${clip.muted ? 'muted' : ''}`}
          key={clip.id}
        >
          <div className="timeline-edit-name">
            <strong>{clip.name}</strong>
            <small>{clip.assetPath}</small>
          </div>
          <label>
            <span>Track</span>
            <select
              value={clip.trackId}
              onChange={(event) => setTrack(clip.id, event.target.value)}
            >
              {session.tracks.map((track) => (
                <option value={track.id} key={track.id}>
                  {track.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Start ms</span>
            <input
              type="number"
              min="0"
              value={clip.startMs}
              onChange={(event) => update(clip.id, 'startMs', Number(event.target.value))}
            />
          </label>
          <label>
            <span>Length ms</span>
            <input
              type="number"
              min="1"
              value={clip.durationMs}
              onChange={(event) => update(clip.id, 'durationMs', Number(event.target.value))}
            />
          </label>
          <label>
            <span>Source in</span>
            <input
              type="number"
              min="0"
              value={clip.sourceInMs}
              onChange={(event) => update(clip.id, 'sourceInMs', Number(event.target.value))}
            />
          </label>
          <label>
            <span>Source out</span>
            <input
              type="number"
              min="0"
              value={clip.sourceOutMs}
              onChange={(event) => update(clip.id, 'sourceOutMs', Number(event.target.value))}
            />
          </label>
          <label>
            <span>Gain dB</span>
            <input
              type="number"
              min="-90"
              max="24"
              step="0.5"
              value={clip.gainDb}
              onChange={(event) => update(clip.id, 'gainDb', Number(event.target.value))}
            />
          </label>
          <label>
            <span>Fade in</span>
            <input
              type="number"
              min="0"
              value={clip.fadeInMs}
              onChange={(event) => update(clip.id, 'fadeInMs', Number(event.target.value))}
            />
          </label>
          <label>
            <span>Fade out</span>
            <input
              type="number"
              min="0"
              value={clip.fadeOutMs}
              onChange={(event) => update(clip.id, 'fadeOutMs', Number(event.target.value))}
            />
          </label>
          <label>
            <span>Pan</span>
            <input
              type="number"
              min="-1"
              max="1"
              step="0.05"
              value={clip.pan}
              onChange={(event) => update(clip.id, 'pan', Number(event.target.value))}
            />
          </label>
          <button className="text-button" onClick={() => toggleLoop(clip.id)}>
            {clip.loopEnabled ? 'Loop on' : 'Loop'}
          </button>
          <button className="text-button" onClick={() => duplicate(clip.id)}>
            Duplicate
          </button>
          <button className="text-button" onClick={() => split(clip.id)}>
            Split
          </button>
          <button className="text-button" onClick={() => toggleMute(clip.id)}>
            {clip.muted ? 'Unmute' : 'Mute'}
          </button>
          <button className="text-button danger" onClick={() => remove(clip.id)}>
            Remove
          </button>
        </div>
      ))}
    </section>
  );
}

export function TimelineRenderControls({
  session,
  result,
  stems,
  message,
  onRender,
  onRenderStems,
  onPreview,
  onStop,
  previewing,
}: {
  session: Session;
  result: RenderResult | null;
  stems: RenderResult[];
  message: string;
  onRender: (options: RenderOptions) => void;
  onRenderStems: (options: RenderOptions) => void;
  onPreview: () => void;
  onStop: () => void;
  previewing: boolean;
}) {
  const [rangeStartMs, setRangeStartMs] = useState(0);
  const [rangeEndMs, setRangeEndMs] = useState('');
  const [normalize, setNormalize] = useState(false);
  const [trackId, setTrackId] = useState('master');
  const options = (): RenderOptions => ({
    rangeStartMs: Math.max(0, Math.round(Number(rangeStartMs) || 0)),
    rangeEndMs: rangeEndMs.trim() ? Math.max(1, Math.round(Number(rangeEndMs) || 1)) : null,
    normalize,
    trackId: trackId === 'master' ? null : trackId,
  });
  const submit = () => onRender(options());
  const submitStems = () => onRenderStems(options());
  const hasAudibleClips = session.timeline.some((clip) => !clip.muted);
  return (
    <section className="section-card timeline-render">
      <header>
        <div>
          <span className="eyebrow">OFFLINE RENDER</span>
          <h2>Export audible timeline</h2>
        </div>
        <div>
          <button className="text-button" disabled={!hasAudibleClips} onClick={submit}>
            Render WAV
          </button>
          <button
            className="text-button"
            disabled={!hasAudibleClips || session.tracks.length < 1}
            onClick={submitStems}
          >
            Render stems
          </button>
          {result && (
            <button className="text-button" onClick={previewing ? onStop : onPreview}>
              {previewing ? 'Stop preview' : 'Preview'}
            </button>
          )}
        </div>
      </header>
      <p className="inspector-copy">
        Writes a new stereo float WAV with clip position, gain, fade, pan and mute state. Source
        assets are never flattened. Stem export writes one safe, independent WAV per audible track.
      </p>
      <div className="render-options">
        <label>
          <span>Target</span>
          <select value={trackId} onChange={(event) => setTrackId(event.target.value)}>
            <option value="master">Master mix</option>
            {session.tracks.map((track) => (
              <option value={track.id} key={track.id}>
                {track.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Range start ms</span>
          <input
            type="number"
            min="0"
            value={rangeStartMs}
            onChange={(event) => setRangeStartMs(Number(event.target.value))}
          />
        </label>
        <label>
          <span>Range end ms</span>
          <input
            type="number"
            min="1"
            placeholder="Timeline end"
            value={rangeEndMs}
            onChange={(event) => setRangeEndMs(event.target.value)}
          />
        </label>
        <label className="render-normalize">
          <input
            type="checkbox"
            checked={normalize}
            onChange={(event) => setNormalize(event.target.checked)}
          />
          <span>Normalize to -0.2 dBFS</span>
        </label>
      </div>
      {result ? (
        <div className="render-result">
          <strong>
            {result.durationMs / 1000}s · {result.clipCount} clips ·{' '}
            {result.trackId ? 'track export' : 'master mix'} ·{' '}
            {result.normalized ? 'normalized' : 'master gain'}
          </strong>
          <small>
            {result.rangeStartMs}–{result.rangeEndMs} ms
          </small>
          <code>{result.path}</code>
        </div>
      ) : (
        <small className="render-message">{message}</small>
      )}
      {stems.length > 0 && (
        <div className="stem-results">
          <strong>{stems.length} track stems ready</strong>
          {stems.map((stem) => (
            <div className="stem-result" key={stem.id}>
              <span>
                {session.tracks.find((track) => track.id === stem.trackId)?.name ??
                  stem.trackId ??
                  'Track'}
              </span>
              <small>
                {stem.durationMs / 1000}s · {stem.clipCount} clips
              </small>
              <code>{stem.path}</code>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

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
