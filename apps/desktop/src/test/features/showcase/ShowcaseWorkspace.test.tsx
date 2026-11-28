// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { VisualSnapshot } from '@riffra/visual-renderer';
import App from '@/app/App';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe = vi.fn();
      disconnect = vi.fn();
    },
  );
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function renderApp(api: FakeNativeApi) {
  render(<App api={api} />);
}

async function waitForShell() {
  await waitFor(() => expect(screen.getByRole('main')).toBeInTheDocument());
}

function viewModeButton(label: 'Edit' | 'Showcase') {
  return screen.getByRole('button', { name: label });
}

function snapshotCalls(api: FakeNativeApi) {
  return api.calls.filter((call) => call === 'getVisualSnapshot').length;
}

function visualSnapshot(): VisualSnapshot {
  return {
    schemaVersion: 1,
    title: 'Showcase Test',
    durationSec: 8,
    tempos: [{ t: 0, bpm: 120 }],
    meters: [{ t: 0, bar: 1, numerator: 4, denominator: 4 }],
    beats: [
      { t: 0, bar: 1, beat: 1, downbeat: true },
      { t: 0.5, bar: 1, beat: 2, downbeat: false },
    ],
    sections: [],
    chords: [],
    tracks: [],
    drumLanes: [],
  };
}

describe('Showcase integration', () => {
  it('switches between Edit and Showcase from the global bar and with V', async () => {
    // Arrange
    const api = createFakeNativeApi();
    const user = userEvent.setup();
    renderApp(api);
    await waitForShell();
    expect(viewModeButton('Edit')).toHaveAttribute('aria-pressed', 'true');
    expect(document.querySelector('[data-arrange-workspace]')).toBeInTheDocument();

    // Act
    await user.click(viewModeButton('Showcase'));

    // Assert
    await waitFor(() => expect(viewModeButton('Showcase')).toHaveAttribute('aria-pressed', 'true'));
    expect(document.querySelector('[data-showcase-workspace]')).toBeInTheDocument();
    expect(document.querySelector('[data-arrange-workspace]')).not.toBeInTheDocument();

    // Act
    await user.keyboard('v');

    // Assert
    await waitFor(() => expect(viewModeButton('Edit')).toHaveAttribute('aria-pressed', 'true'));
    expect(document.querySelector('[data-arrange-workspace]')).toBeInTheDocument();
  });

  it('leaves the view mode to Ctrl+V, typing targets, and handled key events', async () => {
    // Arrange
    const api = createFakeNativeApi();
    const user = userEvent.setup();
    renderApp(api);
    await waitForShell();

    // Act: modified keys belong to the browser or the operating system.
    fireEvent.keyDown(document.body, { key: 'v', ctrlKey: true });

    // Assert
    expect(viewModeButton('Edit')).toHaveAttribute('aria-pressed', 'true');

    // Act: typing into the command palette must not switch views.
    await user.keyboard('{Control>}k{/Control}');
    await user.keyboard('v');

    // Assert
    expect(viewModeButton('Edit')).toHaveAttribute('aria-pressed', 'true');

    // Act: a consumed key event stays consumed.
    await user.keyboard('{Escape}');
    const consume = (event: KeyboardEvent) => event.preventDefault();
    window.addEventListener('keydown', consume, true);
    fireEvent.keyDown(document.body, { key: 'v' });
    window.removeEventListener('keydown', consume, true);

    // Assert
    expect(viewModeButton('Edit')).toHaveAttribute('aria-pressed', 'true');
  });

  it('toggles playback with Space in Showcase mode', async () => {
    // Arrange
    const api = createFakeNativeApi();
    const user = userEvent.setup();
    renderApp(api);
    await waitForShell();
    fireEvent.keyDown(document.body, { key: 'v' });
    await waitFor(() =>
      expect(document.querySelector('[data-showcase-workspace]')).toBeInTheDocument(),
    );
    const calls = () => api.calls.filter((call) => call === 'playTimeline').length;
    expect(calls()).toBe(0);

    // Act
    await user.keyboard(' ');

    // Assert
    await waitFor(() => expect(calls()).toBe(1));
  });

  it('reads the snapshot only while Showcase is shown and renders the theme', async () => {
    // Arrange
    const snapshot = visualSnapshot();
    const api = createFakeNativeApi({ responses: { getVisualSnapshot: () => snapshot } });
    renderApp(api);
    await waitForShell();
    expect(snapshotCalls(api)).toBe(0);

    // Act
    fireEvent.keyDown(document.body, { key: 'v' });

    // Assert
    await waitFor(() => expect(snapshotCalls(api)).toBe(1));
    await waitFor(() =>
      expect(document.querySelector('[data-theme="studio-console"]')).toBeInTheDocument(),
    );

    // Act
    fireEvent.keyDown(document.body, { key: 'v' });

    // Assert
    await waitFor(() =>
      expect(document.querySelector('[data-showcase-workspace]')).not.toBeInTheDocument(),
    );
    expect(snapshotCalls(api)).toBe(1);
    expect(document.querySelector('[data-theme="studio-console"]')).not.toBeInTheDocument();
  });

  it('shows the empty state when the Project has nothing to visualize', async () => {
    // Arrange
    const api = createFakeNativeApi();
    renderApp(api);
    await waitForShell();

    // Act
    fireEvent.keyDown(document.body, { key: 'v' });

    // Assert
    await waitFor(() => expect(snapshotCalls(api)).toBe(1));
    expect(await screen.findByText('Nothing to visualize yet')).toBeInTheDocument();
    expect(document.querySelector('[data-theme="studio-console"]')).not.toBeInTheDocument();
  });
});
