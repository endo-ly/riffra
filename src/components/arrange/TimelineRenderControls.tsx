import { useState } from 'react';
import type { CreativeSession, RenderOptions, RenderResult } from '@/lib/domain';

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
  session: CreativeSession;
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
  const hasAudibleClips = session.arrangement.audioClips.some((clip) => !clip.muted);
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
            disabled={!hasAudibleClips || session.arrangement.tracks.length < 1}
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
            {session.arrangement.tracks.map((track) => (
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
            <div className="stem-result" key={stem.assetId}>
              <span>
                {session.arrangement.tracks.find((track) => track.id === stem.trackId)?.name ??
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
