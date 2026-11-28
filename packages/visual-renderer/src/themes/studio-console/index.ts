import { formatKeyLabel } from '../../format';
import type { VisualSnapshot } from '../../generated';
import { findSectionAt, findTempoAt } from '../../query';
import type { ViewState, VisualLayout, VisualTheme } from '../../types';
import { ChordStripView } from './chord-strip';
import { CircleOfFifthsView } from './circle-of-fifths';
import { PianoRollView } from './piano-roll';
import { PulseLaneView } from './pulse-lane';
import { STUDIO_CONSOLE_STYLES } from './styles';

const SUPPORTED_LAYOUTS: readonly VisualLayout[] = ['16:9', '9:16', 'free'];

interface MountedInstance {
  styleEl: HTMLStyleElement;
  root: HTMLDivElement;
  header: HTMLElement;
  titleEl: HTMLHeadingElement;
  sectionPill: HTMLSpanElement;
  keyPill: HTMLSpanElement;
  bpmPill: HTMLSpanElement;
  chordStrip: ChordStripView;
  pianoRoll: PianoRollView;
  circleOfFifths: CircleOfFifthsView;
  pulseLane: PulseLaneView;
  resizeObserver: ResizeObserver | null;
  lastRender: {
    snapshot: VisualSnapshot;
    timeSec: number;
    state: ViewState;
  } | null;
}

/**
 * Creates a fresh `studio-console` theme instance.
 *
 * Useful when mounting multiple independent views concurrently without sharing
 * internal DOM handles.
 */
export function createStudioConsoleTheme(): VisualTheme {
  let mounted: MountedInstance | null = null;

  const renderInto = (
    instance: MountedInstance,
    snapshot: VisualSnapshot,
    timeSec: number,
    state: ViewState,
  ): void => {
    const layout = state.layout ?? 'free';
    instance.root.dataset.layout = layout;

    const showHeader = state.showHeader !== false;
    instance.header.hidden = !showHeader;
    if (showHeader) {
      instance.titleEl.textContent = snapshot.title || 'Untitled';
      const sectionWindow = findSectionAt(snapshot, timeSec);
      const sectionName = sectionWindow.current?.name;
      if (sectionName) {
        instance.sectionPill.textContent = sectionName;
        instance.sectionPill.hidden = false;
      } else {
        instance.sectionPill.textContent = '';
        instance.sectionPill.hidden = true;
      }

      const key = sectionWindow.current?.key;
      if (key) {
        instance.keyPill.textContent = formatKeyLabel(key);
        instance.keyPill.hidden = false;
      } else {
        instance.keyPill.textContent = '';
        instance.keyPill.hidden = true;
      }

      const tempo = findTempoAt(snapshot, timeSec);
      if (tempo) {
        const bpm = Number.isInteger(tempo.bpm) ? String(tempo.bpm) : tempo.bpm.toFixed(1);
        instance.bpmPill.textContent = `${bpm} BPM`;
        instance.bpmPill.hidden = false;
      } else {
        instance.bpmPill.textContent = '';
        instance.bpmPill.hidden = true;
      }
    }

    instance.chordStrip.render(snapshot, timeSec);
    instance.pianoRoll.render(snapshot, timeSec, state);
    instance.circleOfFifths.render(snapshot, timeSec);
    instance.pulseLane.render(snapshot, timeSec);
    instance.lastRender = { snapshot, timeSec, state };
  };

  const unmount = (): void => {
    if (!mounted) return;
    mounted.resizeObserver?.disconnect();
    mounted.pianoRoll.unmount();
    mounted.root.remove();
    mounted.styleEl.remove();
    mounted = null;
  };

  return {
    id: 'studio-console',
    name: 'Studio Console',
    description:
      'Hardware-inspired dark studio visualizer combining chord progression, scrolling piano roll with CC64 sustain, Circle of Fifths, and step pulse lanes.',
    supportedLayouts: SUPPORTED_LAYOUTS,

    mount(container: HTMLElement, snapshot: VisualSnapshot): void {
      if (mounted) unmount();

      const doc = container.ownerDocument;
      const styleEl = doc.createElement('style');
      styleEl.setAttribute('data-theme', 'studio-console');
      styleEl.textContent = STUDIO_CONSOLE_STYLES;

      const root = doc.createElement('div');
      root.className = 'sc-root';
      root.dataset.theme = 'studio-console';
      root.dataset.layout = 'free';

      // Header
      const header = doc.createElement('header');
      header.className = 'sc-header';
      header.setAttribute('data-role', 'header');

      const brand = doc.createElement('div');
      brand.className = 'sc-header-brand';

      const led = doc.createElement('span');
      led.className = 'sc-status-led';
      led.setAttribute('aria-hidden', 'true');

      const titleEl = doc.createElement('h1');
      titleEl.className = 'sc-title';
      titleEl.textContent = snapshot.title || 'Untitled';

      brand.append(led, titleEl);

      const meta = doc.createElement('div');
      meta.className = 'sc-header-meta';

      const sectionPill = doc.createElement('span');
      sectionPill.className = 'sc-meta-pill';
      sectionPill.setAttribute('data-role', 'header-section');
      sectionPill.hidden = true;

      const keyPill = doc.createElement('span');
      keyPill.className = 'sc-meta-pill sc-meta-pill--accent';
      keyPill.setAttribute('data-role', 'header-key');
      keyPill.hidden = true;

      const bpmPill = doc.createElement('span');
      bpmPill.className = 'sc-meta-pill';
      bpmPill.setAttribute('data-role', 'header-bpm');
      bpmPill.hidden = true;

      meta.append(sectionPill, keyPill, bpmPill);
      header.append(brand, meta);

      // Subviews
      const chordStrip = new ChordStripView(doc);
      const pianoRoll = new PianoRollView(doc);
      const circleOfFifths = new CircleOfFifthsView(doc);
      const pulseLane = new PulseLaneView(doc);

      pianoRoll.updateSnapshot(snapshot);
      pulseLane.updateSnapshot(snapshot);

      const main = doc.createElement('div');
      main.className = 'sc-main';
      main.append(pianoRoll.element, circleOfFifths.element);

      root.append(header, chordStrip.element, main, pulseLane.element);
      container.append(styleEl, root);

      const instance: MountedInstance = {
        styleEl,
        root,
        header,
        titleEl,
        sectionPill,
        keyPill,
        bpmPill,
        chordStrip,
        pianoRoll,
        circleOfFifths,
        pulseLane,
        resizeObserver: null,
        lastRender: null,
      };

      if (typeof ResizeObserver !== 'undefined') {
        const observer = new ResizeObserver(() => {
          if (mounted !== instance || !instance.lastRender) return;
          const { snapshot: lastSnap, timeSec, state } = instance.lastRender;
          renderInto(instance, lastSnap, timeSec, state);
        });
        observer.observe(root);
        instance.resizeObserver = observer;
      }

      mounted = instance;
    },

    updateSnapshot(snapshot: VisualSnapshot): void {
      if (!mounted) return;
      mounted.pianoRoll.updateSnapshot(snapshot);
      mounted.pulseLane.updateSnapshot(snapshot);
    },

    render(snapshot: VisualSnapshot, timeSec: number, state: ViewState): void {
      if (!mounted) return;
      renderInto(mounted, snapshot, timeSec, state);
    },

    unmount,
  };
}

/** Default shared Studio Console theme instance. */
export const studioConsoleTheme: VisualTheme = createStudioConsoleTheme();
