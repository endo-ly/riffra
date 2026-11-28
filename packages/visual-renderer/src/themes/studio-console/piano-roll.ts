import { midiToLabel } from '../../format';
import type { VisualNote, VisualSnapshot, VisualSustain, VisualTrack } from '../../generated';
import { clamp01, expDecay } from '../../math';
import type { ColorMode, ViewState } from '../../types';
import { interpolateVelocityColor, STUDIO_PALETTE, type TrackColorStyle } from './palette';

const MAX_LEGEND_ITEMS = 6;
const BLACK_KEY_PCS = new Set([1, 3, 6, 8, 10]);

interface LegendItemNodes {
  item: HTMLSpanElement;
  swatch: HTMLSpanElement;
  label: HTMLSpanElement;
}

interface IndexedNote {
  note: VisualNote;
  trackIndex: number;
  category: VisualTrack['category'];
}

interface SnapshotCache {
  snapshot: VisualSnapshot;
  nonDrumTracks: VisualTrack[];
  notes: IndexedNote[];
  sustains: VisualSustain[];
  minMidi: number;
  maxMidi: number;
  isSoloEmphasis: boolean;
}

/**
 * Canvas 2D scrolling piano roll with CC64 sustain sub-lane, deterministic
 * note illumination, and click-to-seek support.
 */
export class PianoRollView {
  readonly element: HTMLElement;
  private readonly canvasWrap: HTMLDivElement;
  private readonly canvas: HTMLCanvasElement;
  private readonly legendContainer: HTMLDivElement;
  private readonly legendItems: LegendItemNodes[] = [];

  private cache: SnapshotCache | null = null;
  private lastLegendKey = '';
  private lastTimeSec = 0;
  private lastDurationSec = 0;
  private currentOnSeek: ((timeSec: number) => void) | undefined;

  private readonly handleClick: (event: MouseEvent) => void;

  constructor(doc: Document) {
    this.element = doc.createElement('section');
    this.element.className = 'sc-panel sc-piano-panel';
    this.element.setAttribute('data-role', 'piano-roll');

    const header = doc.createElement('div');
    header.className = 'sc-panel-header';

    const title = doc.createElement('span');
    title.className = 'sc-panel-title';
    title.textContent = 'PIANO ROLL';

    this.legendContainer = doc.createElement('div');
    this.legendContainer.className = 'sc-legend';
    this.legendContainer.setAttribute('data-role', 'piano-legend');

    for (let index = 0; index < MAX_LEGEND_ITEMS; index += 1) {
      const item = doc.createElement('span');
      item.className = 'sc-legend-item';
      item.hidden = true;

      const swatch = doc.createElement('span');
      swatch.className = 'sc-legend-swatch';

      const label = doc.createElement('span');
      item.append(swatch, label);
      this.legendContainer.appendChild(item);
      this.legendItems.push({ item, swatch, label });
    }

    header.append(title, this.legendContainer);

    this.canvasWrap = doc.createElement('div');
    this.canvasWrap.className = 'sc-piano-canvas-wrap';

    this.canvas = doc.createElement('canvas');
    this.canvas.className = 'sc-piano-canvas';
    this.canvas.dataset.seekable = 'false';
    this.canvasWrap.appendChild(this.canvas);

    this.element.append(header, this.canvasWrap);

    this.handleClick = (event: MouseEvent) => {
      const onSeek = this.currentOnSeek;
      if (!onSeek) return;
      const rect = this.canvas.getBoundingClientRect();
      if (rect.width <= 0) return;
      const ratio = (event.clientX - rect.left) / rect.width;
      const windowSec = STUDIO_PALETTE.pianoRoll.windowSec;
      const windowStart = this.lastTimeSec - windowSec * STUDIO_PALETTE.pianoRoll.playheadRatio;
      const rawTime = windowStart + ratio * windowSec;
      const clamped = Math.min(Math.max(rawTime, 0), Math.max(0, this.lastDurationSec));
      onSeek(clamped);
    };

    this.canvas.addEventListener('click', this.handleClick);
  }

  updateSnapshot(snapshot: VisualSnapshot): void {
    const nonDrumTracks = snapshot.tracks.filter((track) => track.category !== 'drums');
    const notes: IndexedNote[] = [];
    const sustains: VisualSustain[] = [];
    let minNoteMidi = Number.POSITIVE_INFINITY;
    let maxNoteMidi = Number.NEGATIVE_INFINITY;

    for (let trackIndex = 0; trackIndex < nonDrumTracks.length; trackIndex += 1) {
      const track = nonDrumTracks[trackIndex];
      for (const note of track.notes) {
        notes.push({ note, trackIndex, category: track.category });
        if (note.midi < minNoteMidi) minNoteMidi = note.midi;
        if (note.midi > maxNoteMidi) maxNoteMidi = note.midi;
      }
      for (const sustain of track.sustain) {
        sustains.push(sustain);
      }
    }

    const { defaultMinMidi, defaultMaxMidi, pitchPadding, minPitchSpan } = STUDIO_PALETTE.pianoRoll;
    let minMidi: number = defaultMinMidi;
    let maxMidi: number = defaultMaxMidi;
    if (notes.length > 0) {
      minMidi = Math.max(0, minNoteMidi - pitchPadding);
      maxMidi = Math.min(127, maxNoteMidi + pitchPadding);
      const span = maxMidi - minMidi;
      if (span < minPitchSpan) {
        const deficit = minPitchSpan - span;
        const padLow = Math.floor(deficit / 2);
        const padHigh = deficit - padLow;
        minMidi = Math.max(0, minMidi - padLow);
        maxMidi = Math.min(127, maxMidi + padHigh);
      }
    }

    const isSoloEmphasis =
      nonDrumTracks.length === 1 && nonDrumTracks[0].notes.some((note) => !note.top);

    this.cache = {
      snapshot,
      nonDrumTracks,
      notes,
      sustains,
      minMidi,
      maxMidi,
      isSoloEmphasis,
    };
    this.lastLegendKey = '';
  }

  render(snapshot: VisualSnapshot, timeSec: number, state: ViewState): void {
    if (!this.cache || this.cache.snapshot !== snapshot) {
      this.updateSnapshot(snapshot);
    }
    const cache = this.cache;
    if (!cache) return;

    this.lastTimeSec = timeSec;
    this.lastDurationSec = snapshot.durationSec;
    this.currentOnSeek = state.onSeek;
    this.canvas.dataset.seekable = state.onSeek ? 'true' : 'false';

    this.syncLegend(state.colorMode, cache.nonDrumTracks);

    const ctx = this.canvas.getContext('2d');
    if (!ctx) return;

    const cssWidth = Math.max(1, Math.round(this.canvasWrap.clientWidth || 640));
    const cssHeight = Math.max(1, Math.round(this.canvasWrap.clientHeight || 260));
    const dpr = Math.max(1, typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1);
    const backingWidth = Math.round(cssWidth * dpr);
    const backingHeight = Math.round(cssHeight * dpr);

    if (this.canvas.width !== backingWidth || this.canvas.height !== backingHeight) {
      this.canvas.width = backingWidth;
      this.canvas.height = backingHeight;
    }

    ctx.save();
    ctx.scale(dpr, dpr);
    this.drawFrame(ctx, cssWidth, cssHeight, cache, timeSec, state.colorMode);
    ctx.restore();
  }

  unmount(): void {
    this.canvas.removeEventListener('click', this.handleClick);
    this.currentOnSeek = undefined;
    this.cache = null;
  }

  private syncLegend(colorMode: ColorMode, tracks: readonly VisualTrack[]): void {
    const trackNames = tracks
      .slice(0, MAX_LEGEND_ITEMS)
      .map((track) => track.name)
      .join('|');
    const key = `${colorMode}:${trackNames}`;
    if (key === this.lastLegendKey) return;
    this.lastLegendKey = key;

    const entries: { label: string; color: string }[] = [];
    if (colorMode === 'harmony') {
      entries.push(
        { label: STUDIO_PALETTE.harmony.root.label, color: STUDIO_PALETTE.harmony.root.fill },
        {
          label: STUDIO_PALETTE.harmony.chordTone.label,
          color: STUDIO_PALETTE.harmony.chordTone.fill,
        },
        { label: STUDIO_PALETTE.harmony.tension.label, color: STUDIO_PALETTE.harmony.tension.fill },
        {
          label: STUDIO_PALETTE.harmony.nonChord.label,
          color: STUDIO_PALETTE.harmony.nonChord.fill,
        },
      );
    } else if (colorMode === 'velocity') {
      for (const stop of STUDIO_PALETTE.velocityLegend) {
        entries.push({ label: stop.label, color: stop.color });
      }
    } else {
      for (let index = 0; index < Math.min(tracks.length, MAX_LEGEND_ITEMS); index += 1) {
        const style = STUDIO_PALETTE.tracks[index % STUDIO_PALETTE.tracks.length];
        entries.push({ label: tracks[index].name, color: style.fill });
      }
    }

    for (let index = 0; index < this.legendItems.length; index += 1) {
      const node = this.legendItems[index];
      const entry = entries[index];
      if (!entry) {
        node.item.hidden = true;
        continue;
      }
      node.item.hidden = false;
      node.swatch.style.backgroundColor = entry.color;
      node.label.textContent = entry.label;
    }
  }

  private drawFrame(
    ctx: CanvasRenderingContext2D,
    width: number,
    height: number,
    cache: SnapshotCache,
    timeSec: number,
    colorMode: ColorMode,
  ): void {
    const { pianoRoll, surface, grid, text, decay } = STUDIO_PALETTE;
    const sustainHeight = Math.min(pianoRoll.sustainLaneHeight, Math.floor(height * 0.22));
    const rollTop = pianoRoll.headerInsetTop;
    const rollBottom = Math.max(rollTop + 20, height - sustainHeight);
    const rollHeight = rollBottom - rollTop;

    const windowSec = pianoRoll.windowSec;
    const playheadX = width * pianoRoll.playheadRatio;
    const windowStart = timeSec - windowSec * pianoRoll.playheadRatio;
    const windowEnd = windowStart + windowSec;
    const timeToX = (t: number) => ((t - windowStart) / windowSec) * width;

    // Background fill
    ctx.fillStyle = surface.panelRecessed;
    ctx.fillRect(0, 0, width, height);

    // Pitch bands and octave lines
    const pitchCount = Math.max(1, cache.maxMidi - cache.minMidi + 1);
    const rowHeight = rollHeight / pitchCount;
    const midiToY = (midi: number) => rollBottom - (midi - cache.minMidi + 1) * rowHeight;

    for (let midi = cache.minMidi; midi <= cache.maxMidi; midi += 1) {
      const y = midiToY(midi);
      const pc = ((midi % 12) + 12) % 12;
      if (BLACK_KEY_PCS.has(pc)) {
        ctx.fillStyle = grid.blackKeyBand;
        ctx.fillRect(0, y, width, rowHeight);
      }
      if (pc === 0) {
        const lineY = Math.round(y + rowHeight) + 0.5;
        ctx.strokeStyle = grid.octaveLine;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(0, lineY);
        ctx.lineTo(width, lineY);
        ctx.stroke();

        ctx.fillStyle = text.dim;
        ctx.font = `500 9px ${STUDIO_PALETTE.typography.mono}`;
        ctx.fillText(midiToLabel(midi), 6, lineY - 3);
      }
    }

    // Beat and bar vertical grid lines
    const beats = cache.snapshot.beats;
    let beatIndex = lowerBoundBeat(beats, windowStart);
    while (beatIndex < beats.length && beats[beatIndex].t <= windowEnd) {
      const beat = beats[beatIndex];
      const x = Math.round(timeToX(beat.t)) + 0.5;
      ctx.strokeStyle = beat.downbeat ? grid.barLine : grid.beatLine;
      ctx.lineWidth = beat.downbeat ? 1.25 : 1;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, rollBottom);
      ctx.stroke();

      if (beat.downbeat) {
        ctx.fillStyle = text.muted;
        ctx.font = `600 9px ${STUDIO_PALETTE.typography.mono}`;
        ctx.fillText(`${beat.bar}`, x + 4, 12);
      }
      beatIndex += 1;
    }

    // Soft playhead atmospheric wash
    const washWidth = Math.max(24, width * 0.06);
    const washGrad = ctx.createLinearGradient(playheadX - washWidth, 0, playheadX + 8, 0);
    washGrad.addColorStop(0, 'rgba(245, 185, 99, 0)');
    washGrad.addColorStop(0.85, grid.playheadWash);
    washGrad.addColorStop(1, 'rgba(245, 185, 99, 0)');
    ctx.fillStyle = washGrad;
    ctx.fillRect(playheadX - washWidth, 0, washWidth + 8, rollBottom);

    // Notes
    const afterglowMargin = 1.5;
    for (const item of cache.notes) {
      const { note, trackIndex, category } = item;
      if (note.endSec < windowStart - afterglowMargin || note.startSec > windowEnd) {
        continue;
      }

      const x1 = timeToX(note.startSec);
      const x2 = timeToX(note.endSec);
      const drawX = Math.max(-4, x1);
      const drawRight = Math.min(width + 4, Math.max(x2, x1 + 3));
      const drawW = drawRight - drawX;
      if (drawW <= 0) continue;

      const isSounding = timeSec >= note.startSec && timeSec <= note.endSec;
      const attackGlow =
        timeSec >= note.startSec ? expDecay(timeSec - note.startSec, decay.attackGlowRate) : 0;
      const afterglow =
        timeSec > note.endSec ? expDecay(timeSec - note.endSec, decay.afterglowRate) : 0;
      const sustainGlow = isSounding ? 1 : 0;
      const energy = clamp01(Math.max(attackGlow, sustainGlow * 0.72, afterglow * 0.55));

      const isHighlightedSolo =
        cache.isSoloEmphasis &&
        (note.top || note.velocity >= STUDIO_PALETTE.pianoRoll.soloVelocityThreshold);
      const isRecessedSolo = cache.isSoloEmphasis && !isHighlightedSolo;

      const heightScale = isHighlightedSolo ? 0.94 : isRecessedSolo ? 0.66 : 0.82;
      const noteH = Math.max(3, rowHeight * heightScale);
      const noteY = midiToY(note.midi) + (rowHeight - noteH) / 2;

      const paletteStyle = resolveNoteColor(note, trackIndex, colorMode);
      const baseAlpha =
        (isRecessedSolo ? 0.52 : STUDIO_PALETTE.categoryAlpha[category]) *
        (0.7 + (note.velocity / 127) * 0.3);

      // Outer illumination halo when active or decaying
      if (energy > 0.03) {
        const pad = 2 + attackGlow * 5;
        ctx.save();
        ctx.globalAlpha = clamp01(energy * (isHighlightedSolo ? 0.65 : 0.5));
        ctx.fillStyle = paletteStyle.glow;
        fillRoundedRect(
          ctx,
          drawX - pad * 0.5,
          noteY - pad * 0.5,
          drawW + pad,
          noteH + pad,
          pianoRoll.noteCornerRadius + 2,
        );
        ctx.restore();
      }

      // Main note body
      ctx.save();
      ctx.globalAlpha = clamp01(baseAlpha + energy * 0.28);
      ctx.fillStyle = paletteStyle.fill;
      fillRoundedRect(ctx, drawX, noteY, drawW, noteH, pianoRoll.noteCornerRadius);

      // Specular inner core on active/attacking notes
      if (sustainGlow > 0 || attackGlow > 0.08) {
        ctx.globalAlpha = clamp01(0.24 + attackGlow * 0.58);
        ctx.fillStyle = paletteStyle.core;
        fillRoundedRect(
          ctx,
          drawX + 1,
          noteY + 1,
          Math.max(1, drawW - 2),
          Math.max(1.5, noteH * 0.44),
          Math.max(1, pianoRoll.noteCornerRadius - 1),
        );
      }

      // Crisp border
      ctx.globalAlpha = clamp01(0.55 + energy * 0.45 + (isHighlightedSolo ? 0.15 : 0));
      ctx.strokeStyle = paletteStyle.stroke;
      ctx.lineWidth = isHighlightedSolo ? 1.4 : 1;
      strokeRoundedRect(ctx, drawX, noteY, drawW, noteH, pianoRoll.noteCornerRadius);
      ctx.restore();

      // Playhead attack flare at exact strike point
      if (attackGlow > 0.06 && x1 <= playheadX + 4 && x2 >= playheadX - 18) {
        const flareRadius = 5 + attackGlow * 10;
        const cy = noteY + noteH / 2;
        ctx.save();
        ctx.globalAlpha = clamp01(attackGlow * 0.72);
        const flare = ctx.createRadialGradient(playheadX, cy, 0.5, playheadX, cy, flareRadius);
        flare.addColorStop(0, paletteStyle.core);
        flare.addColorStop(0.45, paletteStyle.glow);
        flare.addColorStop(1, 'rgba(255, 255, 255, 0)');
        ctx.fillStyle = flare;
        ctx.beginPath();
        ctx.arc(playheadX, cy, flareRadius, 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
      }
    }

    // CC64 Sustain Pedal sub-lane
    this.drawSustainLane(ctx, width, rollBottom, sustainHeight, cache.sustains, timeSec, timeToX);

    // Playhead vertical line
    ctx.save();
    ctx.strokeStyle = grid.playheadGlow;
    ctx.lineWidth = 5;
    ctx.beginPath();
    ctx.moveTo(playheadX, 0);
    ctx.lineTo(playheadX, height);
    ctx.stroke();

    ctx.strokeStyle = grid.playheadLine;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(playheadX, 0);
    ctx.lineTo(playheadX, height);
    ctx.stroke();

    // Top needle cap
    ctx.fillStyle = grid.playheadLine;
    ctx.beginPath();
    ctx.moveTo(playheadX - 4, 0);
    ctx.lineTo(playheadX + 4, 0);
    ctx.lineTo(playheadX, 6);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }

  private drawSustainLane(
    ctx: CanvasRenderingContext2D,
    width: number,
    top: number,
    height: number,
    sustains: readonly VisualSustain[],
    timeSec: number,
    timeToX: (t: number) => number,
  ): void {
    const { surface, border, grid, text } = STUDIO_PALETTE;
    ctx.fillStyle = surface.sustainLane;
    ctx.fillRect(0, top, width, height);

    ctx.strokeStyle = border.panel;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, top + 0.5);
    ctx.lineTo(width, top + 0.5);
    ctx.stroke();

    const barTop = top + 7;
    const barHeight = Math.max(6, height - 13);

    for (const sustain of sustains) {
      const x1 = timeToX(sustain.startSec);
      const x2 = timeToX(sustain.endSec);
      if (x2 < 0 || x1 > width) continue;
      const drawX = Math.max(0, x1);
      const drawRight = Math.min(width, x2);
      const drawW = drawRight - drawX;
      if (drawW <= 0) continue;

      const active = timeSec >= sustain.startSec && timeSec <= sustain.endSec;
      ctx.fillStyle = active ? grid.sustainActiveFill : grid.sustainFill;
      fillRoundedRect(ctx, drawX, barTop, drawW, barHeight, 2);

      ctx.strokeStyle = active ? grid.sustainActiveStroke : grid.sustainStroke;
      ctx.lineWidth = active ? 1.5 : 1;
      ctx.beginPath();
      ctx.moveTo(drawX, barTop + 0.5);
      ctx.lineTo(drawX + drawW, barTop + 0.5);
      ctx.stroke();
    }

    ctx.fillStyle = text.dim;
    ctx.font = `600 9px ${STUDIO_PALETTE.typography.mono}`;
    ctx.fillText('CC64', 6, top + height - 6);
  }
}

function resolveNoteColor(
  note: VisualNote,
  trackIndex: number,
  colorMode: ColorMode,
): TrackColorStyle {
  if (colorMode === 'harmony') {
    return note.chordRole
      ? STUDIO_PALETTE.harmony[note.chordRole]
      : STUDIO_PALETTE.harmony.fallback;
  }
  if (colorMode === 'velocity') {
    return interpolateVelocityColor(note.velocity);
  }
  return STUDIO_PALETTE.tracks[trackIndex % STUDIO_PALETTE.tracks.length];
}

function lowerBoundBeat(beats: readonly { t: number }[], targetSec: number): number {
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

function fillRoundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
): void {
  traceRoundedRect(ctx, x, y, width, height, radius);
  ctx.fill();
}

function strokeRoundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
): void {
  traceRoundedRect(ctx, x, y, width, height, radius);
  ctx.stroke();
}

function traceRoundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
): void {
  const r = Math.min(radius, width / 2, height / 2);
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + width - r, y);
  ctx.quadraticCurveTo(x + width, y, x + width, y + r);
  ctx.lineTo(x + width, y + height - r);
  ctx.quadraticCurveTo(x + width, y + height, x + width - r, y + height);
  ctx.lineTo(x + r, y + height);
  ctx.quadraticCurveTo(x, y + height, x, y + height - r);
  ctx.lineTo(x, y + r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.closePath();
}
