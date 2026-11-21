import type { CreativeSession } from '@/model/generated';

export function describeCountIn(session: CreativeSession): string {
  const beats = session.settings.countInBeats;
  if (!beats) return 'Off';
  const beatsPerBar = session.arrangement.timebase.timeSignatureChanges[0].numerator;
  if (beats >= beatsPerBar * 2) return '2 Bars';
  if (beats >= beatsPerBar) return '1 Bar';
  return String(beats);
}

export function countInBadge(session: CreativeSession): string {
  const beats = session.settings.countInBeats;
  const beatsPerBar = session.arrangement.timebase.timeSignatureChanges[0].numerator;
  return beats % beatsPerBar === 0 ? String(beats / beatsPerBar) : `${beats}b`;
}

export function nextCountInBeats(session: CreativeSession): number {
  const beatsPerBar = session.arrangement.timebase.timeSignatureChanges[0].numerator;
  const current = session.settings.countInBeats;
  if (current === 0) return beatsPerBar;
  if (current < beatsPerBar * 2) return beatsPerBar * 2;
  return 0;
}
