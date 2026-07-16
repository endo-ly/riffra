import type { Session } from '@/lib/domain';

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
