// @vitest-environment jsdom
import { defaultSession } from '@/native/browser-defaults';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import type { AutomationLane } from '@/model/generated';
import { automationValueAt } from '../model/automation-value';
import type { AutomationOption } from '../model/automation-targets';
import { AutomationLaneView } from './AutomationLaneView';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it('switches step interpolation at the point tick like Native automation', () => {
  // Arrange
  const points = [
    { id: 'before', tick: 0, value: 0 },
    { id: 'boundary', tick: 960, value: 1 },
  ];

  // Act
  const values = [959, 960, 961].map((tick) =>
    automationValueAt(points, 'step', tick, 0.5, defaultSession().arrangement.timebase),
  );

  // Assert
  expect(values).toEqual([0, 1, 1]);
});

it('previews point drags, cancels transient values, and commits one canonical lane edit', async () => {
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
  const previewed = vi.fn();
  let previewScope = 'project:host';
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
        timebase={defaultSession().arrangement.timebase}
        option={option}
        options={[option]}
        lane={lane}
        timelineWidth={100}
        pixelsPerTick={1}
        playheadTick={10}
        playheadTickRef={{ current: 10 }}
        previewScope={previewScope}
        snapTick={Math.round}
        onPreview={previewed}
        onTarget={() => undefined}
        onClose={() => undefined}
        onCommit={(points, interpolation) => {
          committed(points, interpolation);
          setLane({ ...lane, points, interpolation });
        }}
      />
    );
  }
  const { container, rerender } = render(<Harness />);
  const lane = screen.getByLabelText('Master Gain automation');

  // Act / Assert
  fireEvent.change(screen.getByLabelText('Automation interpolation'), {
    target: { value: 'step' },
  });
  expect(container.querySelector('polyline')).toHaveAttribute(
    'points',
    '0,17.066666666666666 0,17.066666666666666 20,17.066666666666666 20,0 100,0',
  );
  expect(screen.getByText(/Current:/)).toHaveTextContent('Current: -12.000 dB');
  fireEvent.pointerDown(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' }), {
    button: 0,
  });
  fireEvent.pointerMove(window, { clientX: 5, clientY: 25 });
  await waitFor(() => expect(previewed).toHaveBeenCalled());
  const canceledPreview = previewed.mock.lastCall;
  expect(canceledPreview?.[0]).toBeCloseTo(-17.6);
  fireEvent.keyDown(window, { key: 'Escape' });
  await waitFor(() => expect(previewed).toHaveBeenLastCalledWith(null, canceledPreview?.[1]));
  fireEvent.pointerDown(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' }), {
    button: 0,
  });
  fireEvent.pointerMove(window, { clientX: 5, clientY: 25 });
  await waitFor(() => expect(previewed.mock.lastCall?.[0]).toBeDefined());
  const switchedPreview = previewed.mock.lastCall;
  previewScope = 'project:other-host';
  rerender(<Harness />);
  await waitFor(() => expect(previewed).toHaveBeenLastCalledWith(null, switchedPreview?.[1]));
  fireEvent.pointerDown(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' }), {
    button: 0,
  });
  fireEvent.pointerMove(window, { clientX: 5, clientY: 25 });
  await waitFor(() => expect(previewed.mock.lastCall?.[0]).toBeDefined());
  const canceledByPointer = previewed.mock.lastCall;
  fireEvent.pointerCancel(window);
  await waitFor(() => expect(previewed).toHaveBeenLastCalledWith(null, canceledByPointer?.[1]));
  expect(committed).toHaveBeenCalledTimes(1);
  fireEvent.pointerUp(window);
  expect(committed).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' })).toHaveStyle({
    left: '0px',
  });
  fireEvent.pointerDown(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' }), {
    button: 0,
  });
  fireEvent.pointerMove(window, { clientX: 10, clientY: 64 });
  await waitFor(() => expect(previewed.mock.lastCall?.[0]).toBeDefined());
  const committedPreview = previewed.mock.lastCall;
  expect(committedPreview?.[0]).toBe(-45);
  fireEvent.pointerUp(window);
  await waitFor(() => expect(previewed).toHaveBeenLastCalledWith(null, committedPreview?.[1]));
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

it('coalesces pending drag previews and sends the final clear after an in-flight request', async () => {
  // Arrange
  vi.stubGlobal('PointerEvent', MouseEvent);
  let releaseFirstPreview!: () => void;
  let releaseLatestPreview!: () => void;
  const firstPreview = new Promise<void>((resolve) => {
    releaseFirstPreview = resolve;
  });
  const latestPreview = new Promise<void>((resolve) => {
    releaseLatestPreview = resolve;
  });
  let previewCalls = 0;
  const previewed = vi.fn((_value: number | null) => {
    previewCalls += 1;
    if (previewCalls === 1) return firstPreview;
    if (previewCalls === 2) return latestPreview;
    return Promise.resolve();
  });
  const option: AutomationOption = {
    target: { kind: 'masterGain' },
    name: 'Master Gain',
    staticValue: 0,
    min: -90,
    max: 0,
    unit: 'dB',
  };
  const committed = vi.fn();
  render(
    <AutomationLaneView
      timebase={defaultSession().arrangement.timebase}
      option={option}
      options={[option]}
      lane={{
        id: 'lane',
        target: option.target,
        interpolation: 'linear',
        points: [{ id: 'point', tick: 0, value: -12 }],
      }}
      timelineWidth={100}
      pixelsPerTick={1}
      playheadTick={0}
      playheadTickRef={{ current: 0 }}
      previewScope="project:host"
      snapTick={Math.round}
      onPreview={previewed}
      onTarget={() => undefined}
      onClose={() => undefined}
      onCommit={committed}
    />,
  );

  // Act
  fireEvent.pointerDown(screen.getByRole('button', { name: 'Master Gain -12dB at tick 0' }), {
    button: 0,
  });
  fireEvent.pointerMove(window, { clientX: 2, clientY: 25 });
  fireEvent.pointerMove(window, { clientX: 4, clientY: 40 });
  fireEvent.pointerMove(window, { clientX: 6, clientY: 55 });

  // Assert: while a request is active, multiple moves collapse to the latest value.
  expect(previewed).toHaveBeenCalledTimes(1);
  releaseFirstPreview();
  await waitFor(() => expect(previewed).toHaveBeenCalledTimes(2));
  expect(previewed.mock.calls[1][0]).toBe(Math.round(((-90 * 55) / 128) * 10) / 10);

  // Act: PointerUp replaces any pending value with the final preview clear.
  fireEvent.pointerMove(window, { clientX: 8, clientY: 65 });
  fireEvent.pointerMove(window, { clientX: 10, clientY: 75 });
  fireEvent.pointerUp(window);

  // Assert
  expect(previewed).toHaveBeenCalledTimes(2);
  expect(committed).toHaveBeenCalledTimes(1);
  releaseLatestPreview();
  await waitFor(() => expect(previewed).toHaveBeenCalledTimes(3));
  expect(previewed.mock.calls[2][0]).toBeNull();
});

it('limits VST3 display formatting to the latest playhead value at four requests per second', async () => {
  // Arrange
  vi.useFakeTimers();
  const option: AutomationOption = {
    target: { kind: 'deviceParameter', deviceId: 'effect:1', parameterId: 'cutoff' },
    name: 'Cutoff',
    staticValue: 0,
    min: 0,
    max: 1,
    unit: 'normalized',
  };
  const formatter = vi.fn(async (value: number) => `${value.toFixed(2)} Hz`);
  const view = (playheadTick: number) => (
    <AutomationLaneView
      timebase={defaultSession().arrangement.timebase}
      option={option}
      options={[option]}
      lane={{
        id: 'lane',
        target: option.target,
        interpolation: 'linear',
        points: [
          { id: 'start', tick: 0, value: 0 },
          { id: 'end', tick: 100, value: 1 },
        ],
      }}
      timelineWidth={100}
      pixelsPerTick={1}
      playheadTick={playheadTick}
      playheadTickRef={{ current: playheadTick }}
      previewScope="project:host"
      snapTick={Math.round}
      onPreview={() => undefined}
      formatValue={formatter}
      onTarget={() => undefined}
      onClose={() => undefined}
      onCommit={() => undefined}
    />
  );
  const { rerender } = render(view(0));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });

  // Act
  rerender(view(1));
  rerender(view(2));
  rerender(view(12));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(249);
  });
  expect(formatter).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });

  // Assert
  expect(formatter).toHaveBeenCalledTimes(2);
  expect(formatter).toHaveBeenLastCalledWith(0.12);
  expect(screen.getByText(/Current:/)).toHaveTextContent('Current: 0.12 Hz');
});
