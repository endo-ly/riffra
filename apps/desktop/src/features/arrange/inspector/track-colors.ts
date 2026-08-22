export const TRACK_COLOR_PALETTE: readonly string[] = [
  '#7eb8ff',
  '#7ee0a0',
  '#ff9f6b',
  '#ffd166',
  '#c79eff',
  '#ff7eb0',
  '#64e8ff',
  '#a8b0bf',
] as const;

/** Automatic coloring is a presentation concern: explicit track colors win,
 * otherwise the track's position picks a palette entry. */
export function resolveTrackColor(track: { color?: string | null }, index: number): string {
  return track.color ?? TRACK_COLOR_PALETTE[Math.max(0, index) % TRACK_COLOR_PALETTE.length];
}
