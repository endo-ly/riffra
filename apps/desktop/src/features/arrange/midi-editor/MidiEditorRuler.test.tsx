// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { createRef } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { MidiEditorRuler } from './MidiEditorRuler';

afterEach(cleanup);

const timebase = { ppq: 960, bpm: 120, timeSignatureNumerator: 4, timeSignatureDenominator: 4 };

function barLabels(clipStartTick: number): string[] {
  render(
    <MidiEditorRuler
      timebase={timebase}
      clipStartTick={clipStartTick}
      visibleTicks={2 * 3_840}
      pixelsPerTick={0.1}
      playheadRef={createRef()}
    />,
  );
  return [...screen.getByLabelText('MIDI editor ruler').querySelectorAll('strong')].map(
    (label) => label.textContent ?? '',
  );
}

describe('MidiEditorRuler', () => {
  it('labels downbeats by bar number and mid-bar starts with the beat', () => {
    expect(barLabels(3_840)).toEqual(['2', '3']);
    cleanup();
    expect(barLabels(2 * 960)).toEqual(['1.3', '2', '3']);
    const marks = screen.getByLabelText('MIDI editor ruler').querySelectorAll('i > strong');
    expect([...marks].map((label) => (label.parentElement as HTMLElement).style.left)).toEqual([
      '0px',
      '192px',
      '576px',
    ]);
    cleanup();
    expect(barLabels(240)).toEqual(['1.1', '2', '3']);
  });

  it('places beat boundaries after an off-beat clip start and inside the visible range', () => {
    // Arrange / Act
    expect(barLabels(2 * 960 + 240)).toEqual(['1.3', '2', '3']);
    const ruler = screen.getByLabelText('MIDI editor ruler');
    const sections = [...ruler.querySelectorAll('i:has(strong)')] as HTMLElement[];

    // Assert
    expect(sections.map((section) => section.style.left)).toEqual(['0px', '168px', '552px']);
    expect(
      sections.flatMap((section) =>
        [...section.querySelectorAll('span')].map(
          (beat) => Number.parseFloat(section.style.left) + Number.parseFloat(beat.style.left),
        ),
      ),
    ).toEqual([72, 264, 360, 456, 648, 744]);
  });
});
