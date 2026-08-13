import type { CreativeSession } from '@/model/domain';
import surface from '@/shared/ui/Surface.module.css';
import styles from './SampleWorkspace.module.css';

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
    <section className={`${surface.sectionCard} ${styles.samplePreview}`}>
      <header>
        <div>
          <span className={surface.eyebrow}>PREVIEW BUS</span>
          <h2>Audition mapped regions</h2>
        </div>
        <button className={surface.textButton} disabled={!playingId} onClick={onStop}>
          Stop
        </button>
      </header>
      {session.playState.sampleInstrument.pads.map((pad) => (
        <div className={styles.samplePreviewRow} key={pad.id}>
          <div>
            <strong>{pad.name}</strong>
            <small>
              MIDI {pad.midiKey} · {pad.startMs}–{pad.endMs} ms
            </small>
          </div>
          <button
            className={`${surface.textButton} ${playingId === pad.id ? styles.active : ''}`}
            onClick={() => onPreview(pad)}
          >
            {playingId === pad.id ? 'Playing' : 'Preview'}
          </button>
        </div>
      ))}
    </section>
  );
}
