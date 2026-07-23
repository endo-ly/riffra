import type { AudioStatus, MidiProbe } from '@/lib/domain';

export function MidiMonitor({
  probe,
  audio,
  onPanic,
}: {
  probe: MidiProbe;
  audio: AudioStatus;
  onPanic: () => void;
}) {
  return (
    <section className="section-card midi-monitor">
      <header>
        <div>
          <span className="eyebrow">MIDI MONITOR</span>
          <h2>{audio.midiInputActive ? 'Listening' : 'Listening is disabled'}</h2>
        </div>
        <button className="text-button danger" onClick={onPanic}>
          Panic
        </button>
      </header>
      {probe.inputs.length === 0 ? (
        <p className="inspector-copy">No MIDI input port is visible to Windows.</p>
      ) : (
        <ul className="midi-monitor-list">
          {probe.inputs.map((device) => (
            <li key={device.id}>
              <i className={`midi-led${audio.midiInputActive ? '' : ' idle'}`} />
              <strong>{device.name}</strong>
            </li>
          ))}
        </ul>
      )}
      <small className="midi-message">
        Messages {audio.midiMessages} · Last note{' '}
        {audio.lastMidiNote == null ? '—' : audio.lastMidiNote} · Pads {audio.midiPadMappings} ·
        Triggers {audio.midiPadTriggers}
      </small>
    </section>
  );
}
