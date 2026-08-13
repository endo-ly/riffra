// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { ToastStack } from '@/shared/ui/ToastStack';
import { clearToast, showToast } from '@/shared/toasts';

afterEach(() => {
  cleanup();
});

describe('toast store', () => {
  it('keeps source-owned notifications independent when their text matches', () => {
    // Arrange
    render(<ToastStack />);

    // Act
    act(() => {
      showToast('session', 'The edit failed.', { kind: 'error', persistent: true });
      showToast('arrange', 'The edit failed.', { persistent: true });
    });

    // Assert
    expect(screen.getAllByText('The edit failed.')).toHaveLength(2);
    act(() => clearToast('session'));
    expect(screen.getAllByText('The edit failed.')).toHaveLength(1);
  });

  it('updates and clears a notification through its source', () => {
    // Arrange
    render(<ToastStack />);

    // Act
    act(() => {
      showToast('audio', 'Audio device is unavailable.', { kind: 'error', persistent: true });
      showToast('audio', 'Audio device recovered.', { persistent: true });
    });

    // Assert
    expect(screen.queryByText('Audio device is unavailable.')).not.toBeInTheDocument();
    expect(screen.getByText('Audio device recovered.')).toBeInTheDocument();
    act(() => clearToast('audio'));
    expect(screen.queryByText('Audio device recovered.')).not.toBeInTheDocument();
  });
});
