import type { RecordingAsset, Session } from '@/lib/domain';

export function WorkspaceSample({
  session,
  recordings,
  onCreateSamplePad,
  onPreviewPad,
}: {
  session: Session;
  recordings: RecordingAsset[];
  onCreateSamplePad: (recording: RecordingAsset) => void;
  onPreviewPad: (pad: Session['samplePads'][number]) => void;
}) {
  const pads = Array.from({ length: 16 }, (_, index) => session.samplePads[index] ?? null);
  return (
    <div className="workspace-scroll sample-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">SAMPLE INSTRUMENT</span>
          <h1>Audio → Pad / Keyboard</h1>
        </div>
        <span className="status-tag">SOURCE MAPPING</span>
      </section>
      <section className="section-card pad-card">
        <header>
          <div>
            <span className="eyebrow">PADS</span>
            <h2>{session.samplePads.length} mapped</h2>
          </div>
          <small>Playback engine follows this mapping gate</small>
        </header>
        <div className="pad-grid">
          {pads.map((pad, index) => (
            <button
              className={`sample-pad ${pad ? 'filled' : 'empty'}`}
              key={pad?.id ?? `empty-${index}`}
              onClick={pad ? () => onPreviewPad(pad) : undefined}
              aria-label={pad ? `Preview ${pad.name}` : `Empty pad ${index + 1}`}
            >
              <strong>{pad?.name ?? `Pad ${index + 1}`}</strong>
              <small>{pad ? `MIDI ${pad.midiKey}` : 'Empty'}</small>
            </button>
          ))}
        </div>
      </section>
      <section className="section-card sample-sources">
        <header>
          <div>
            <span className="eyebrow">SOURCES</span>
            <h2>録音をPadへ割り当てる</h2>
          </div>
          <small>元ファイルは変更されません</small>
        </header>
        {recordings.length === 0 ? (
          <p className="inspector-copy">Inboxに録音がありません。</p>
        ) : (
          recordings.slice(0, 12).map((recording) => (
            <div className="source-row" key={recording.id}>
              <div>
                <strong>{recording.name}</strong>
                <small>
                  {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                </small>
              </div>
              <button className="text-button" onClick={() => onCreateSamplePad(recording)}>
                Map to Pad
              </button>
            </div>
          ))
        )}
      </section>
    </div>
  );
}
