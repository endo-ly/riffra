import type { MidiEvent, MidiNote } from './domain';

export function notesFromMidiEvents(events: MidiEvent[]): MidiNote[] {
  const active = new Map<string, MidiEvent[]>();
  const notes: MidiNote[] = [];
  const ordered = [...events].sort((left, right) => left.timeMs - right.timeMs);
  const finish = (start: MidiEvent, endMs: number) =>
    notes.push({
      id: `midi-note:${notes.length}`,
      note: start.note,
      startMs: Math.max(0, Math.round(start.timeMs)),
      durationMs: Math.max(1, Math.round(endMs - start.timeMs)),
      velocity: Math.max(1, Math.min(127, start.velocity)),
      channel: Math.max(1, Math.min(16, start.channel)),
    });
  for (const event of ordered) {
    const key = `${event.channel}:${event.note}`;
    const kind = event.status & 0xf0;
    if (kind === 0x90 && event.velocity > 0) {
      active.set(key, [...(active.get(key) ?? []), event]);
    } else if (kind === 0x80 || kind === 0x90) {
      const stack = active.get(key);
      const start = stack?.pop();
      if (start) finish(start, event.timeMs);
      if (!stack?.length) active.delete(key);
    }
  }
  const endMs = ordered.reduce((latest, event) => Math.max(latest, event.timeMs), 0) + 100;
  for (const stack of active.values()) for (const start of stack) finish(start, endMs);
  return notes.sort((left, right) => left.startMs - right.startMs || left.note - right.note);
}
