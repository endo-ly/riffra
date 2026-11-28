import { formatKeyLabel, formatMusicSymbol } from '../../format';
import type { VisualKey, VisualSnapshot } from '../../generated';
import {
  circleOfFifthsAngle,
  circleOfFifthsIndex,
  clamp01,
  easeOutCubic,
  lerpAngle,
} from '../../math';
import { findChordWindow, findRecentChordRoots, findSectionAt } from '../../query';
import { STUDIO_PALETTE } from './palette';

const SVG_NS = 'http://www.w3.org/2000/svg';
const FIFTHS_LABELS = ['C', 'G', 'D', 'A', 'E', 'B', 'F♯', 'D♭', 'A♭', 'E♭', 'B♭', 'F'] as const;
const MAX_TRAIL_SEGMENTS = 3;

interface VertexNodes {
  circle: SVGCircleElement;
  halo: SVGCircleElement;
  text: SVGTextElement;
  slotIndex: number;
}

interface TrailSegmentNodes {
  glow: SVGLineElement;
  line: SVGLineElement;
}

/**
 * SVG Circle of Fifths renderer with C fixed at 12 o'clock, smoothly rotating
 * diatonic arc on modulation, and attenuating chord-root progression trails.
 *
 * All SVG nodes are allocated once in the constructor and updated via
 * attributes and text content during `render()`.
 */
export class CircleOfFifthsView {
  readonly element: HTMLElement;
  private readonly diatonicGlow: SVGPathElement;
  private readonly diatonicArc: SVGPathElement;
  private readonly trails: TrailSegmentNodes[] = [];
  private readonly trailHead: SVGCircleElement;
  private readonly vertices: VertexNodes[] = [];
  private readonly centerPrimary: SVGTextElement;
  private readonly centerSecondary: SVGTextElement;
  private readonly sectionTag: HTMLSpanElement;
  private readonly keyReadout: HTMLSpanElement;
  private readonly footer: HTMLDivElement;

  constructor(doc: Document) {
    const C = STUDIO_PALETTE.circle;

    this.element = doc.createElement('section');
    this.element.className = 'sc-panel sc-circle-panel';
    this.element.setAttribute('data-role', 'circle-of-fifths');

    const header = doc.createElement('div');
    header.className = 'sc-panel-header';

    const title = doc.createElement('span');
    title.className = 'sc-panel-title';
    title.textContent = 'CIRCLE OF FIFTHS';

    this.sectionTag = doc.createElement('span');
    this.sectionTag.className = 'sc-panel-tag';
    this.sectionTag.setAttribute('data-role', 'circle-section');
    this.sectionTag.hidden = true;

    header.append(title, this.sectionTag);

    const stage = doc.createElement('div');
    stage.className = 'sc-circle-stage';

    const svg = doc.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('class', 'sc-circle-svg');
    svg.setAttribute('viewBox', `0 0 ${C.viewBoxSize} ${C.viewBoxSize}`);
    svg.setAttribute('role', 'img');
    svg.setAttribute('aria-label', 'Circle of Fifths');

    // Outer technical bezel rings
    const outerRing = doc.createElementNS(SVG_NS, 'circle');
    outerRing.setAttribute('cx', String(C.center));
    outerRing.setAttribute('cy', String(C.center));
    outerRing.setAttribute('r', String(C.outerRingRadius));
    outerRing.setAttribute('fill', 'none');
    outerRing.setAttribute('stroke', STUDIO_PALETTE.border.subtle);
    outerRing.setAttribute('stroke-width', '1');

    const innerRing = doc.createElementNS(SVG_NS, 'circle');
    innerRing.setAttribute('cx', String(C.center));
    innerRing.setAttribute('cy', String(C.center));
    innerRing.setAttribute('r', String(C.diatonicArcRadius));
    innerRing.setAttribute('fill', 'rgba(7, 10, 16, 0.55)');
    innerRing.setAttribute('stroke', STUDIO_PALETTE.border.subtle);
    innerRing.setAttribute('stroke-width', '1');

    svg.append(outerRing, innerRing);

    // Diatonic arc + soft glow
    this.diatonicGlow = doc.createElementNS(SVG_NS, 'path');
    this.diatonicGlow.setAttribute('data-role', 'diatonic-arc-glow');
    this.diatonicGlow.setAttribute('fill', 'none');
    this.diatonicGlow.setAttribute('stroke', C.diatonicArcGlow);
    this.diatonicGlow.setAttribute('stroke-width', String(C.arcGlowWidth));
    this.diatonicGlow.setAttribute('stroke-linecap', 'round');
    this.diatonicGlow.setAttribute('visibility', 'hidden');

    this.diatonicArc = doc.createElementNS(SVG_NS, 'path');
    this.diatonicArc.setAttribute('data-role', 'diatonic-arc');
    this.diatonicArc.setAttribute('fill', 'none');
    this.diatonicArc.setAttribute('stroke', C.diatonicArcColor);
    this.diatonicArc.setAttribute('stroke-width', String(C.arcStrokeWidth));
    this.diatonicArc.setAttribute('stroke-linecap', 'round');
    this.diatonicArc.setAttribute('visibility', 'hidden');

    svg.append(this.diatonicGlow, this.diatonicArc);

    // Pre-allocated progression trail lines
    for (let index = 0; index < MAX_TRAIL_SEGMENTS; index += 1) {
      const glow = doc.createElementNS(SVG_NS, 'line');
      glow.setAttribute('stroke', C.trailGlow);
      glow.setAttribute('stroke-linecap', 'round');
      glow.setAttribute('visibility', 'hidden');

      const line = doc.createElementNS(SVG_NS, 'line');
      line.setAttribute('data-role', 'trail-segment');
      line.setAttribute('stroke', C.trailColor);
      line.setAttribute('stroke-linecap', 'round');
      line.setAttribute('visibility', 'hidden');

      svg.append(glow, line);
      this.trails.push({ glow, line });
    }

    this.trailHead = doc.createElementNS(SVG_NS, 'circle');
    this.trailHead.setAttribute('r', '4.2');
    this.trailHead.setAttribute('fill', STUDIO_PALETTE.accent.amberBright);
    this.trailHead.setAttribute('stroke', STUDIO_PALETTE.accent.amber);
    this.trailHead.setAttribute('stroke-width', '1.5');
    this.trailHead.setAttribute('visibility', 'hidden');
    svg.appendChild(this.trailHead);

    // 12 fixed vertices (C at 12 o'clock)
    for (let slotIndex = 0; slotIndex < 12; slotIndex += 1) {
      const angle = -Math.PI / 2 + slotIndex * (Math.PI / 6);
      const x = C.center + Math.cos(angle) * C.nodeOrbitRadius;
      const y = C.center + Math.sin(angle) * C.nodeOrbitRadius;

      const group = doc.createElementNS(SVG_NS, 'g');
      group.setAttribute('data-slot', String(slotIndex));

      const halo = doc.createElementNS(SVG_NS, 'circle');
      halo.setAttribute('cx', x.toFixed(2));
      halo.setAttribute('cy', y.toFixed(2));
      halo.setAttribute('r', String(C.tonicNodeRadius + 4));
      halo.setAttribute('fill', 'none');
      halo.setAttribute('stroke', 'none');
      halo.setAttribute('stroke-width', '2');
      halo.setAttribute('opacity', '0');

      const circle = doc.createElementNS(SVG_NS, 'circle');
      circle.setAttribute('cx', x.toFixed(2));
      circle.setAttribute('cy', y.toFixed(2));
      circle.setAttribute('r', String(C.nodeRadius));
      circle.setAttribute('fill', C.inactiveNodeFill);
      circle.setAttribute('stroke', C.inactiveNodeStroke);
      circle.setAttribute('stroke-width', '1.25');

      const text = doc.createElementNS(SVG_NS, 'text');
      text.setAttribute('x', x.toFixed(2));
      text.setAttribute('y', y.toFixed(2));
      text.setAttribute('text-anchor', 'middle');
      text.setAttribute('dominant-baseline', 'central');
      text.setAttribute('fill', STUDIO_PALETTE.text.muted);
      text.setAttribute('font-size', '11');
      text.setAttribute('font-weight', '600');
      text.setAttribute('font-family', STUDIO_PALETTE.typography.mono);
      text.textContent = FIFTHS_LABELS[slotIndex];

      group.append(halo, circle, text);
      svg.appendChild(group);
      this.vertices.push({ circle, halo, text, slotIndex });
    }

    // Center current chord display
    this.centerPrimary = doc.createElementNS(SVG_NS, 'text');
    this.centerPrimary.setAttribute('data-role', 'circle-center-primary');
    this.centerPrimary.setAttribute('x', String(C.center));
    this.centerPrimary.setAttribute('y', String(C.center - 4));
    this.centerPrimary.setAttribute('text-anchor', 'middle');
    this.centerPrimary.setAttribute('dominant-baseline', 'central');
    this.centerPrimary.setAttribute('fill', STUDIO_PALETTE.accent.amberBright);
    this.centerPrimary.setAttribute('font-size', '18');
    this.centerPrimary.setAttribute('font-weight', '700');
    this.centerPrimary.setAttribute('font-family', STUDIO_PALETTE.typography.sans);
    this.centerPrimary.textContent = '—';

    this.centerSecondary = doc.createElementNS(SVG_NS, 'text');
    this.centerSecondary.setAttribute('data-role', 'circle-center-secondary');
    this.centerSecondary.setAttribute('x', String(C.center));
    this.centerSecondary.setAttribute('y', String(C.center + 16));
    this.centerSecondary.setAttribute('text-anchor', 'middle');
    this.centerSecondary.setAttribute('dominant-baseline', 'central');
    this.centerSecondary.setAttribute('fill', STUDIO_PALETTE.text.secondary);
    this.centerSecondary.setAttribute('font-size', '11');
    this.centerSecondary.setAttribute('font-weight', '500');
    this.centerSecondary.setAttribute('font-family', STUDIO_PALETTE.typography.mono);

    svg.append(this.centerPrimary, this.centerSecondary);
    stage.appendChild(svg);

    // Bottom readout: current key
    this.footer = doc.createElement('div');
    this.footer.className = 'sc-circle-footer';

    this.keyReadout = doc.createElement('span');
    this.keyReadout.className = 'sc-circle-key';
    this.keyReadout.setAttribute('data-role', 'circle-key');

    this.footer.append(this.keyReadout);
    this.footer.hidden = true;
    this.element.append(header, stage, this.footer);
  }

  render(snapshot: VisualSnapshot, timeSec: number): void {
    const sectionWindow = findSectionAt(snapshot, timeSec);
    const currentKey = sectionWindow.current?.key;
    const previousKey = sectionWindow.previous?.key;
    const chordWindow = findChordWindow(snapshot, timeSec, 4);
    const activeChord =
      chordWindow.current >= 0 ? chordWindow.chords[chordWindow.current] : undefined;
    const recentRoots = findRecentChordRoots(snapshot, timeSec, MAX_TRAIL_SEGMENTS + 1);

    const sectionName = sectionWindow.current?.name;
    if (sectionName) {
      this.sectionTag.textContent = sectionName;
      this.sectionTag.hidden = false;
    } else {
      this.sectionTag.textContent = '';
      this.sectionTag.hidden = true;
    }

    this.updateDiatonicArc(currentKey, previousKey, sectionWindow.current?.startSec ?? 0, timeSec);
    this.updateTrails(recentRoots);
    this.updateVertices(currentKey, activeChord?.rootPc, activeChord?.tonesPc);
    this.updateCenter(activeChord);
    this.updateFooter(currentKey);
  }

  private updateDiatonicArc(
    currentKey: VisualKey | undefined,
    previousKey: VisualKey | undefined,
    sectionStartSec: number,
    timeSec: number,
  ): void {
    const scalePcs = currentKey?.scalePcs;
    if (!currentKey || !scalePcs || scalePcs.length === 0) {
      this.diatonicArc.setAttribute('visibility', 'hidden');
      this.diatonicArc.setAttribute('d', '');
      this.diatonicGlow.setAttribute('visibility', 'hidden');
      this.diatonicGlow.setAttribute('d', '');
      return;
    }

    const anchorPc = currentKey.tonicPc ?? scalePcs[0];
    const anchorSlot = circleOfFifthsIndex(anchorPc);

    let minDelta = 0;
    let maxDelta = 0;
    for (const pc of scalePcs) {
      const slot = circleOfFifthsIndex(pc);
      const delta = ((((slot - anchorSlot + 6) % 12) + 12) % 12) - 6;
      if (delta < minDelta) minDelta = delta;
      if (delta > maxDelta) maxDelta = delta;
    }

    let animatedAngle = circleOfFifthsAngle(anchorPc);
    if (
      previousKey?.tonicPc !== undefined &&
      currentKey.tonicPc !== undefined &&
      previousKey.tonicPc !== currentKey.tonicPc
    ) {
      const elapsed = Math.max(0, timeSec - sectionStartSec);
      const progress = easeOutCubic(clamp01(elapsed / STUDIO_PALETTE.decay.modulationDurationSec));
      animatedAngle = lerpAngle(
        circleOfFifthsAngle(previousKey.tonicPc),
        circleOfFifthsAngle(currentKey.tonicPc),
        progress,
      );
    }

    const stepRad = Math.PI / 6;
    const padRad = stepRad * 0.28;
    const startAngle = animatedAngle + minDelta * stepRad - padRad;
    const endAngle = animatedAngle + maxDelta * stepRad + padRad;
    const arcPath = describeArc(
      STUDIO_PALETTE.circle.center,
      STUDIO_PALETTE.circle.center,
      STUDIO_PALETTE.circle.diatonicArcRadius,
      startAngle,
      endAngle,
    );

    this.diatonicArc.setAttribute('d', arcPath);
    this.diatonicArc.setAttribute('visibility', 'visible');
    this.diatonicGlow.setAttribute('d', arcPath);
    this.diatonicGlow.setAttribute('visibility', 'visible');
  }

  private updateTrails(roots: readonly number[]): void {
    const C = STUDIO_PALETTE.circle;
    const segmentCount = Math.max(0, roots.length - 1);

    for (let index = 0; index < this.trails.length; index += 1) {
      const segment = this.trails[index];
      if (index >= segmentCount) {
        segment.line.setAttribute('visibility', 'hidden');
        segment.glow.setAttribute('visibility', 'hidden');
        continue;
      }

      const fromPc = roots[index];
      const toPc = roots[index + 1];
      if (((fromPc % 12) + 12) % 12 === ((toPc % 12) + 12) % 12) {
        segment.line.setAttribute('visibility', 'hidden');
        segment.glow.setAttribute('visibility', 'hidden');
        continue;
      }

      const fromAngle = circleOfFifthsAngle(fromPc);
      const toAngle = circleOfFifthsAngle(toPc);
      const x1 = C.center + Math.cos(fromAngle) * C.trailRadius;
      const y1 = C.center + Math.sin(fromAngle) * C.trailRadius;
      const x2 = C.center + Math.cos(toAngle) * C.trailRadius;
      const y2 = C.center + Math.sin(toAngle) * C.trailRadius;

      const recency = (index + 1) / segmentCount;
      const opacity = 0.22 + recency * 0.72;
      const width = C.trailWidthMin + (C.trailWidthMax - C.trailWidthMin) * recency;

      segment.line.setAttribute('x1', x1.toFixed(2));
      segment.line.setAttribute('y1', y1.toFixed(2));
      segment.line.setAttribute('x2', x2.toFixed(2));
      segment.line.setAttribute('y2', y2.toFixed(2));
      segment.line.setAttribute('stroke-width', width.toFixed(2));
      segment.line.setAttribute('opacity', opacity.toFixed(2));
      segment.line.setAttribute('visibility', 'visible');

      segment.glow.setAttribute('x1', x1.toFixed(2));
      segment.glow.setAttribute('y1', y1.toFixed(2));
      segment.glow.setAttribute('x2', x2.toFixed(2));
      segment.glow.setAttribute('y2', y2.toFixed(2));
      segment.glow.setAttribute('stroke-width', (width * 2.8).toFixed(2));
      segment.glow.setAttribute('opacity', (opacity * 0.45).toFixed(2));
      segment.glow.setAttribute('visibility', 'visible');
    }

    if (roots.length >= 2) {
      const latestAngle = circleOfFifthsAngle(roots[roots.length - 1]);
      this.trailHead.setAttribute(
        'cx',
        (C.center + Math.cos(latestAngle) * C.trailRadius).toFixed(2),
      );
      this.trailHead.setAttribute(
        'cy',
        (C.center + Math.sin(latestAngle) * C.trailRadius).toFixed(2),
      );
      this.trailHead.setAttribute('visibility', 'visible');
    } else {
      this.trailHead.setAttribute('visibility', 'hidden');
    }
  }

  private updateVertices(
    currentKey: VisualKey | undefined,
    activeRootPc: number | undefined,
    chordTonesPc: readonly number[] | undefined,
  ): void {
    const C = STUDIO_PALETTE.circle;
    const diatonicSlots = new Set<number>();
    if (currentKey?.scalePcs) {
      for (const pc of currentKey.scalePcs) {
        diatonicSlots.add(circleOfFifthsIndex(pc));
      }
    }
    const tonicSlot =
      currentKey?.tonicPc !== undefined ? circleOfFifthsIndex(currentKey.tonicPc) : -1;
    const activeRootSlot = activeRootPc !== undefined ? circleOfFifthsIndex(activeRootPc) : -1;
    const chordToneSlots = new Set<number>();
    if (chordTonesPc) {
      for (const pc of chordTonesPc) {
        chordToneSlots.add(circleOfFifthsIndex(pc));
      }
    }

    for (const vertex of this.vertices) {
      const isRoot = vertex.slotIndex === activeRootSlot;
      const isTonic = vertex.slotIndex === tonicSlot;
      const isDiatonic = diatonicSlots.has(vertex.slotIndex);
      const isChordTone = chordToneSlots.has(vertex.slotIndex);

      if (isRoot) {
        vertex.circle.setAttribute('r', String(C.tonicNodeRadius));
        vertex.circle.setAttribute('fill', C.activeRootFill);
        vertex.circle.setAttribute('stroke', C.activeRootStroke);
        vertex.circle.setAttribute('stroke-width', '2');
        vertex.halo.setAttribute('stroke', STUDIO_PALETTE.accent.amberGlow);
        vertex.halo.setAttribute('opacity', '0.9');
        vertex.text.setAttribute('fill', STUDIO_PALETTE.accent.amberBright);
      } else if (isTonic) {
        vertex.circle.setAttribute('r', String(C.tonicNodeRadius));
        vertex.circle.setAttribute('fill', C.tonicNodeFill);
        vertex.circle.setAttribute('stroke', C.tonicNodeStroke);
        vertex.circle.setAttribute('stroke-width', '1.75');
        vertex.halo.setAttribute('stroke', 'none');
        vertex.halo.setAttribute('opacity', '0');
        vertex.text.setAttribute('fill', STUDIO_PALETTE.text.primary);
      } else if (isDiatonic || isChordTone) {
        vertex.circle.setAttribute('r', String(C.nodeRadius));
        vertex.circle.setAttribute('fill', C.diatonicNodeFill);
        vertex.circle.setAttribute('stroke', C.diatonicNodeStroke);
        vertex.circle.setAttribute('stroke-width', isChordTone ? '1.6' : '1.25');
        vertex.halo.setAttribute('stroke', 'none');
        vertex.halo.setAttribute('opacity', '0');
        vertex.text.setAttribute('fill', STUDIO_PALETTE.text.primary);
      } else {
        vertex.circle.setAttribute('r', String(C.nodeRadius));
        vertex.circle.setAttribute('fill', C.inactiveNodeFill);
        vertex.circle.setAttribute('stroke', C.inactiveNodeStroke);
        vertex.circle.setAttribute('stroke-width', '1.1');
        vertex.halo.setAttribute('stroke', 'none');
        vertex.halo.setAttribute('opacity', '0');
        vertex.text.setAttribute('fill', STUDIO_PALETTE.text.muted);
      }
    }
  }

  private updateCenter(activeChord: { symbol: string; degree?: string } | undefined): void {
    if (!activeChord) {
      this.centerPrimary.textContent = '—';
      this.centerSecondary.textContent = '';
      return;
    }

    const formattedSymbol = formatMusicSymbol(activeChord.symbol);
    if (activeChord.degree !== undefined && activeChord.degree.length > 0) {
      this.centerPrimary.textContent = formatMusicSymbol(activeChord.degree);
      this.centerSecondary.textContent = formattedSymbol;
    } else {
      this.centerPrimary.textContent = formattedSymbol;
      this.centerSecondary.textContent = '';
    }
  }

  private updateFooter(currentKey: VisualKey | undefined): void {
    if (currentKey) {
      this.keyReadout.textContent = formatKeyLabel(currentKey);
      this.footer.hidden = false;
    } else {
      this.keyReadout.textContent = '';
      this.footer.hidden = true;
    }
  }
}

function describeArc(
  cx: number,
  cy: number,
  radius: number,
  startAngle: number,
  endAngle: number,
): string {
  const span = endAngle - startAngle;
  if (span >= Math.PI * 1.99) {
    const mid = startAngle + Math.PI;
    return `${describeArc(cx, cy, radius, startAngle, mid)} ${describeArc(cx, cy, radius, mid, endAngle)}`;
  }
  const x1 = cx + Math.cos(startAngle) * radius;
  const y1 = cy + Math.sin(startAngle) * radius;
  const x2 = cx + Math.cos(endAngle) * radius;
  const y2 = cy + Math.sin(endAngle) * radius;
  const largeArcFlag = span > Math.PI ? 1 : 0;
  return `M ${x1.toFixed(2)} ${y1.toFixed(2)} A ${radius} ${radius} 0 ${largeArcFlag} 1 ${x2.toFixed(2)} ${y2.toFixed(2)}`;
}
