// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MidiClip } from '@/model/domain';
import { MidiEditorPanel } from './MidiEditorPanel';

afterEach(cleanup);

const timebase = {
  ppq: 960,
  tempoChanges: [{ tick: 0, bpm: 120 }],
  timeSignatureChanges: [{ tick: 0, numerator: 4, denominator: 4 }],
};

function setup(startTick: number) {
  const clip: MidiClip = {
    instrumentControlEvents: [],
    id: 'clip:1',
    name: 'Clip',
    trackId: 'track:1',
    startTick,
    durationTicks: 3840,
    notes: [
      { id: 'note:1', note: 60, startTick: 100, durationTicks: 200, velocity: 96, channel: 1 },
    ],
    events: [],
    muted: false,
    loopEnabled: false,
  };
  const onAddNote = vi.fn();
  const onUpdateNote = vi.fn();
  const onUpdateNotes = vi.fn();
  const onInsertNotes = vi.fn();
  const playheadTickRef = { current: startTick };
  render(
    <MidiEditorPanel
      clip={clip}
      trackColor={null}
      timebase={timebase}
      playheadTick={startTick}
      playheadTickRef={playheadTickRef}
      playing={false}
      onRemoveNotes={() => undefined}
      onAddNote={onAddNote}
      onUpdateNote={onUpdateNote}
      onUpdateNotes={onUpdateNotes}
      onInsertNotes={onInsertNotes}
    />,
  );
  const lane = document.querySelector<HTMLElement>('[data-midi-lane]')!;
  Object.defineProperty(lane, 'getBoundingClientRect', {
    value: () => ({ left: 0, top: 0, width: 691.2, height: 1536 }),
  });
  return { lane, onAddNote, onUpdateNote, onUpdateNotes, onInsertNotes, playheadTickRef };
}

function positions(root: Element, selector: string) {
  return [...root.querySelectorAll<HTMLElement>(selector)].map((line) =>
    Number.parseFloat(line.style.left),
  );
}

describe('MidiEditorPanel', () => {
  it.each([1920, 2160])(
    'aligns all grid layers to Arrangement boundaries at clip tick %i',
    (startTick) => {
      // Arrange / Act
      const { lane } = setup(startTick);
      const velocity = document.querySelector('[data-velocity-lane]')!;
      const bar = (3840 - startTick) * 0.18;
      const beats = [2880, 3840, 4800, ...(startTick === 1920 ? [1920] : [5760])]
        .sort((a, b) => a - b)
        .map((tick) => (tick - startTick) * 0.18);

      // Assert
      expect(positions(lane, '[class*="barLine"]')).toEqual([bar]);
      expect(positions(velocity, '[class*="velocityBarLine"]')).toEqual([bar]);
      expect(positions(lane, '[class*="beatLine"]')).toEqual(beats);
      expect(positions(velocity, '[class*="velocityBeatLine"]')).toEqual(beats);
      const ruler = screen.getByLabelText('MIDI editor ruler');
      expect(positions(ruler, 'i:has(strong)').slice(1)).toEqual([bar]);
      fireEvent.change(screen.getByLabelText('Snap'), { target: { value: '1/8t' } });
      const subdivisions = positions(lane, '[data-grid-subdivision]');
      expect(subdivisions[0]).toBeCloseTo((startTick === 2160 ? 80 : 320) * 0.18);
      for (const left of subdivisions) {
        expect((startTick + left / 0.18) / 320).toBeCloseTo(
          Math.round((startTick + left / 0.18) / 320),
        );
      }
    },
  );

  it('snaps creation, drawing, movement and right-edge resizing to the visible beat lines', () => {
    // Arrange
    const { lane, onAddNote, onUpdateNote, onUpdateNotes, onInsertNotes, playheadTickRef } =
      setup(2160);
    fireEvent.change(screen.getByLabelText('Snap'), { target: { value: '1/4' } });
    const note = lane.querySelector<HTMLElement>('[data-note-id="note:1"]')!;

    // Act / Assert: double-click and Draw use the same boundaries.
    fireEvent.doubleClick(lane, { clientX: 70, clientY: 804 });
    expect(onAddNote).toHaveBeenLastCalledWith('clip:1', 720, 60, 960, 96, 1);
    fireEvent.click(screen.getByRole('button', { name: 'Draw' }));
    fireEvent.pointerDown(lane, { clientX: 70, clientY: 804, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 320, clientY: 804, pointerId: 1 });
    expect(onAddNote).toHaveBeenLastCalledWith('clip:1', 720, 60, 960, 96, 1);

    fireEvent.click(screen.getByRole('button', { name: 'Pointer' }));
    fireEvent.pointerDown(note, { clientX: 100, clientY: 804, pointerId: 2 });
    fireEvent.pointerUp(window, { clientX: 170, clientY: 804, pointerId: 2 });
    expect(onUpdateNote).toHaveBeenLastCalledWith(
      'clip:1',
      expect.objectContaining({ startTick: 720 }),
    );
    const resize = note.querySelector<HTMLElement>('[class*="resizeHandle"]')!;
    fireEvent.pointerDown(resize, { clientX: 100, clientY: 804, pointerId: 3 });
    fireEvent.pointerUp(window, { clientX: 250, clientY: 804, pointerId: 3 });
    expect(onUpdateNote).toHaveBeenLastCalledWith(
      'clip:1',
      expect.objectContaining({ startTick: 100, durationTicks: 620 }),
    );

    fireEvent.click(note);
    fireEvent.keyDown(screen.getByLabelText('MIDI Editor'), { key: 'ArrowRight' });
    expect(onUpdateNotes).toHaveBeenLastCalledWith('clip:1', [
      { noteId: 'note:1', patch: { startTick: 720, note: 60 } },
    ]);
    playheadTickRef.current = 2160 + 350;
    fireEvent.keyDown(screen.getByLabelText('MIDI Editor'), { key: 'c', ctrlKey: true });
    fireEvent.keyDown(screen.getByLabelText('MIDI Editor'), { key: 'v', ctrlKey: true });
    expect(onInsertNotes).toHaveBeenLastCalledWith('clip:1', [
      { pitch: 60, startTick: 720, durationTicks: 200, velocity: 96, channel: 1 },
    ]);
  });
});
