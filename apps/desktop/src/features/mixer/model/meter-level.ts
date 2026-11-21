/** Linear amplitude as dBFS; silence and negative peaks read as −∞. */
export function dbfs(value: number): number {
  return value > 0 ? 20 * Math.log10(value) : -Infinity;
}

/** Position of a linear peak on the meter's −60 dB … +6 dB scale. */
export function levelPercent(value: number): number {
  const db = dbfs(value);
  if (!Number.isFinite(db)) return 0;
  return Math.max(0, Math.min(100, ((db + 60) / 66) * 100));
}

/** Formats a linear amplitude as dBFS, showing `floorDb` and below as −∞. */
export function formatMeterLevel(value: number, floorDb = Number.NEGATIVE_INFINITY): string {
  const db = dbfs(value);
  if (!Number.isFinite(db) || db <= floorDb) return '−∞';
  return `${db >= 0 ? '+' : ''}${db.toFixed(1)}`;
}
