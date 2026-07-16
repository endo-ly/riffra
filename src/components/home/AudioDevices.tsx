import type { AudioDeviceProbe } from '@/lib/domain';

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
