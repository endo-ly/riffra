import type { RecordingAsset, Session } from '@/lib/domain';

export function WorkspaceArrange({
  session,
  setSession,
  recordings,
  onPlaceRecording,
}: {
  session: Session;
  setSession: (value: Session) => void;
  recordings: RecordingAsset[];
  onPlaceRecording: (recording: RecordingAsset) => void;
}) {
  const timelineEnd = Math.max(
    10_000,
    ...session.timeline.map((clip) => clip.startMs + clip.durationMs),
  );
  const toggleTrack = (id: string, field: 'muted' | 'solo') =>
    setSession({
      ...session,
      tracks: session.tracks.map((track) =>
        track.id === id ? { ...track, [field]: !track[field] } : track,
      ),
    });
  const updateTrack = (id: string, field: 'gainDb' | 'pan', value: number) =>
    setSession({
      ...session,
      tracks: session.tracks.map((track) =>
        track.id !== id
          ? track
          : {
              ...track,
              [field]:
                field === 'gainDb'
                  ? Math.max(-90, Math.min(24, value))
                  : Math.max(-1, Math.min(1, value)),
            },
      ),
    });
  const addTrack = () => {
    const name = window.prompt('Track name', `Track ${session.tracks.length + 1}`)?.trim();
    if (!name) return;
    setSession({
      ...session,
      tracks: [
        ...session.tracks,
        {
          id: `track:${Date.now()}`,
          name: name.slice(0, 80),
          gainDb: 0,
          pan: 0,
          muted: false,
          solo: false,
        },
      ],
    });
  };
  return (
    <div className="arrange-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">NON-DESTRUCTIVE TIMELINE</span>
          <h1>Arrange ideas without moving sources</h1>
        </div>
        <span className="status-tag">
          {session.timeline.length} CLIPS · {session.tracks.length} TRACKS
        </span>
      </section>
      <section className="section-card track-mixer">
        <header>
          <div>
            <span className="eyebrow">TRACK MIXER</span>
            <h2>Shared lanes and safe mix state</h2>
          </div>
          <button className="text-button" onClick={addTrack}>
            Add track
          </button>
        </header>
        <div className="track-mixer-grid">
          {session.tracks.map((track) => (
            <div className={`track-mixer-row ${track.muted ? 'muted' : ''}`} key={track.id}>
              <strong>{track.name}</strong>
              <label>
                <span>Gain</span>
                <input
                  type="number"
                  min="-90"
                  max="24"
                  step="0.5"
                  value={track.gainDb}
                  onChange={(event) => updateTrack(track.id, 'gainDb', Number(event.target.value))}
                />
              </label>
              <label>
                <span>Pan</span>
                <input
                  type="number"
                  min="-1"
                  max="1"
                  step="0.05"
                  value={track.pan}
                  onChange={(event) => updateTrack(track.id, 'pan', Number(event.target.value))}
                />
              </label>
              <button className="text-button" onClick={() => toggleTrack(track.id, 'muted')}>
                {track.muted ? 'Unmute' : 'Mute'}
              </button>
              <button className="text-button" onClick={() => toggleTrack(track.id, 'solo')}>
                {track.solo ? 'Unsolo' : 'Solo'}
              </button>
            </div>
          ))}
        </div>
      </section>
      <section className="section-card timeline-card">
        <div className="timeline-ruler">
          <span>00:00</span>
          <span>{(timelineEnd / 1000).toFixed(1)} s</span>
        </div>
        <div className="timeline-lane">
          {session.timeline.length === 0 && (
            <small>InboxのRecordingを右側からTimelineへ配置できます。</small>
          )}
          {session.timeline.map((clip) => {
            const track = session.tracks.find((item) => item.id === clip.trackId);
            return (
              <article
                className={`timeline-clip ${clip.muted || track?.muted ? 'muted' : ''}`}
                key={clip.id}
                style={{
                  left: `${(clip.startMs / timelineEnd) * 100}%`,
                  width: `${Math.max(8, (clip.durationMs / timelineEnd) * 100)}%`,
                }}
              >
                <strong>{clip.name}</strong>
                <small>
                  {track?.name ?? 'Main'} · {clip.gainDb.toFixed(1)} dB ·{' '}
                  {clip.muted ? 'Muted' : 'Source linked'}
                </small>
              </article>
            );
          })}
        </div>
      </section>
      <section className="section-card arrange-sources">
        <header>
          <div>
            <span className="eyebrow">INBOX SOURCES</span>
            <h2>素材を配置</h2>
          </div>
          <small>元ファイルは変更されません</small>
        </header>
        {recordings.length === 0 ? (
          <p className="inspector-copy">まだ録音がありません。</p>
        ) : (
          recordings.slice(0, 12).map((recording) => (
            <div className="source-row" key={recording.id}>
              <div>
                <strong>{recording.name}</strong>
                <small>
                  {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                </small>
              </div>
              <button className="text-button" onClick={() => onPlaceRecording(recording)}>
                Place
              </button>
            </div>
          ))
        )}
      </section>
    </div>
  );
}
