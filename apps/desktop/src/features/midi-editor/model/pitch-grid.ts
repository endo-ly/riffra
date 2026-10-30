export const PITCH_HIGH = 128;
export const PITCH_LOW = 0;

export function pitchFromClientY(
  clientY: number,
  boundsTop: number,
  rowHeight: number,
  clampToRange = false,
) {
  const row = Math.floor((clientY - boundsTop) / rowHeight);
  const pitch = PITCH_HIGH - 1 - row;
  if (clampToRange) return Math.max(PITCH_LOW, Math.min(PITCH_HIGH - 1, pitch));
  return pitch >= PITCH_LOW && pitch < PITCH_HIGH ? pitch : null;
}

export function pitchRowTop(pitch: number, rowHeight: number) {
  return (PITCH_HIGH - pitch - 1) * rowHeight;
}
