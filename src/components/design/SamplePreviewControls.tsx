import type { CreativeSession } from '@/lib/domain';

export function SamplePreviewControls({
  session,
  playingId,
  onPreview,
  onStop,
}: {
  session: CreativeSession;
  playingId: string | null;
  onPreview: (pad: CreativeSession['playState']['sampleInstrument']['pads'][number]) => void;
  onStop: () => void;
}) {
  if (!session.playState.sampleInstrument.pads.length) return null;
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
      {session.playState.sampleInstrument.pads.map((pad) => (
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
