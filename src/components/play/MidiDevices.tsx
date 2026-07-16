import type { MidiProbe } from '@/lib/domain';

export function MidiDevices({ probe, onRefresh }: { probe: MidiProbe; onRefresh: () => void }) {
  return (
    <section className="section-card midi-card">
      <header>
        <div>
          <span className="eyebrow">MIDI DEVICES</span>
          <h2>Input / Output ports</h2>
        </div>
        <button className="text-button" onClick={onRefresh}>
          Refresh
        </button>
      </header>
      <div className="midi-port-grid">
        <div>
          <span className="eyebrow">INPUTS</span>
          {probe.inputs.length ? (
            probe.inputs.map((name) => (
              <div className="midi-port" key={`in:${name}`}>
                <i className="midi-led" />
                <strong>{name}</strong>
              </div>
            ))
          ) : (
            <small className="inspector-copy">No MIDI input is visible.</small>
          )}
        </div>
        <div>
          <span className="eyebrow">OUTPUTS</span>
          {probe.outputs.length ? (
            probe.outputs.map((name) => (
              <div className="midi-port" key={`out:${name}`}>
                <i className="midi-led output" />
                <strong>{name}</strong>
              </div>
            ))
          ) : (
            <small className="inspector-copy">No MIDI output is visible.</small>
          )}
        </div>
      </div>
      <small className="midi-message">{probe.message}</small>
    </section>
  );
}
