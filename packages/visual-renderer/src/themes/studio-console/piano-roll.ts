import type { VisualSnapshot, VisualSustain, VisualTrack } from '../../generated';
import type { ColorMode, ViewState } from '../../types';
import { STUDIO_PALETTE } from './palette';
import {
  canvasXToTimeSec,
  drawPianoRollFrame,
  type IndexedNote,
  type PianoRollSnapshotCache,
} from './piano-roll-canvas';

const MAX_LEGEND_ITEMS = 6;

interface LegendItemNodes {
  item: HTMLSpanElement;
  swatch: HTMLSpanElement;
  label: HTMLSpanElement;
}

/**
 * Manages the Piano Roll panel DOM, snapshot indexing, legend, and click-to-seek,
 * delegating pure Canvas 2D frame rendering to `drawPianoRollFrame`.
 */
export class PianoRollView {
  readonly element: HTMLElement;
  private readonly canvasWrap: HTMLDivElement;
  private readonly canvas: HTMLCanvasElement;
  private readonly legendContainer: HTMLDivElement;
  private readonly legendItems: LegendItemNodes[] = [];

  private cache: PianoRollSnapshotCache | null = null;
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
      onSeek(
        canvasXToTimeSec(
          event.clientX - rect.left,
          rect.width,
          this.lastTimeSec,
          this.lastDurationSec,
        ),
      );
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
    drawPianoRollFrame(ctx, cssWidth, cssHeight, cache, timeSec, state.colorMode);
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
}
