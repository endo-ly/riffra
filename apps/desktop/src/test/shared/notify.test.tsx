// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { notify } from '@/shared/notify';
import { ToastStack } from '@/shared/ui/message/ToastStack';

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('notify', () => {
  it('coalesces identical toasts and keeps only the newest three', () => {
    // Arrange
    render(<ToastStack />);

    // Act
    act(() => {
      notify.error('Export failed', { cause: new Error('disk full') });
      notify.error('Export failed', { cause: new Error('disk full') });
      notify.info('First');
    });

    // Assert
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(screen.getAllByRole('status')).toHaveLength(1);

    // Act
    act(() => {
      notify.info('Second');
      notify.info('Third');
    });

    // Assert
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.getAllByRole('status').map((toast) => toast.textContent)).toEqual([
      expect.stringContaining('First'),
      expect.stringContaining('Second'),
      expect.stringContaining('Third'),
    ]);
  });

  it('shows the cause of a failure and keeps it while the pointer rests on it', () => {
    // Arrange
    vi.useFakeTimers();
    render(<ToastStack />);
    act(() => notify.error('Export failed', { cause: new Error('disk full') }));
    const toast = screen.getByRole('alert');

    // Act
    fireEvent.pointerEnter(toast);
    act(() => {
      vi.advanceTimersByTime(10_000);
    });

    // Assert
    expect(toast).toHaveTextContent('disk full');
    expect(screen.getByRole('alert')).toBeInTheDocument();

    // Act
    fireEvent.pointerLeave(toast);
    act(() => {
      vi.advanceTimersByTime(8_000);
    });

    // Assert
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});
