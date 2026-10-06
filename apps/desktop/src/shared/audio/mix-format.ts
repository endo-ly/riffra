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
