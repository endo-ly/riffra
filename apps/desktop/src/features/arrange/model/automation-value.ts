import type { AutomationInterpolation, AutomationPoint } from '@/model/domain';

export function automationValueAt(
  points: readonly AutomationPoint[],
  interpolation: AutomationInterpolation,
  tick: number,
  fallback: number,
): number {
  if (points.length === 0) return fallback;
  if (tick <= points[0].tick) return points[0].value;
  for (let index = 1; index < points.length; index += 1) {
    const right = points[index];
    if (tick >= right.tick) continue;
    const left = points[index - 1];
    if (interpolation === 'step') return left.value;
    const amount = (tick - left.tick) / (right.tick - left.tick);
    return left.value + (right.value - left.value) * amount;
  }
  return points.at(-1)!.value;
}
