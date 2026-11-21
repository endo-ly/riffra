/** Returns the last segment of a path, ignoring separators at its end. */
export function fileName(path: string | null): string | null {
  if (!path) return null;
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? null;
}
