import type { AudioChannelInfo, AudioDeviceProbe } from '@/lib/domain';
import { chooseInitialDriverRoute, includeEffectiveOption } from '@/lib/audio-settings';

export function AudioDriverPicker({
  probe,
  current,
  inputDevice,
  inputChannel,
  inputChannels,
  outputDevice,
  sampleRate,
  bufferSize,
  onSelect,
}: {
  probe: AudioDeviceProbe;
  current: string | null;
  inputDevice: string | null;
  inputChannel: number | null;
  inputChannels: AudioChannelInfo[];
  outputDevice: string | null;
  sampleRate: number | null;
  bufferSize: number | null;
  onSelect: (
    driver: string,
    inputDevice: string | null,
    inputChannel: number,
    outputDevice: string | null,
    sampleRate: number,
    bufferSize: number,
  ) => void;
}) {
  if (!probe.drivers.length) return null;
  const activeDriver = current ?? probe.drivers[0].name;
  const activeDriverInfo =
    probe.drivers.find((driver) => driver.name === activeDriver) ?? probe.drivers[0];
  const pairedDevices = activeDriverInfo.inputs.filter((device) =>
    activeDriverInfo.outputs.includes(device),
  );
  const selectedRate = sampleRate ?? 48_000;
  const selectedBuffer = bufferSize ?? 256;
  const rateOptions = includeEffectiveOption(selectedRate, [44_100, 48_000, 88_200, 96_000]);
  const bufferOptions = includeEffectiveOption(selectedBuffer, [64, 128, 256, 512, 1024]);
  const accessDescription = {
    shared: 'Shared with other Windows applications',
    exclusive: 'Exclusive: other applications using this device will pause',
    driverManaged: 'Sharing depends on the selected driver',
  }[activeDriverInfo.accessMode];
  const select = (
    driver: string,
    nextInput: string | null,
    nextInputChannel: number,
    nextOutput: string | null,
    rate = selectedRate,
    buffer = selectedBuffer,
  ) => onSelect(driver, nextInput, nextInputChannel, nextOutput, rate, buffer);
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
                onClick={() =>
                  select(activeDriver, inputDevice, inputChannel ?? 0, outputDevice, rate)
                }
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
                onClick={() =>
                  select(
                    activeDriver,
                    inputDevice,
                    inputChannel ?? 0,
                    outputDevice,
                    selectedRate,
                    buffer,
                  )
                }
                key={buffer}
              >
                {buffer} samples
              </button>
            ))}
          </div>
        </fieldset>
      </div>
      <div className={`audio-access-note ${activeDriverInfo.accessMode}`}>{accessDescription}</div>
      <div className="driver-picker-grid">
        {probe.drivers.map((driver) => (
          <button
            className={`driver-choice ${driver.name === current ? 'active' : ''}`}
            key={driver.name}
            onClick={() => {
              const route = chooseInitialDriverRoute(driver, inputDevice, outputDevice);
              select(driver.name, route.inputDevice, 0, route.outputDevice);
            }}
          >
            <strong>{driver.name}</strong>
            <small>
              {driver.name === current ? 'Current' : 'Use this driver'} ·{' '}
              {
                {
                  shared: 'Shared',
                  exclusive: 'Exclusive',
                  driverManaged: 'Driver controlled',
                }[driver.accessMode]
              }
            </small>
          </button>
        ))}
      </div>
      <div className="audio-device-selectors">
        {activeDriverInfo.devicePairing === 'sameDevice' ? (
          <label>
            Audio device
            <select
              aria-label="Audio device"
              value={inputDevice === outputDevice ? (inputDevice ?? '') : ''}
              onChange={(event) => {
                const device = event.target.value || null;
                select(activeDriver, device, 0, device);
              }}
            >
              {pairedDevices.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <label>
            Input device
            <select
              aria-label="Input device"
              value={inputDevice ?? ''}
              onChange={(event) =>
                select(activeDriver, event.target.value || null, 0, outputDevice)
              }
            >
              <option value="">System default</option>
              {activeDriverInfo.inputs.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          Input channel
          <select
            aria-label="Input channel"
            value={inputChannel ?? ''}
            disabled={inputChannels.length === 0}
            onChange={(event) =>
              select(activeDriver, inputDevice, Number(event.target.value), outputDevice)
            }
          >
            {inputChannels.length === 0 && <option value="">No active input channels</option>}
            {inputChannels.map((channel) => (
              <option key={channel.index} value={channel.index}>
                {channel.name}
              </option>
            ))}
          </select>
        </label>
        {activeDriverInfo.devicePairing === 'independent' && (
          <label>
            Output device
            <select
              aria-label="Output device"
              value={outputDevice ?? ''}
              onChange={(event) =>
                select(activeDriver, inputDevice, inputChannel ?? 0, event.target.value || null)
              }
            >
              <option value="">System default</option>
              {activeDriverInfo.outputs.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
    </section>
  );
}
