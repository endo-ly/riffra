import { formatMusicSymbol } from '../../format';
import type { VisualSnapshot } from '../../generated';
import { findChordWindow } from '../../query';

const SLOT_COUNT = 4;

interface ChordSlotNodes {
  card: HTMLDivElement;
  primary: HTMLDivElement;
  secondary: HTMLDivElement;
  progressFill: HTMLDivElement;
}

/**
 * Renders the 4-slot chord progression strip centered on the sounding chord.
 *
 * All slot DOM elements are allocated once on creation and updated in place
 * during `render()`.
 */
export class ChordStripView {
  readonly element: HTMLDivElement;
  private readonly slots: ChordSlotNodes[] = [];
  private readonly emptyState: HTMLDivElement;

  constructor(doc: Document) {
    this.element = doc.createElement('div');
    this.element.className = 'sc-chord-strip';
    this.element.setAttribute('data-role', 'chord-strip');

    for (let index = 0; index < SLOT_COUNT; index += 1) {
      const card = doc.createElement('div');
      card.className = 'sc-chord-card';
      card.dataset.active = 'false';

      const primary = doc.createElement('div');
      primary.className = 'sc-chord-primary';

      const secondary = doc.createElement('div');
      secondary.className = 'sc-chord-secondary';

      const progressTrack = doc.createElement('div');
      progressTrack.className = 'sc-chord-progress-track';

      const progressFill = doc.createElement('div');
      progressFill.className = 'sc-chord-progress-fill';
      progressTrack.appendChild(progressFill);

      card.append(primary, secondary, progressTrack);
      this.element.appendChild(card);
      this.slots.push({ card, primary, secondary, progressFill });
    }

    this.emptyState = doc.createElement('div');
    this.emptyState.className = 'sc-chord-empty';
    this.emptyState.textContent = 'NO HARMONY TRACK';
    this.emptyState.hidden = true;
    this.element.appendChild(this.emptyState);
  }

  render(snapshot: VisualSnapshot, timeSec: number): void {
    if (snapshot.chords.length === 0) {
      this.emptyState.hidden = false;
      for (const slot of this.slots) {
        slot.card.hidden = true;
        slot.card.dataset.active = 'false';
        slot.progressFill.style.width = '0%';
      }
      return;
    }

    this.emptyState.hidden = true;
    const window = findChordWindow(snapshot, timeSec, SLOT_COUNT);

    for (let index = 0; index < SLOT_COUNT; index += 1) {
      const slot = this.slots[index];
      const chord = window.chords[index];
      if (!chord) {
        slot.card.hidden = true;
        slot.card.dataset.active = 'false';
        slot.progressFill.style.width = '0%';
        continue;
      }

      slot.card.hidden = false;
      const isActive = index === window.current;
      slot.card.dataset.active = isActive ? 'true' : 'false';

      const formattedSymbol = formatMusicSymbol(chord.symbol);
      if (chord.degree !== undefined && chord.degree.length > 0) {
        slot.primary.textContent = formatMusicSymbol(chord.degree);
        slot.secondary.textContent = formattedSymbol;
        slot.secondary.hidden = false;
      } else {
        slot.primary.textContent = formattedSymbol;
        slot.secondary.textContent = '';
        slot.secondary.hidden = true;
      }

      slot.progressFill.style.width = isActive ? `${(window.progress * 100).toFixed(2)}%` : '0%';
    }
  }
}
