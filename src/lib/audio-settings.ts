import type { AudioDriverInfo, AudioStatus } from './domain';

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

export function chooseInitialDriverRoute(
  driver: AudioDriverInfo,
  currentInput: string | null,
  currentOutput: string | null,
): { inputDevice: string | null; outputDevice: string | null } {
  if (driver.devicePairing === 'sameDevice') {
    const outputDevices = new Set(driver.outputs);
    const devices = driver.inputs.filter((device) => outputDevices.has(device));
    const currentDevices = [currentInput, currentOutput].filter(
      (device): device is string => device !== null,
    );
    const selected = devices.reduce<string | null>((best, candidate) => {
      if (best === null) return candidate;
      return relatedDeviceScore(candidate, currentDevices) >
        relatedDeviceScore(best, currentDevices)
        ? candidate
        : best;
    }, null);
    return { inputDevice: selected, outputDevice: selected };
  }
  return {
    inputDevice: driver.inputs[0] ?? null,
    outputDevice: driver.outputs[0] ?? null,
  };
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
