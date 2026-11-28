// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { VisualSnapshot } from '../generated';
import { createStudioConsoleTheme } from '../themes/studio-console';
import type { ColorMode, ViewState } from '../types';

function buildFixture(overrides: Partial<VisualSnapshot> = {}): VisualSnapshot {
  return {
    schemaVersion: 1,
    title: 'Midnight Console',
    durationSec: 12,
    tempos: [{ t: 0, bpm: 120 }],
    meters: [{ t: 0, bar: 1, numerator: 4, denominator: 4 }],
    beats: [
      { t: 0, bar: 1, beat: 1, downbeat: true },
      { t: 0.5, bar: 1, beat: 2, downbeat: false },
      { t: 1.0, bar: 1, beat: 3, downbeat: false },
      { t: 1.5, bar: 1, beat: 4, downbeat: false },
      { t: 2.0, bar: 2, beat: 1, downbeat: true },
      { t: 2.5, bar: 2, beat: 2, downbeat: false },
      { t: 3.0, bar: 2, beat: 3, downbeat: false },
      { t: 3.5, bar: 2, beat: 4, downbeat: false },
      { t: 4.0, bar: 3, beat: 1, downbeat: true },
      { t: 4.5, bar: 3, beat: 2, downbeat: false },
      { t: 5.0, bar: 3, beat: 3, downbeat: false },
      { t: 5.5, bar: 3, beat: 4, downbeat: false },
    ],
    sections: [
      {
        name: 'Intro',
        occurrence: 1,
        startSec: 0,
        endSec: 4,
        key: { tonic: 'D', scale: 'aeolian', tonicPc: 2, scalePcs: [0, 2, 4, 5, 7, 9, 10] },
      },
      {
        name: 'Chorus',
        occurrence: 1,
        startSec: 4,
        endSec: 12,
        key: { tonic: 'F', scale: 'ionian', tonicPc: 5, scalePcs: [0, 2, 4, 5, 7, 9, 10] },
      },
    ],
    chords: [
      {
        startSec: 0,
        endSec: 2,
        symbol: 'Dm9',
        degree: 'Im9',
        rootPc: 2,
        tonesPc: [2, 5, 9, 0, 4],
      },
      {
        startSec: 2,
        endSec: 4,
        symbol: 'Bbmaj7(#11)',
        degree: 'bVImaj7(#11)',
        rootPc: 10,
        tonesPc: [10, 2, 5, 9, 4],
      },
      {
        startSec: 4,
        endSec: 6,
        symbol: 'Fmaj7',
        rootPc: 5,
        tonesPc: [5, 9, 0, 4],
      },
    ],
    tracks: [
      {
        id: 'piano',
        name: 'Grand Piano',
        category: 'lead',
        notes: [
          { startSec: 0, endSec: 1.5, midi: 74, velocity: 112, chordRole: 'root', top: true },
          { startSec: 0, endSec: 1.5, midi: 62, velocity: 78, chordRole: 'chordTone', top: false },
          { startSec: 2.0, endSec: 3.5, midi: 76, velocity: 96, chordRole: 'tension', top: true },
          { startSec: 4.0, endSec: 5.5, midi: 77, velocity: 104, chordRole: 'nonChord', top: true },
        ],
        sustain: [{ startSec: 0, endSec: 3.8 }],
      },
    ],
    drumLanes: [],
    ...overrides,
  };
}

function createCanvasRecorder(): {
  ctx: CanvasRenderingContext2D;
  calls: string[];
} {
  const calls: string[] = [];
  const gradient = { addColorStop: () => undefined };
  const ctx = {
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    globalAlpha: 1,
    font: '',
    save: () => calls.push('save'),
    restore: () => calls.push('restore'),
    scale: (x: number, y: number) => calls.push(`scale:${x},${y}`),
    fillRect: (x: number, y: number, w: number, h: number) =>
      calls.push(`fillRect:${x.toFixed(1)},${y.toFixed(1)},${w.toFixed(1)},${h.toFixed(1)}`),
    beginPath: () => calls.push('beginPath'),
    closePath: () => calls.push('closePath'),
    moveTo: (x: number, y: number) => calls.push(`moveTo:${x.toFixed(1)},${y.toFixed(1)}`),
    lineTo: (x: number, y: number) => calls.push(`lineTo:${x.toFixed(1)},${y.toFixed(1)}`),
    quadraticCurveTo: () => calls.push('quad'),
    arc: (x: number, y: number, r: number) =>
      calls.push(`arc:${x.toFixed(1)},${y.toFixed(1)},${r.toFixed(1)}`),
    fill: () => calls.push(`fill:${String(ctx.fillStyle)}`),
    stroke: () => calls.push(`stroke:${String(ctx.strokeStyle)}`),
    fillText: (text: string) => calls.push(`text:${text}`),
    createLinearGradient: () => gradient,
    createRadialGradient: () => gradient,
  } as unknown as CanvasRenderingContext2D;
  return { ctx, calls };
}

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('studio-console theme', () => {
  it('mounts, renders across timestamps and color modes without recreating DOM, and cleans up on unmount', () => {
    // Arrange
    const container = document.createElement('div');
    document.body.appendChild(container);
    const snapshot = buildFixture();
    const theme = createStudioConsoleTheme();

    const disconnectSpy = vi.fn();
    const observeSpy = vi.fn();
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe = observeSpy;
        unobserve = vi.fn();
        disconnect = disconnectSpy;
      },
    );

    const recorder = createCanvasRecorder();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(recorder.ctx);

    // Act
    theme.mount(container, snapshot);
    const rootBefore = container.querySelector('.sc-root');
    const firstCardBefore = container.querySelector('.sc-chord-card');
    const firstArcBefore = container.querySelector('[data-role="diatonic-arc"]');

    theme.render(snapshot, 2.5, { colorMode: 'harmony', layout: '16:9' });

    // Assert: chord strip formats degree as primary and symbol as secondary
    const activeCard = container.querySelector('.sc-chord-card[data-active="true"]');
    expect(activeCard?.querySelector('.sc-chord-primary')?.textContent).toBe('♭VImaj7(♯11)');
    expect(activeCard?.querySelector('.sc-chord-secondary')?.textContent).toBe('B♭maj7(♯11)');
    expect(container.querySelector('[data-role="header-section"]')?.textContent).toBe('Intro');
    expect(container.querySelector('[data-role="header-bpm"]')?.textContent).toBe('120 BPM');
    expect(firstArcBefore?.getAttribute('visibility')).toBe('visible');

    // Act: advance across modulation and switch through all 3 color modes
    const modes: ColorMode[] = ['harmony', 'velocity', 'track'];
    for (const mode of modes) {
      theme.render(snapshot, 4.2, { colorMode: mode, layout: '9:16' });
    }

    // Assert: same DOM nodes reused, chord without degree promotes symbol to primary
    expect(container.querySelector('.sc-root')).toBe(rootBefore);
    expect(container.querySelector('.sc-chord-card')).toBe(firstCardBefore);
    expect(container.querySelector('[data-role="diatonic-arc"]')).toBe(firstArcBefore);
    expect(container.querySelector('.sc-root')?.getAttribute('data-layout')).toBe('9:16');
    expect(
      container.querySelector('.sc-chord-card[data-active="true"] .sc-chord-primary')?.textContent,
    ).toBe('Fmaj7');
    expect(recorder.calls.length).toBeGreaterThan(0);

    // Act & Assert: unmount releases observer and removes DOM
    theme.unmount();
    expect(disconnectSpy).toHaveBeenCalledOnce();
    expect(container.children).toHaveLength(0);
  });

  it('handles missing key, empty chords, and hidden header gracefully', () => {
    // Arrange
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
    const container = document.createElement('div');
    document.body.appendChild(container);
    const snapshot = buildFixture({
      sections: [{ name: 'Unkeyed', occurrence: 1, startSec: 0, endSec: 12 }],
      chords: [],
    });
    const theme = createStudioConsoleTheme();

    // Act
    theme.mount(container, snapshot);
    theme.render(snapshot, 1.0, { colorMode: 'harmony', showHeader: false });

    // Assert
    expect((container.querySelector('[data-role="header"]') as HTMLElement).hidden).toBe(true);
    expect((container.querySelector('.sc-chord-empty') as HTMLElement).hidden).toBe(false);
    expect(container.querySelector('[data-role="diatonic-arc"]')?.getAttribute('visibility')).toBe(
      'hidden',
    );
    expect((container.querySelector('[data-role="circle-key"]') as HTMLElement).hidden).toBe(true);
    expect(container.querySelector('[data-role="circle-bar"]')?.textContent).toBe('Bar 1');

    theme.unmount();
  });

  it('maps piano-roll clicks to clamped seek times and detaches listeners on unmount', () => {
    // Arrange
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
    const container = document.createElement('div');
    document.body.appendChild(container);
    const snapshot = buildFixture({ durationSec: 10 });
    const theme = createStudioConsoleTheme();
    const onSeek = vi.fn();

    theme.mount(container, snapshot);
    theme.render(snapshot, 2.0, { colorMode: 'harmony', onSeek });

    const canvas = container.querySelector('canvas') as HTMLCanvasElement;
    vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue({
      left: 100,
      top: 0,
      width: 400,
      height: 200,
      right: 500,
      bottom: 200,
      x: 100,
      y: 0,
      toJSON: () => ({}),
    });

    // Act: click at playhead ratio (28% of 400 = 112px from left -> 2.0s) and past right edge (clamped to durationSec)
    canvas.dispatchEvent(new MouseEvent('click', { clientX: 212, bubbles: true }));
    canvas.dispatchEvent(new MouseEvent('click', { clientX: 1000, bubbles: true }));
    canvas.dispatchEvent(new MouseEvent('click', { clientX: 0, bubbles: true }));

    // Assert
    expect(onSeek).toHaveBeenCalledTimes(3);
    expect(onSeek.mock.calls[0][0]).toBeCloseTo(2.0, 2);
    expect(onSeek.mock.calls[1][0]).toBe(10);
    expect(onSeek.mock.calls[2][0]).toBe(0);

    // Unmount detaches listener
    theme.unmount();
    canvas.dispatchEvent(new MouseEvent('click', { clientX: 212, bubbles: true }));
    expect(onSeek).toHaveBeenCalledTimes(3);
  });

  it('produces identical DOM and canvas output for the same inputs regardless of prior render history', () => {
    // Arrange
    const snapshot = buildFixture();
    const state: ViewState = { colorMode: 'harmony', layout: '16:9' };

    const containerA = document.createElement('div');
    const containerB = document.createElement('div');
    document.body.append(containerA, containerB);

    const recorderA = createCanvasRecorder();
    const recorderB = createCanvasRecorder();

    const themeA = createStudioConsoleTheme();
    const themeB = createStudioConsoleTheme();

    themeA.mount(containerA, snapshot);
    themeB.mount(containerB, snapshot);

    const canvasA = containerA.querySelector('canvas') as HTMLCanvasElement;
    const canvasB = containerB.querySelector('canvas') as HTMLCanvasElement;
    vi.spyOn(canvasA, 'getContext').mockReturnValue(recorderA.ctx);
    vi.spyOn(canvasB, 'getContext').mockReturnValue(recorderB.ctx);

    // Act: themeA jumps directly to 4.2s; themeB renders 0s -> 8s -> 4.2s
    themeA.render(snapshot, 4.2, state);

    themeB.render(snapshot, 0, state);
    themeB.render(snapshot, 8.0, state);
    recorderB.calls.length = 0;
    themeB.render(snapshot, 4.2, state);

    // Assert
    expect(containerB.innerHTML).toBe(containerA.innerHTML);
    expect(recorderB.calls).toEqual(recorderA.calls);

    themeA.unmount();
    themeB.unmount();
  });
});
