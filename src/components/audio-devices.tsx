import type { AudioDeviceProbe, AudioStatus, MidiProbe, Session } from '@/lib/domain';
import { includeEffectiveOption } from '@/lib/audio-settings';

export function AudioDevices({
  probe,
  onRefresh,
}: {
  probe: AudioDeviceProbe;
  onRefresh: () => void;
}) {
  return (
    <section className="section-card audio-device-list">
      <header>
        <div>
          <span className="eyebrow">WINDOWS DEVICES</span>
          <h2>WASAPI / ASIO / MIDI</h2>
        </div>
        <button className="text-button" onClick={onRefresh}>
          Refresh
        </button>
      </header>
      {probe.drivers.length === 0 ? (
        <p className="inspector-copy">No audio driver list is available yet.</p>
      ) : (
        probe.drivers.map((driver) => (
          <div className="audio-driver-row" key={driver.name}>
            <strong>{driver.name}</strong>
            <small>
              {driver.inputs.length} inputs · {driver.outputs.length} outputs
            </small>
          </div>
        ))
      )}
      <small className="device-probe-message">
        {probe.message} · MIDI {probe.midiInputs.length} in / {probe.midiOutputs.length} out
      </small>
    </section>
  );
}

export function AudioDriverPicker({
  probe,
  current,
  sampleRate,
  bufferSize,
  onSelect,
}: {
  probe: AudioDeviceProbe;
  current: string | null;
  sampleRate: number | null;
  bufferSize: number | null;
  onSelect: (driver: string, sampleRate: number, bufferSize: number) => void;
}) {
  if (!probe.drivers.length) return null;
  const activeDriver = current ?? probe.drivers[0].name;
  const selectedRate = sampleRate ?? 48_000;
  const selectedBuffer = bufferSize ?? 256;
  const rateOptions = includeEffectiveOption(selectedRate, [44_100, 48_000, 88_200, 96_000]);
  const bufferOptions = includeEffectiveOption(selectedBuffer, [64, 128, 256, 512, 1024]);
  return (
    <section className="section-card audio-driver-picker">
      <header>
        <div>
          <span className="eyebrow">DRIVER ROUTING</span>
          <h2>Choose a safe audio backend</h2>
        </div>
        <small>Switching re-enters emergency mute</small>
      </header>
      <div className="audio-format-picker">
        <fieldset>
          <legend>Sample rate</legend>
          <div className="audio-format-options">
            {rateOptions.map((rate) => (
              <button
                className={rate === selectedRate ? 'active' : ''}
                aria-pressed={rate === selectedRate}
                onClick={() => onSelect(activeDriver, rate, selectedBuffer)}
                key={rate}
              >
                {rate.toLocaleString()} Hz
              </button>
            ))}
          </div>
        </fieldset>
        <fieldset>
          <legend>Buffer</legend>
          <div className="audio-format-options">
            {bufferOptions.map((buffer) => (
              <button
                className={buffer === selectedBuffer ? 'active' : ''}
                aria-pressed={buffer === selectedBuffer}
                onClick={() => onSelect(activeDriver, selectedRate, buffer)}
                key={buffer}
              >
                {buffer} samples
              </button>
            ))}
          </div>
        </fieldset>
      </div>
      <div className="driver-picker-grid">
        {probe.drivers.map((driver) => (
          <button
            className={`driver-choice ${driver.name === current ? 'active' : ''}`}
            key={driver.name}
            onClick={() => onSelect(driver.name, selectedRate, selectedBuffer)}
          >
            <strong>{driver.name}</strong>
            <small>{driver.name === current ? 'Current' : 'Use this driver'}</small>
          </button>
        ))}
      </div>
    </section>
  );
}

export function CaptureSettings({
  session,
  setSession,
}: {
  session: Session;
  setSession: (value: Session) => void;
}) {
  return (
    <section className="section-card capture-settings">
      <header>
        <div>
          <span className="eyebrow">CAPTURE</span>
          <h2>Quick Record timing</h2>
        </div>
        <small>Stored with this Scratch Session</small>
      </header>
      <label>
        <span>Visual count-in</span>
        <select
          value={session.countInBeats}
          onChange={(event) => setSession({ ...session, countInBeats: Number(event.target.value) })}
        >
          {[0, 1, 2, 3, 4, 8].map((beats) => (
            <option value={beats} key={beats}>
              {beats === 0 ? 'Off' : `${beats} beats`}
            </option>
          ))}
        </select>
      </label>
      <p className="inspector-copy">
        When enabled, recording starts after the countdown. Existing audio is never captured during
        the count-in.
      </p>
    </section>
  );
}

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
