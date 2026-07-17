import type { CreativeSession, RecordingAsset } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

export function WorkspaceArrange({
  session,
  setSession,
  recordings,
  onPlaceRecording,
  api,
}: {
  session: CreativeSession;
  setSession: (value: CreativeSession) => void;
  recordings: RecordingAsset[];
  onPlaceRecording: (recording: RecordingAsset) => void;
  api: NativeApi;
}) {
  const timelineEnd = Math.max(
    10_000,
    ...session.arrangement.audioClips.map((clip) => clip.positionMs + clip.durationMs),
  );
  const toggleTrack = async (id: string, field: 'muted' | 'solo') => {
    const track = session.arrangement.tracks.find((item) => item.id === id);
    if (!track) return;
    setSession(await api.updateTrack(id, { [field]: !track[field] }));
  };
  const updateTrack = async (id: string, field: 'gainDb' | 'pan', value: number) => {
    setSession(await api.updateTrack(id, { [field]: value }));
  };
  const addTrack = async () => {
    const name = window
      .prompt('Track name', `Track ${session.arrangement.tracks.length + 1}`)
      ?.trim();
    if (!name) return;
    setSession(await api.addTrack(name));
  };
  return (
    <div className="arrange-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">NON-DESTRUCTIVE TIMELINE</span>
          <h1>Arrange ideas without moving sources</h1>
        </div>
        <span className="status-tag">
          {session.arrangement.audioClips.length} CLIPS · {session.arrangement.tracks.length} TRACKS
        </span>
      </section>
      <section className="section-card track-mixer">
        <header>
          <div>
            <span className="eyebrow">TRACK MIXER</span>
            <h2>Shared lanes and safe mix state</h2>
          </div>
          <button className="text-button" onClick={() => void addTrack()}>
            Add track
          </button>
        </header>
        <div className="track-mixer-grid">
          {session.arrangement.tracks.map((track) => (
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
                  onChange={(event) =>
                    void updateTrack(track.id, 'gainDb', Number(event.target.value))
                  }
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
                  onChange={(event) =>
                    void updateTrack(track.id, 'pan', Number(event.target.value))
                  }
                />
              </label>
              <button className="text-button" onClick={() => void toggleTrack(track.id, 'muted')}>
                {track.muted ? 'Unmute' : 'Mute'}
              </button>
              <button className="text-button" onClick={() => void toggleTrack(track.id, 'solo')}>
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
          {session.arrangement.audioClips.length === 0 && (
            <small>InboxのRecordingを右側からTimelineへ配置できます。</small>
          )}
          {session.arrangement.audioClips.map((clip) => {
            const track = session.arrangement.tracks.find((item) => item.id === clip.trackId);
            return (
              <article
                className={`timeline-clip ${clip.muted || track?.muted ? 'muted' : ''}`}
                key={clip.id}
                style={{
                  left: `${(clip.positionMs / timelineEnd) * 100}%`,
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
