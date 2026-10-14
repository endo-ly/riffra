// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import type { AutomationLane } from '@/model/domain';
import type { AutomationOption } from '../model/automation-targets';
import { AutomationLaneView } from './AutomationLaneView';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it('edits Step curves, cancels point gestures, and preserves point identities through edits', () => {
  // Arrange
  vi.stubGlobal('PointerEvent', MouseEvent);
  const option: AutomationOption = {
    target: { kind: 'masterGain' },
    name: 'Master Gain',
    staticValue: 0,
    min: -90,
    max: 0,
    unit: 'dB',
  };
  const committed = vi.fn();
  function Harness() {
    const [lane, setLane] = useState<AutomationLane>({
      id: 'lane',
      target: option.target,
      interpolation: 'linear',
      points: [
        { id: 'a', tick: 0, value: -12 },
        { id: 'b', tick: 20, value: 0 },
      ],
    });
    return (
      <AutomationLaneView
        option={option}
        options={[option]}
        lane={lane}
        timelineWidth={100}
        pixelsPerTick={1}
        snapTick={Math.round}
        onTarget={() => undefined}
        onClose={() => undefined}
        onCommit={(points, interpolation) => {
          committed(points, interpolation);
          setLane({ ...lane, points, interpolation });
        }}
      />
    );
  }
  const { container } = render(<Harness />);
  const lane = screen.getByLabelText('Master Gain automation');

  // Act / Assert
  fireEvent.change(screen.getByLabelText('Automation interpolation'), {
    target: { value: 'step' },
  });
  expect(container.querySelector('polyline')).toHaveAttribute(
    'points',
    '0,17.066666666666666 0,17.066666666666666 20,17.066666666666666 20,0 100,0',
  );
  fireEvent.pointerDown(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' }), {
    button: 0,
  });
  fireEvent.pointerMove(window, { clientX: 5, clientY: 25 });
  fireEvent.keyDown(window, { key: 'Escape' });
  fireEvent.pointerUp(window);
  expect(committed).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' })).toHaveStyle({
    left: '0px',
  });
  fireEvent.pointerDown(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' }), {
    button: 0,
  });
  fireEvent.pointerMove(window, { clientX: 10, clientY: 64 });
  fireEvent.pointerUp(window);
  expect(committed).toHaveBeenLastCalledWith(
    [
      { id: 'a', tick: 10, value: -45 },
      { id: 'b', tick: 20, value: 0 },
    ],
    'step',
  );
  fireEvent.keyDown(lane, { key: 'Delete' });
  expect(committed).toHaveBeenLastCalledWith([{ id: 'b', tick: 20, value: 0 }], 'step');
  fireEvent.pointerDown(lane, { button: 0, clientX: 20, clientY: 96 });
  expect(committed).toHaveBeenLastCalledWith([{ id: 'b', tick: 20, value: -67.5 }], 'step');
  fireEvent.keyDown(lane, { key: 'Delete' });
  expect(committed).toHaveBeenLastCalledWith([], 'step');
});
