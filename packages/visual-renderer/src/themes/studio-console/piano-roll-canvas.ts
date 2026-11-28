import { midiToLabel } from '../../format';
import type { VisualNote, VisualSnapshot, VisualSustain, VisualTrack } from '../../generated';
import { clamp01, expDecay } from '../../math';
import { findFirstBeatAtOrAfter } from '../../query';
import type { ColorMode } from '../../types';
import { interpolateVelocityColor, STUDIO_PALETTE, type TrackColorStyle } from './palette';

const BLACK_KEY_PCS = new Set([1, 3, 6, 8, 10]);
const KEY_STRIP_WIDTH = 28;
const RULER_HEIGHT = 20;

export interface IndexedNote {
  note: VisualNote;
  trackIndex: number;
  category: VisualTrack['category'];
}

export interface PianoRollSnapshotCache {
  snapshot: VisualSnapshot;
  nonDrumTracks: VisualTrack[];
  notes: IndexedNote[];
  sustains: VisualSustain[];
  minMidi: number;
  maxMidi: number;
  isSoloEmphasis: boolean;
}

/** Converts a click X coordinate on the piano roll canvas to a clamped song time in seconds. */
export function canvasXToTimeSec(
  offsetX: number,
  canvasWidth: number,
  timeSec: number,
  durationSec: number,
): number {
  const usableWidth = Math.max(1, canvasWidth - KEY_STRIP_WIDTH);
  const ratio = (offsetX - KEY_STRIP_WIDTH) / usableWidth;
  const windowSec = STUDIO_PALETTE.pianoRoll.windowSec;
  const windowStart = timeSec - windowSec * STUDIO_PALETTE.pianoRoll.playheadRatio;
  const rawTime = windowStart + ratio * windowSec;
  return Math.min(Math.max(rawTime, 0), Math.max(0, durationSec));
}

/** Draws one deterministic frame of the scrolling piano roll and optional CC64 lane. */
export function drawPianoRollFrame(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  cache: PianoRollSnapshotCache,
  timeSec: number,
  colorMode: ColorMode,
): void {
  const { pianoRoll, surface, grid, text, border, decay } = STUDIO_PALETTE;
  const hasSustain = cache.sustains.length > 0;
  const sustainHeight = hasSustain
    ? Math.min(pianoRoll.sustainLaneHeight, Math.floor(height * 0.18))
    : 0;
  const rollTop = RULER_HEIGHT;
  const rollBottom = Math.max(rollTop + 20, height - sustainHeight);
  const rollHeight = rollBottom - rollTop;

  const rollLeft = KEY_STRIP_WIDTH;
  const rollWidth = Math.max(1, width - rollLeft);
  const windowSec = pianoRoll.windowSec;
  const playheadX = rollLeft + rollWidth * pianoRoll.playheadRatio;
  const windowStart = timeSec - windowSec * pianoRoll.playheadRatio;
  const windowEnd = windowStart + windowSec;
  const timeToX = (t: number) => rollLeft + ((t - windowStart) / windowSec) * rollWidth;

  // Base background
  ctx.fillStyle = surface.panelRecessed;
  ctx.fillRect(0, 0, width, height);

  // Top bar ruler background
  ctx.fillStyle = 'rgba(13, 18, 29, 0.85)';
  ctx.fillRect(rollLeft, 0, rollWidth, rollTop);
  ctx.strokeStyle = border.subtle;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, rollTop + 0.5);
  ctx.lineTo(width, rollTop + 0.5);
  ctx.stroke();

  // Pitch grid & left piano key strip
  const pitchCount = Math.max(1, cache.maxMidi - cache.minMidi + 1);
  const rowHeight = rollHeight / pitchCount;
  const midiToY = (midi: number) => rollBottom - (midi - cache.minMidi + 1) * rowHeight;

  for (let midi = cache.minMidi; midi <= cache.maxMidi; midi += 1) {
    const y = midiToY(midi);
    const pc = ((midi % 12) + 12) % 12;
    const isBlack = BLACK_KEY_PCS.has(pc);

    if (isBlack) {
      ctx.fillStyle = grid.blackKeyBand;
      ctx.fillRect(rollLeft, y, rollWidth, rowHeight);
    }

    // Left key strip cell
    ctx.fillStyle = isBlack ? '#0b0f17' : 'rgba(182, 204, 240, 0.085)';
    ctx.fillRect(0, y, rollLeft - 1, Math.max(1, rowHeight - 0.5));

    if (pc === 0) {
      const lineY = Math.round(y + rowHeight) + 0.5;
      ctx.strokeStyle = grid.octaveLine;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(rollLeft, lineY);
      ctx.lineTo(width, lineY);
      ctx.stroke();

      ctx.fillStyle = text.secondary;
      ctx.font = `600 9.5px ${STUDIO_PALETTE.typography.mono}`;
      ctx.fillText(midiToLabel(midi), 4, Math.min(rollBottom - 2, lineY - 3));
    }
  }

  // Key strip right divider
  ctx.strokeStyle = border.panel;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(rollLeft - 0.5, 0);
  ctx.lineTo(rollLeft - 0.5, height);
  ctx.stroke();

  // Clip main roll area so notes and bars never bleed over the key strip
  ctx.save();
  ctx.beginPath();
  ctx.rect(rollLeft, 0, rollWidth, height);
  ctx.clip();

  // Beat and bar vertical grid lines
  const beats = cache.snapshot.beats;
  let beatIndex = findFirstBeatAtOrAfter(beats, windowStart);
  while (beatIndex < beats.length && beats[beatIndex].t <= windowEnd) {
    const beat = beats[beatIndex];
    const x = Math.round(timeToX(beat.t)) + 0.5;
    ctx.strokeStyle = beat.downbeat ? grid.barLine : grid.beatLine;
    ctx.lineWidth = beat.downbeat ? 1.25 : 1;
    ctx.beginPath();
    ctx.moveTo(x, beat.downbeat ? 0 : rollTop);
    ctx.lineTo(x, rollBottom);
    ctx.stroke();

    if (beat.downbeat) {
      ctx.fillStyle = text.secondary;
      ctx.font = `600 10px ${STUDIO_PALETTE.typography.mono}`;
      ctx.fillText(`${beat.bar}`, x + 5, 13);
    }
    beatIndex += 1;
  }

  // Atmospheric playhead wash
  const washWidth = Math.max(36, rollWidth * 0.08);
  const washGrad = ctx.createLinearGradient(playheadX - washWidth, 0, playheadX + 14, 0);
  washGrad.addColorStop(0, 'rgba(245, 185, 99, 0)');
  washGrad.addColorStop(0.82, 'rgba(245, 185, 99, 0.1)');
  washGrad.addColorStop(1, 'rgba(245, 185, 99, 0)');
  ctx.fillStyle = washGrad;
  ctx.fillRect(playheadX - washWidth, rollTop, washWidth + 14, rollHeight);

  if (cache.notes.length === 0) {
    ctx.fillStyle = text.muted;
    ctx.font = `600 11px ${STUDIO_PALETTE.typography.mono}`;
    ctx.textAlign = 'center';
    ctx.fillText('NO MELODIC TRACKS', rollLeft + rollWidth / 2, rollTop + rollHeight / 2);
    ctx.textAlign = 'start';
  }

  // Notes
  for (const item of cache.notes) {
    const { note, trackIndex, category } = item;
    if (note.endSec < windowStart || note.startSec > windowEnd) {
      continue;
    }

    const x1 = timeToX(note.startSec);
    const x2 = timeToX(note.endSec);
    const drawX = Math.max(rollLeft - 4, x1);
    const drawRight = Math.min(width + 4, Math.max(x2, x1 + 3));
    const drawW = drawRight - drawX;
    if (drawW <= 0) continue;

    const isSounding = timeSec >= note.startSec && timeSec <= note.endSec;
    const isPassed = timeSec > note.endSec;
    const attackGlow =
      timeSec >= note.startSec ? expDecay(timeSec - note.startSec, decay.attackGlowRate) : 0;
    const afterglow = isPassed ? expDecay(timeSec - note.endSec, decay.afterglowRate) : 0;
    const energy = clamp01(Math.max(attackGlow, isSounding ? 0.88 : 0, afterglow * 0.55));

    const isHighlightedSolo =
      cache.isSoloEmphasis &&
      (note.top || note.velocity >= STUDIO_PALETTE.pianoRoll.soloVelocityThreshold);
    const isRecessedSolo = cache.isSoloEmphasis && !isHighlightedSolo;

    const heightScale = isSounding
      ? isHighlightedSolo
        ? 0.98
        : 0.9
      : isHighlightedSolo
        ? 0.88
        : isRecessedSolo
          ? 0.62
          : 0.78;
    const noteH = Math.max(3, rowHeight * heightScale);
    const noteY = midiToY(note.midi) + (rowHeight - noteH) / 2;

    const paletteStyle = resolveNoteColor(note, trackIndex, colorMode);
    const temporalPresence = isSounding ? 1 : isPassed ? 0.44 + afterglow * 0.42 : 0.62;
    const baseAlpha =
      (isRecessedSolo ? 0.48 : STUDIO_PALETTE.categoryAlpha[category]) *
      (0.72 + (note.velocity / 127) * 0.28) *
      temporalPresence;

    // Outer bloom halo when sounding or freshly released
    if (energy > 0.04) {
      const pad = 3 + attackGlow * 7 + (isSounding ? 2 : 0);
      ctx.save();
      ctx.globalAlpha = clamp01(energy * (isHighlightedSolo ? 0.72 : 0.56));
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

    // Specular inner core on sounding / attacking notes
    if (isSounding || attackGlow > 0.08) {
      ctx.globalAlpha = clamp01(0.32 + attackGlow * 0.62);
      ctx.fillStyle = paletteStyle.core;
      fillRoundedRect(
        ctx,
        drawX + 1,
        noteY + 1,
        Math.max(1, drawW - 2),
        Math.max(1.5, noteH * 0.48),
        Math.max(1, pianoRoll.noteCornerRadius - 1),
      );
    }

    // Crisp border
    ctx.globalAlpha = clamp01(
      (isSounding ? 0.96 : isPassed ? 0.35 : 0.55) + (isHighlightedSolo ? 0.1 : 0),
    );
    ctx.strokeStyle = isSounding ? paletteStyle.core : paletteStyle.stroke;
    ctx.lineWidth = isSounding || isHighlightedSolo ? 1.4 : 0.9;
    strokeRoundedRect(ctx, drawX, noteY, drawW, noteH, pianoRoll.noteCornerRadius);
    ctx.restore();

    // Playhead strike flare
    if ((isSounding || attackGlow > 0.06) && x1 <= playheadX + 4 && x2 >= playheadX - 18) {
      const flareStrength = Math.max(attackGlow, isSounding ? 0.45 : 0);
      const flareRadius = 7 + flareStrength * 14;
      const cy = noteY + noteH / 2;
      ctx.save();
      ctx.globalAlpha = clamp01(flareStrength * 0.85);
      const flare = ctx.createRadialGradient(playheadX, cy, 0.5, playheadX, cy, flareRadius);
      flare.addColorStop(0, paletteStyle.core);
      flare.addColorStop(0.4, paletteStyle.glow);
      flare.addColorStop(1, 'rgba(255, 255, 255, 0)');
      ctx.fillStyle = flare;
      ctx.beginPath();
      ctx.arc(playheadX, cy, flareRadius, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
  }

  // Optional CC64 sustain sub-lane
  if (hasSustain && sustainHeight > 0) {
    drawSustainLane(
      ctx,
      rollLeft,
      width,
      rollBottom,
      sustainHeight,
      cache.sustains,
      timeSec,
      timeToX,
    );
  }

  // Playhead needle
  ctx.save();
  ctx.strokeStyle = grid.playheadGlow;
  ctx.lineWidth = 6;
  ctx.beginPath();
  ctx.moveTo(playheadX, 0);
  ctx.lineTo(playheadX, height);
  ctx.stroke();

  ctx.strokeStyle = grid.playheadLine;
  ctx.lineWidth = 1.6;
  ctx.beginPath();
  ctx.moveTo(playheadX, 0);
  ctx.lineTo(playheadX, height);
  ctx.stroke();

  ctx.fillStyle = grid.playheadLine;
  ctx.beginPath();
  ctx.moveTo(playheadX - 5, 0);
  ctx.lineTo(playheadX + 5, 0);
  ctx.lineTo(playheadX, 7);
  ctx.closePath();
  ctx.fill();
  ctx.restore();

  ctx.restore();

  if (hasSustain && sustainHeight > 0) {
    ctx.fillStyle = text.secondary;
    ctx.font = `600 8.5px ${STUDIO_PALETTE.typography.mono}`;
    ctx.fillText('PED', 4, rollBottom + sustainHeight - 6);
  }
}

function drawSustainLane(
  ctx: CanvasRenderingContext2D,
  left: number,
  width: number,
  top: number,
  height: number,
  sustains: readonly VisualSustain[],
  timeSec: number,
  timeToX: (t: number) => number,
): void {
  const { surface, border, grid } = STUDIO_PALETTE;
  ctx.fillStyle = surface.sustainLane;
  ctx.fillRect(left, top, width - left, height);

  ctx.strokeStyle = border.panel;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, top + 0.5);
  ctx.lineTo(width, top + 0.5);
  ctx.stroke();

  const barTop = top + 6;
  const barHeight = Math.max(6, height - 11);

  for (const sustain of sustains) {
    const x1 = timeToX(sustain.startSec);
    const x2 = timeToX(sustain.endSec);
    if (x2 < left || x1 > width) continue;
    const drawX = Math.max(left, x1);
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
