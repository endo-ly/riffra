import type { CreativeSession } from '@/lib/domain';

export function SamplePadEditor({
  session,
  setSession,
}: {
  session: CreativeSession;
  setSession: (value: CreativeSession) => void;
}) {
  if (!session.playState.sampleInstrument.pads.length) return null;
  const updateRange = (id: string, field: 'startMs' | 'endMs', value: number) => {
    const safeValue = Math.max(0, Math.round(Number.isFinite(value) ? value : 0));
    setSession({
      ...session,
      playState: {
        ...session.playState,
        sampleInstrument: {
          ...session.playState.sampleInstrument,
          pads: session.playState.sampleInstrument.pads.map((pad) => {
            if (pad.id !== id) return pad;
            const startMs = field === 'startMs' ? safeValue : pad.startMs;
            const endMs = field === 'endMs' ? Math.max(1, safeValue) : pad.endMs;
            return field === 'startMs'
              ? { ...pad, startMs, endMs: Math.max(endMs, startMs + 1) }
              : { ...pad, startMs: Math.min(startMs, Math.max(0, endMs - 1)), endMs };
          }),
        },
      },
    });
  };
  const updatePadValue = (id: string, field: 'gainDb', value: number) =>
    setSession({
      ...session,
      playState: {
        ...session.playState,
        sampleInstrument: {
          ...session.playState.sampleInstrument,
          pads: session.playState.sampleInstrument.pads.map((pad) =>
            pad.id === id
              ? { ...pad, [field]: Math.max(-90, Math.min(24, Number.isFinite(value) ? value : 0)) }
              : pad,
          ),
        },
      },
    });
  const togglePadLoop = (id: string) =>
    setSession({
      ...session,
      playState: {
        ...session.playState,
        sampleInstrument: {
          ...session.playState.sampleInstrument,
          pads: session.playState.sampleInstrument.pads.map((pad) =>
            pad.id === id ? { ...pad, loopEnabled: !pad.loopEnabled } : pad,
          ),
        },
      },
    });
  const removePad = (id: string) =>
    setSession({
      ...session,
      playState: {
        ...session.playState,
        sampleInstrument: {
          ...session.playState.sampleInstrument,
          pads: session.playState.sampleInstrument.pads.filter((pad) => pad.id !== id),
        },
      },
    });
  return (
    <section className="section-card sample-editor">
      <header>
        <div>
          <span className="eyebrow">SLICE RANGES</span>
          <h2>Non-destructive pad regions</h2>
        </div>
        <small>Source files remain untouched</small>
      </header>
      {session.playState.sampleInstrument.pads.map((pad) => (
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
