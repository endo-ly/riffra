import type { AudioStatus } from "./domain";

export type RequestedAudioSettings = {
  driver: string;
  sampleRate: number;
  bufferSize: number;
};

export type EffectiveAudioSettings = {
  driver: string;
  sampleRate: number | null;
  bufferSize: number | null;
  message: string | null;
};

export function includeEffectiveOption(effective: number, options: readonly number[]): number[] {
  return Array.from(new Set([effective, ...options])).sort((left, right) => left - right);
}

export function reconcileAudioSettings(requested: RequestedAudioSettings, status: AudioStatus): EffectiveAudioSettings {
  const unavailable = [
    status.sampleRate !== requested.sampleRate
      ? `${requested.sampleRate.toLocaleString()} Hz (using ${status.sampleRate?.toLocaleString() ?? "unknown"} Hz)`
      : null,
    status.bufferSize !== requested.bufferSize
      ? `${requested.bufferSize} samples (using ${status.bufferSize ?? "unknown"} samples)`
      : null,
  ].filter((value): value is string => value !== null);

  return {
    driver: status.driver ?? requested.driver,
    sampleRate: status.sampleRate,
    bufferSize: status.bufferSize,
    message: unavailable.length > 0
      ? `The driver did not accept ${unavailable.join(" and ")}. Effective settings are selected.`
      : null,
  };
}
