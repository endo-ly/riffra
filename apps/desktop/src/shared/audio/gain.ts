/** Formats a channel gain for display; gains at or below -90 dB read as silent. */
export function formatGainDb(gainDb: number): string {
  if (gainDb <= -90) return '−∞ dB';
  return `${gainDb >= 0 ? '+' : ''}${gainDb.toFixed(1)} dB`;
}
