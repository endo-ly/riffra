// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { usePointerGesture } from '@/shared/usePointerGesture';

afterEach(cleanup);

function GestureHarness({ onCancel, onMove }: { onCancel: () => void; onMove: () => void }) {
  const startGesture = usePointerGesture();
  const [active, setActive] = useState('');

  const begin = (name: string) => (event: React.PointerEvent<HTMLDivElement>) => {
    startGesture(event, {
      onMove,
      onEnd: () => setActive(''),
      onCancel: () => {
        onCancel();
        setActive('');
      },
    });
    setActive(name);
  };

  return (
    <>
      <div data-testid="first-gesture" onPointerDown={begin('first')} />
      <div data-testid="second-gesture" onPointerDown={begin('second')} />
      <output>{active}</output>
    </>
  );
}

describe('usePointerGesture', () => {
  it('cancels once on pointercancel and when a new gesture starts', () => {
    const onCancel = vi.fn();
    const onMove = vi.fn();
    render(<GestureHarness onCancel={onCancel} onMove={onMove} />);

    fireEvent.pointerDown(screen.getByTestId('first-gesture'), { pointerId: 1 });
    fireEvent.pointerCancel(window, { pointerId: 1 });
    fireEvent.pointerMove(window, { pointerId: 1 });
    expect(onCancel).toHaveBeenCalledOnce();
    expect(onMove).not.toHaveBeenCalled();

    fireEvent.pointerDown(screen.getByTestId('first-gesture'), { pointerId: 2 });
    fireEvent.pointerDown(screen.getByTestId('second-gesture'), { pointerId: 3 });
    fireEvent.pointerMove(window, { pointerId: 2 });
    fireEvent.pointerMove(window, { pointerId: 3 });
    expect(onCancel).toHaveBeenCalledTimes(2);
    expect(onMove).toHaveBeenCalledOnce();
  });
});
