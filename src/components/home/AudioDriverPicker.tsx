import type { AudioDeviceProbe } from '@/lib/domain';
import { includeEffectiveOption } from '@/lib/audio-settings';

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
