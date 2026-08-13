import type {
  AudioChannelInfo,
  AudioDeviceInfo,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioDriverInfo,
  AudioStatus,
  DeviceChannels,
} from '@/model/domain';

export interface RequestedAudioSettings {
  driver: string;
  sampleRate: number;
  bufferSize: number;
}

export interface EffectiveAudioSettings {
  driver: string;
  sampleRate: number | null;
  bufferSize: number | null;
  message: string | null;
}

export const audioSampleRateOptions = [44_100, 48_000, 88_200, 96_000] as const;
export const audioBufferSizeOptions = [64, 128, 256, 512, 1024] as const;

export function includeEffectiveOption(effective: number, options: readonly number[]): number[] {
  return Array.from(new Set([effective, ...options])).sort((left, right) => left - right);
}

function deviceTokens(name: string): Set<string> {
  const ignored = new Set(['audio', 'asio', 'input', 'output', 'channel', 'analogue']);
  return new Set(
    (name.toLocaleLowerCase().match(/[\p{L}\p{N}]+/gu) ?? []).filter(
      (token) => !ignored.has(token),
    ),
  );
}

function relatedDeviceScore(candidate: string, currentDevices: readonly string[]): number {
  const candidateTokens = deviceTokens(candidate);
  return currentDevices.reduce((score, current) => {
    const currentTokens = deviceTokens(current);
    return score + Array.from(candidateTokens).filter((token) => currentTokens.has(token)).length;
  }, 0);
}

function preferredDevice(
  devices: readonly AudioDeviceInfo[],
  current: string | null,
  related: readonly string[],
): string | null {
  if (current != null && devices.some((device) => device.name === current)) return current;
  return devices.reduce<string | null>((best, candidate) => {
    if (best === null) return candidate.name;
    return relatedDeviceScore(candidate.name, related) > relatedDeviceScore(best, related)
      ? candidate.name
      : best;
  }, null);
}

export function chooseInitialDriverRoute(
  driver: AudioDriverInfo,
  currentInput: string | null,
  currentOutput: string | null,
): { inputDevice: string | null; outputDevice: string | null } {
  const currentDevices = [currentInput, currentOutput].filter(
    (device): device is string => device !== null,
  );
  if (driver.devicePairing === 'sameDevice') {
    const outputDevices = new Set(driver.outputs.map((device) => device.name));
    const devices = driver.inputs.filter((device) => outputDevices.has(device.name));
    const selected = devices.reduce<string | null>((best, candidate) => {
      if (candidate.name === currentInput || candidate.name === currentOutput)
        return candidate.name;
      if (best === null) return candidate.name;
      return relatedDeviceScore(candidate.name, currentDevices) >
        relatedDeviceScore(best, currentDevices)
        ? candidate.name
        : best;
    }, null);
    return { inputDevice: selected, outputDevice: selected };
  }
  return {
    inputDevice: preferredDevice(driver.inputs, currentInput, currentDevices),
    outputDevice: preferredDevice(driver.outputs, currentOutput, currentDevices),
  };
}

function findDriver(probe: AudioDeviceProbe, name: string): AudioDriverInfo | null {
  return probe.drivers.find((driver) => driver.name === name) ?? null;
}

function findInputDevice(
  driver: AudioDriverInfo | null,
  name: string | null,
): AudioDeviceInfo | null {
  return driver?.inputs.find((device) => device.name === name) ?? null;
}

/**
 * Fills the channel lists reported by a lazy per-device detail probe into the
 * passive startup device probe. Startup discovery never opens a device, so its
 * channels are empty; Audio Settings resolves them for the selected interface
 * only when the user configures it.
 */
export function mergeDeviceChannels(
  probe: AudioDeviceProbe,
  detail: DeviceChannels,
): AudioDeviceProbe {
  return {
    ...probe,
    drivers: probe.drivers.map((driver) => {
      if (driver.name !== detail.driver) return driver;
      return {
        ...driver,
        inputs: driver.inputs.map((device) =>
          device.name === detail.inputDevice
            ? { ...device, channels: detail.inputChannels }
            : device,
        ),
        outputs: driver.outputs.map((device) =>
          device.name === detail.outputDevice
            ? { ...device, channels: detail.outputChannels }
            : device,
        ),
      };
    }),
  };
}

/**
 * Reuses channel details already reported by the active Audio Runtime.
 * Passive device discovery deliberately leaves channel lists empty, but the
 * device currently opened by Riffra has already supplied these names through
 * AudioStatus and does not need another detail probe.
 */
export function mergeAudioStatusChannels(
  probe: AudioDeviceProbe,
  audio: AudioStatus,
): AudioDeviceProbe {
  if (audio.driver == null) return probe;

  const hasInputChannels = audio.inputDevice != null && audio.inputChannels.length > 0;
  const hasOutputChannels = audio.outputDevice != null && audio.outputChannels.length > 0;
  if (!hasInputChannels && !hasOutputChannels) return probe;

  let changed = false;
  const drivers = probe.drivers.map((driver) => {
    if (driver.name !== audio.driver) return driver;
    return {
      ...driver,
      inputs: hasInputChannels
        ? driver.inputs.map((device) => {
            if (device.name !== audio.inputDevice || device.channels === audio.inputChannels)
              return device;
            changed = true;
            return { ...device, channels: audio.inputChannels };
          })
        : driver.inputs,
      outputs: hasOutputChannels
        ? driver.outputs.map((device) => {
            if (device.name !== audio.outputDevice || device.channels === audio.outputChannels)
              return device;
            changed = true;
            return { ...device, channels: audio.outputChannels };
          })
        : driver.outputs,
    };
  });
  return changed ? { ...probe, drivers } : probe;
}

function hasSelectableDevices(driver: AudioDriverInfo): boolean {
  if (driver.devicePairing === 'sameDevice') {
    return driver.inputs.some((input) =>
      driver.outputs.some((output) => output.name === input.name),
    );
  }
  return driver.inputs.length > 0 && driver.outputs.length > 0;
}

function firstChannel(channels: readonly AudioChannelInfo[]): number {
  return channels[0]?.index ?? 0;
}

function normalizeInputChannel(
  inputChannel: number,
  channels: readonly AudioChannelInfo[],
): number {
  return channels.some((channel) => channel.index === inputChannel)
    ? inputChannel
    : firstChannel(channels);
}

export function createAudioSettingsDraft(
  audio: AudioStatus,
  probe: AudioDeviceProbe,
): AudioDriverConfig {
  const driver =
    (audio.driver == null ? null : findDriver(probe, audio.driver)) ?? probe.drivers[0] ?? null;
  if (driver == null) {
    return {
      driver: audio.driver ?? '',
      inputDevice: audio.inputDevice,
      inputChannel: audio.inputChannel ?? 0,
      outputDevice: audio.outputDevice,
      sampleRate: audio.sampleRate ?? 48_000,
      bufferSize: audio.bufferSize ?? 256,
    };
  }
  const route = chooseInitialDriverRoute(driver, audio.inputDevice, audio.outputDevice);
  const inputInfo = findInputDevice(driver, route.inputDevice);
  const channels =
    inputInfo?.channels ?? (route.inputDevice === audio.inputDevice ? audio.inputChannels : []);
  return {
    driver: driver.name,
    inputDevice: route.inputDevice,
    inputChannel: normalizeInputChannel(audio.inputChannel ?? 0, channels),
    outputDevice: route.outputDevice,
    sampleRate: audio.sampleRate ?? 48_000,
    bufferSize: audio.bufferSize ?? 256,
  };
}

export function selectDriverForDraft(
  draft: AudioDriverConfig,
  driver: AudioDriverInfo,
): AudioDriverConfig {
  const route = chooseInitialDriverRoute(driver, draft.inputDevice, draft.outputDevice);
  const inputInfo = findInputDevice(driver, route.inputDevice);
  return {
    ...draft,
    driver: driver.name,
    inputDevice: route.inputDevice,
    inputChannel: firstChannel(inputInfo?.channels ?? []),
    outputDevice: route.outputDevice,
  };
}

export function normalizeAudioSettingsDraft(
  draft: AudioDriverConfig,
  probe: AudioDeviceProbe,
): AudioDriverConfig {
  const currentDriver = findDriver(probe, draft.driver);
  const driver =
    (currentDriver != null && hasSelectableDevices(currentDriver)
      ? currentDriver
      : probe.drivers.find(hasSelectableDevices)) ??
    currentDriver ??
    probe.drivers[0] ??
    null;
  if (driver == null) {
    return {
      ...draft,
      driver: '',
      inputDevice: null,
      inputChannel: 0,
      outputDevice: null,
    };
  }
  const route = chooseInitialDriverRoute(driver, draft.inputDevice, draft.outputDevice);
  const inputInfo = findInputDevice(driver, route.inputDevice);
  return {
    ...draft,
    driver: driver.name,
    inputDevice: route.inputDevice,
    inputChannel: normalizeInputChannel(draft.inputChannel, inputInfo?.channels ?? []),
    outputDevice: route.outputDevice,
  };
}

export function isAudioSettingsDraftValid(
  draft: AudioDriverConfig,
  probe: AudioDeviceProbe,
): boolean {
  const driver = findDriver(probe, draft.driver);
  if (
    driver == null ||
    draft.inputDevice == null ||
    draft.outputDevice == null ||
    draft.sampleRate == null ||
    draft.bufferSize == null
  )
    return false;
  const input = findInputDevice(driver, draft.inputDevice);
  const output = driver.outputs.find((device) => device.name === draft.outputDevice);
  if (input == null || output == null) return false;
  if (driver.devicePairing === 'sameDevice' && draft.inputDevice !== draft.outputDevice)
    return false;
  return input.channels.some((channel) => channel.index === draft.inputChannel);
}

export function reconcileAudioSettings(
  requested: RequestedAudioSettings,
  status: AudioStatus,
): EffectiveAudioSettings {
  const unavailable = [
    status.sampleRate !== requested.sampleRate
      ? `${requested.sampleRate.toLocaleString()} Hz (using ${status.sampleRate?.toLocaleString() ?? 'unknown'} Hz)`
      : null,
    status.bufferSize !== requested.bufferSize
      ? `${requested.bufferSize} samples (using ${status.bufferSize ?? 'unknown'} samples)`
      : null,
  ].filter((value): value is string => value !== null);

  return {
    driver: status.driver ?? requested.driver,
    sampleRate: status.sampleRate,
    bufferSize: status.bufferSize,
    message:
      unavailable.length > 0
        ? `The driver did not accept ${unavailable.join(' and ')}. Effective settings are selected.`
        : null,
  };
}
