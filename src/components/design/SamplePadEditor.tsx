import type { CreativeSession } from '@/lib/domain';

export function SamplePadEditor({
  session,
  updateSamplePad,
  removeSamplePad,
}: {
  session: CreativeSession;
  updateSamplePad: (
    padId: string,
    patch: { startMs?: number; endMs?: number; gainDb?: number; loopEnabled?: boolean },
  ) => void;
  removeSamplePad: (padId: string) => void;
}) {
  if (!session.playState.sampleInstrument.pads.length) return null;
  const updateRange = (id: string, field: 'startMs' | 'endMs', value: number) => {
    const safeValue = Math.max(0, Math.round(Number.isFinite(value) ? value : 0));
    if (field === 'startMs') updateSamplePad(id, { startMs: safeValue });
    else updateSamplePad(id, { endMs: safeValue });
  };
  const updatePadValue = (id: string, field: 'gainDb', value: number) => {
    const clamped = Number.isFinite(value) ? value : 0;
    void updateSamplePad(id, { [field]: clamped });
  };
  const togglePadLoop = (id: string) =>
    void updateSamplePad(id, {
      loopEnabled: !session.playState.sampleInstrument.pads.find((pad) => pad.id === id)
        ?.loopEnabled,
    });
  const removePad = (id: string) => void removeSamplePad(id);
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
