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
});
