import { clamp01 } from './math';
import { midiToLabel } from './format';
import type {
  VisualChord,
  VisualMeter,
  VisualNote,
  VisualSnapshot,
  VisualSection,
  VisualTempo,
  VisualTrack,
} from './generated';

/** The chords shown around the playhead. */
export interface ChordWindow {
  /** At most `count` consecutive chords around the sounding one. */
  chords: VisualChord[];
  /** Index of the sounding chord inside `chords`; `-1` when none sounds. */
  current: number;
  /** Elapsed fraction of the sounding chord, `0..1`. */
  progress: number;
}

/** The section sounding at one instant and the one before it. */
export interface SectionWindow {
  current: VisualSection | null;
  previous: VisualSection | null;
}

/** A playhead position on the step grid of its bar. */
export interface BeatGridPosition {
  bar: number;
  beat: number;
  step: number;
  stepsPerBar: number;
}

/** One deterministic attack lane of the rhythm view. */
export interface PulseLane {
  id: string;
  name: string;
  hits: PulseHit[];
}

export interface PulseHit {
  t: number;
  accent: boolean;
}

/** Categories in the order non-drum pulse lanes are chosen. */
const PULSE_PRIORITY = ['bass', 'chords', 'arp', 'lead', 'vocal'] as const;
/** Velocity at which an attack counts as an accent. */
const ACCENT_VELOCITY = 110;
/** Maximum number of non-drum pulse lanes. */
const MAX_PULSE_LANES = 4;

function lastIndexAtOrBefore<T>(
  items: readonly T[],
  start: (item: T) => number,
  timeSec: number,
): number {
  let low = 0;
  let high = items.length - 1;
  let found = -1;
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    if (start(items[middle]) <= timeSec) {
      found = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found;
}

/** Returns the first beat index at or after `targetSec`. */
export function findFirstBeatAtOrAfter(
  beats: readonly { readonly t: number }[],
  targetSec: number,
): number {
  let low = 0;
  let high = beats.length;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (beats[mid].t < targetSec) {
      low = mid + 1;
    } else {
      high = mid;
    }
  }
  return low;
}

/** Returns the tempo in effect at `timeSec`. */
export function findTempoAt(snapshot: VisualSnapshot, timeSec: number): VisualTempo | null {
  const index = lastIndexAtOrBefore(snapshot.tempos, (tempo) => tempo.t, timeSec);
  return index < 0 ? null : snapshot.tempos[index];
}

/** Returns the meter in effect at `timeSec`. */
export function findMeterAt(snapshot: VisualSnapshot, timeSec: number): VisualMeter | null {
  const index = lastIndexAtOrBefore(snapshot.meters, (meter) => meter.t, timeSec);
  return index < 0 ? null : snapshot.meters[index];
}

/** Returns the section sounding at `timeSec` and its predecessor. */
export function findSectionAt(snapshot: VisualSnapshot, timeSec: number): SectionWindow {
  const index = lastIndexAtOrBefore(snapshot.sections, (section) => section.startSec, timeSec);
  return {
    current: index < 0 ? null : snapshot.sections[index],
    previous: index < 1 ? null : snapshot.sections[index - 1],
  };
}

/** Returns the sounding chord centred in a window of at most `count` chords. */
export function findChordWindow(snapshot: VisualSnapshot, timeSec: number, count = 4): ChordWindow {
  const chords = snapshot.chords;
  const index = lastIndexAtOrBefore(chords, (chord) => chord.startSec, timeSec);
  const sounding = index >= 0 && timeSec < chords[index].endSec;
  const center = Math.max(index, 0);
  const start = Math.max(0, Math.min(center - Math.floor((count - 1) / 2), chords.length - count));
  const visible = chords.slice(start, start + count);
  const progress = sounding
    ? clamp01((timeSec - chords[index].startSec) / (chords[index].endSec - chords[index].startSec))
    : 0;
  return { chords: visible, current: sounding ? index - start : -1, progress };
}

/** Returns the roots of the most recent chords, oldest first. */
export function findRecentChordRoots(
  snapshot: VisualSnapshot,
  timeSec: number,
  maxCount = 4,
): number[] {
  const index = lastIndexAtOrBefore(snapshot.chords, (chord) => chord.startSec, timeSec);
  const roots: number[] = [];
  for (let cursor = index; cursor >= 0 && roots.length < maxCount; cursor -= 1) {
    const rootPc = snapshot.chords[cursor].rootPc;
    if (rootPc !== undefined) roots.unshift(rootPc);
  }
  return roots;
}

/** Maps `timeSec` onto the bar, beat, and step grid it falls in. */
export function resolveBeatGridPosition(
  snapshot: VisualSnapshot,
  timeSec: number,
): BeatGridPosition {
  const index = lastIndexAtOrBefore(snapshot.beats, (beat) => beat.t, timeSec);
  if (index < 0) {
    const meter = findMeterAt(snapshot, timeSec);
    return { bar: 1, beat: 1, step: 0, stepsPerBar: stepsPerBarOf(meter) };
  }
  const beat = snapshot.beats[index];
  const meter = findMeterAt(snapshot, beat.t);
  const stepsPerBar = stepsPerBarOf(meter);
  const stepsPerBeat = stepsPerBeatOf(meter);
  const length = beatLength(snapshot, index);
  const withinBeat = length > 0 ? clamp01((timeSec - beat.t) / length) : 0;
  const step = Math.floor((beat.beat - 1 + withinBeat) * stepsPerBeat);
  return {
    bar: beat.bar,
    beat: beat.beat,
    step: Math.min(Math.max(step, 0), stepsPerBar - 1),
    stepsPerBar,
  };
}

function stepsPerBarOf(meter: VisualMeter | null): number {
  if (!meter) return 16;
  return Math.max(1, Math.round((meter.numerator * 16) / meter.denominator));
}

function stepsPerBeatOf(meter: VisualMeter | null): number {
  return meter ? 16 / meter.denominator : 4;
}

function beatLength(snapshot: VisualSnapshot, index: number): number {
  const beat = snapshot.beats[index];
  const next = snapshot.beats[index + 1];
  if (next && next.t > beat.t) return next.t - beat.t;
  const previous = snapshot.beats[index - 1];
  if (previous && beat.t > previous.t) return beat.t - previous.t;
  const tempo = findTempoAt(snapshot, beat.t);
  const meter = findMeterAt(snapshot, beat.t);
  if (!tempo) return 0;
  return (60 / tempo.bpm) * (4 / (meter?.denominator ?? 4));
}

/**
 * Builds the rhythm lanes drawn for a snapshot.
 *
 * Drum Tracks keep one lane per output pitch. Other Tracks contribute attack
 * lanes derived from their surviving notes and sustain ranges.
 */
export function derivePulseLanes(snapshot: VisualSnapshot): PulseLane[] {
  if (snapshot.drumLanes.length > 0) return drumPulseLanes(snapshot);
  const tracks = snapshot.tracks.filter((track) => track.category !== 'drums');
  if (tracks.length === 1) return singleTrackPulseLanes(tracks[0]);
  return trackPulseLanes(tracks);
}

function drumPulseLanes(snapshot: VisualSnapshot): PulseLane[] {
  return snapshot.drumLanes.map((lane) => {
    const track = snapshot.tracks.find((candidate) => candidate.id === lane.trackId);
    const fallback = track ? `${track.name} ${midiToLabel(lane.pitch)}` : midiToLabel(lane.pitch);
    return {
      id: `${lane.trackId}:${lane.pitch}`,
      name: lane.label ?? fallback,
      hits: lane.hits.map((hit) => ({ t: hit.t, accent: hit.accent })),
    };
  });
}

function singleTrackPulseLanes(track: VisualTrack): PulseLane[] {
  const lanes: PulseLane[] = [
    {
      id: `${track.id}:melody`,
      name: 'MELODY',
      hits: attackHits(track.notes.filter((note) => note.top)),
    },
  ];
  const accompaniment = track.notes.filter((note) => !note.top);
  if (accompaniment.length > 0) {
    lanes.push({ id: `${track.id}:accomp`, name: 'ACCOMP', hits: attackHits(accompaniment) });
  }
  if (track.sustain.length > 0) {
    lanes.push({
      id: `${track.id}:pedal`,
      name: 'PEDAL',
      hits: track.sustain.map((sustain) => ({ t: sustain.startSec, accent: false })),
    });
  }
  return lanes;
}

function trackPulseLanes(tracks: VisualTrack[]): PulseLane[] {
  const ordered = PULSE_PRIORITY.flatMap((category) =>
    tracks.filter((track) => track.category === category),
  );
  return ordered.slice(0, MAX_PULSE_LANES).map((track) => ({
    id: track.id,
    name: track.name,
    hits: attackHits(track.notes),
  }));
}

function attackHits(notes: readonly VisualNote[]): PulseHit[] {
  const velocities = new Map<number, number>();
  for (const note of notes) {
    velocities.set(note.startSec, Math.max(velocities.get(note.startSec) ?? 0, note.velocity));
  }
  return [...velocities.entries()]
    .sort(([left], [right]) => left - right)
    .map(([t, velocity]) => ({ t, accent: velocity >= ACCENT_VELOCITY }));
}
