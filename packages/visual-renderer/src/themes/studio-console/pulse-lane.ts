import type { VisualSnapshot } from '../../generated';
import { stepBounce } from '../../math';
import {
  derivePulseLanes,
  findMeterAt,
  resolveBeatGridPosition,
  type PulseHit,
  type PulseLane,
} from '../../query';
import { STUDIO_PALETTE } from './palette';

interface StepCellNodes {
  cell: HTMLDivElement;
  dot: HTMLDivElement;
}

interface LaneRowNodes {
  row: HTMLDivElement;
  cells: StepCellNodes[];
}

interface IndexedStepHit {
  t: number;
  accent: boolean;
}

interface IndexedLane {
  lane: PulseLane;
  byBar: Map<number, Map<number, IndexedStepHit>>;
}

/**
 * Meter-aware step-grid rhythm and MIDI pulse lane view.
 *
 * Rows and cells are built on snapshot or meter changes and updated purely via
 * attributes and inline deterministic bounce transforms during `render()`.
 */
export class PulseLaneView {
  readonly element: HTMLElement;
  private readonly doc: Document;
  private readonly titleEl: HTMLSpanElement;
  private readonly positionTag: HTMLSpanElement;
  private readonly body: HTMLDivElement;
  private readonly emptyState: HTMLDivElement;

  private cachedSnapshot: VisualSnapshot | null = null;
  private indexedLanes: IndexedLane[] = [];
  private rows: LaneRowNodes[] = [];
  private renderedStepsPerBar = 0;

  constructor(doc: Document) {
    this.doc = doc;
    this.element = doc.createElement('section');
    this.element.className = 'sc-panel sc-pulse-panel';
    this.element.setAttribute('data-role', 'pulse-lane');

    const header = doc.createElement('div');
    header.className = 'sc-panel-header';

    this.titleEl = doc.createElement('span');
    this.titleEl.className = 'sc-panel-title';
    this.titleEl.textContent = 'RHYTHM / MIDI PULSE';

    this.positionTag = doc.createElement('span');
    this.positionTag.className = 'sc-panel-tag';
    this.positionTag.setAttribute('data-role', 'pulse-position');
    this.positionTag.textContent = 'BAR 1 · BEAT 1';

    header.append(this.titleEl, this.positionTag);

    this.body = doc.createElement('div');
    this.body.className = 'sc-pulse-body';

    this.emptyState = doc.createElement('div');
    this.emptyState.className = 'sc-pulse-empty';
    this.emptyState.textContent = 'NO PULSE EVENTS';
    this.emptyState.hidden = true;
    this.body.appendChild(this.emptyState);

    this.element.append(header, this.body);
  }

  updateSnapshot(snapshot: VisualSnapshot): void {
    this.cachedSnapshot = snapshot;
    const lanes = derivePulseLanes(snapshot);
    this.indexedLanes = lanes.map((lane) => ({
      lane,
      byBar: indexHitsByBar(snapshot, lane.hits),
    }));
    this.renderedStepsPerBar = 0;
  }

  render(snapshot: VisualSnapshot, timeSec: number): void {
    if (this.cachedSnapshot !== snapshot) {
      this.updateSnapshot(snapshot);
    }

    const pos = resolveBeatGridPosition(snapshot, timeSec);
    const meter = findMeterAt(snapshot, timeSec);
    const stepsPerBeat = Math.max(1, Math.round(16 / (meter?.denominator ?? 4)));

    this.titleEl.textContent = `RHYTHM / MIDI PULSE (${pos.stepsPerBar}-STEP)`;
    this.positionTag.textContent = `BAR ${pos.bar} · BEAT ${pos.beat}`;

    if (this.indexedLanes.length === 0) {
      this.clearRows();
      this.emptyState.hidden = false;
      return;
    }

    this.emptyState.hidden = true;
    if (
      this.renderedStepsPerBar !== pos.stepsPerBar ||
      this.rows.length !== this.indexedLanes.length
    ) {
      this.rebuildGrid(pos.stepsPerBar, stepsPerBeat);
    }

    const bounceDuration = STUDIO_PALETTE.decay.pulseBounceDurationSec;

    for (let laneIndex = 0; laneIndex < this.indexedLanes.length; laneIndex += 1) {
      const indexed = this.indexedLanes[laneIndex];
      const rowNodes = this.rows[laneIndex];
      const barHits = indexed.byBar.get(pos.bar);

      for (let step = 0; step < pos.stepsPerBar; step += 1) {
        const cellNodes = rowNodes.cells[step];
        const hit = barHits?.get(step);
        const isCurrent = step === pos.step;

        cellNodes.cell.dataset.current = isCurrent ? 'true' : 'false';
        cellNodes.cell.dataset.beatStart = step % stepsPerBeat === 0 ? 'true' : 'false';

        if (!hit) {
          cellNodes.cell.dataset.hit = 'none';
          cellNodes.dot.removeAttribute('style');
          continue;
        }

        cellNodes.cell.dataset.hit = hit.accent ? 'accent' : 'regular';
        const bounce = stepBounce(timeSec - hit.t, bounceDuration);
        if (bounce > 0.005) {
          const scaleY = 1 + bounce * (hit.accent ? 0.55 : 0.42);
          const scaleX = 1 + bounce * 0.18;
          cellNodes.dot.style.transform = `scale(${scaleX.toFixed(3)}, ${scaleY.toFixed(3)})`;
          cellNodes.dot.style.filter = `brightness(${(1 + bounce * 0.55).toFixed(3)})`;
        } else {
          cellNodes.dot.removeAttribute('style');
        }
      }
    }
  }

  private clearRows(): void {
    for (const row of this.rows) {
      row.row.remove();
    }
    this.rows = [];
    this.renderedStepsPerBar = 0;
  }

  private rebuildGrid(stepsPerBar: number, stepsPerBeat: number): void {
    this.clearRows();
    this.renderedStepsPerBar = stepsPerBar;

    for (const indexed of this.indexedLanes) {
      const row = this.doc.createElement('div');
      row.className = 'sc-pulse-row';
      row.dataset.laneId = indexed.lane.id;

      const label = this.doc.createElement('span');
      label.className = 'sc-pulse-label';
      label.textContent = indexed.lane.name;

      const stepsContainer = this.doc.createElement('div');
      stepsContainer.className = 'sc-pulse-steps';
      stepsContainer.style.gridTemplateColumns = `repeat(${stepsPerBar}, minmax(0, 1fr))`;

      const cells: StepCellNodes[] = [];
      for (let step = 0; step < stepsPerBar; step += 1) {
        const cell = this.doc.createElement('div');
        cell.className = 'sc-pulse-step';
        cell.dataset.step = String(step);
        cell.dataset.current = 'false';
        cell.dataset.hit = 'none';
        cell.dataset.beatStart = step % stepsPerBeat === 0 ? 'true' : 'false';

        const dot = this.doc.createElement('div');
        dot.className = 'sc-pulse-dot';
        cell.appendChild(dot);

        stepsContainer.appendChild(cell);
        cells.push({ cell, dot });
      }

      row.append(label, stepsContainer);
      this.body.appendChild(row);
      this.rows.push({ row, cells });
    }
  }
}

function indexHitsByBar(
  snapshot: VisualSnapshot,
  hits: readonly PulseHit[],
): Map<number, Map<number, IndexedStepHit>> {
  const byBar = new Map<number, Map<number, IndexedStepHit>>();
  for (const hit of hits) {
    const pos = resolveBeatGridPosition(snapshot, hit.t);
    let steps = byBar.get(pos.bar);
    if (!steps) {
      steps = new Map<number, IndexedStepHit>();
      byBar.set(pos.bar, steps);
    }
    const existing = steps.get(pos.step);
    if (!existing || (hit.accent && !existing.accent)) {
      steps.set(pos.step, { t: hit.t, accent: hit.accent });
    }
  }
  return byBar;
}
