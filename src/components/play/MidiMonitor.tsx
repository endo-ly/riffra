import type { AudioStatus, MidiProbe } from '@/lib/domain';

export function MidiMonitor({
  probe,
  audio,
  onOpen,
  onClose,
  onPanic,
}: {
  probe: MidiProbe;
  audio: AudioStatus;
  onOpen: (name: string) => void;
  onClose: () => void;
  onPanic: () => void;
}) {
  return (
    <section className="section-card midi-monitor">
      <header>
        <div>
          <span className="eyebrow">MIDI MONITOR</span>
          <h2>{audio.midiInputActive ? 'Listening' : 'Input is closed'}</h2>
        </div>
        <div>
          <button className="text-button danger" onClick={onPanic}>
            Panic
          </button>
          <button className="text-button" disabled={!audio.midiInputActive} onClick={onClose}>
            Close
          </button>
        </div>
      </header>
      {probe.inputs.length === 0 ? (
        <p className="inspector-copy">No MIDI input port is visible to Windows.</p>
      ) : (
        probe.inputs.map((name) => (
          <div className="midi-monitor-row" key={name}>
            <strong>{name}</strong>
            <button
              className="text-button"
              disabled={audio.midiInputActive}
              onClick={() => onOpen(name)}
            >
              Open
            </button>
          </div>
        ))
      )}
      <small className="midi-message">
        Messages {audio.midiMessages} · Last note{' '}
        {audio.lastMidiNote == null ? '—' : audio.lastMidiNote} · Pads {audio.midiPadMappings} ·
        Triggers {audio.midiPadTriggers}
      </small>
      <small className="inspector-copy">
        Unmapped MIDI notes use the bounded offline sine instrument; mapped pads take priority.
      </small>
    </section>
  );
}
