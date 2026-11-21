export interface MixRange {
  min: number;
  max: number;
  step: number;
  /** Value restored by a double-click on the control. */
  defaultValue: number;
}

export const MIX_GAIN_DB: MixRange = { min: -90, max: 24, step: 0.1, defaultValue: 0 };
export const MASTER_GAIN_DB: MixRange = { min: -90, max: 0, step: 0.1, defaultValue: 0 };
export const MIX_PAN: MixRange = { min: -1, max: 1, step: 0.01, defaultValue: 0 };

/** Formats a channel gain for display; gains at or below -90 dB read as silent. */
export function formatGainDb(gainDb: number): string {
  if (gainDb <= -90) return '−∞ dB';
  return `${gainDb >= 0 ? '+' : ''}${gainDb.toFixed(1)} dB`;
}

/** Formats a pan position as C or a side with its percentage, e.g. `L 35`. */
export function formatPan(pan: number): string {
  if (Math.abs(pan) < 0.005) return 'C';
  return `${pan < 0 ? 'L' : 'R'} ${Math.round(Math.abs(pan) * 100)}`;
}
