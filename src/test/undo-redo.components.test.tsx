// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FakeNativeApi } from '@/native/native-api-fake';
import App from '@/App';

afterEach(cleanup);

function renderApp(fake: FakeNativeApi) {
  return render(<App api={fake} />);
}

async function waitForAppShell() {
  await waitFor(() => expect(screen.getByRole('main')).toBeInTheDocument());
}

describe('Undo/Redo (PRJ-003)', () => {
  it('undoes and redoes a session setting change', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();

    const combo = screen.getByRole('combobox', {
      name: 'Visual count-in',
    }) as HTMLSelectElement;
    const initial = combo.value;
    expect(initial).not.toBe('4');

    const user = userEvent.setup();
    await user.selectOptions(combo, '4');

    const undoButton = screen.getByRole('button', { name: 'Undo' });
    await waitFor(() => expect(undoButton).not.toBeDisabled());
    expect(
      (screen.getByRole('combobox', { name: 'Visual count-in' }) as HTMLSelectElement).value,
    ).toBe('4');

    await user.click(undoButton);
    await waitFor(() =>
      expect(
        (screen.getByRole('combobox', { name: 'Visual count-in' }) as HTMLSelectElement).value,
      ).toBe(initial),
    );

    const redoButton = screen.getByRole('button', { name: 'Redo' });
    expect(redoButton).not.toBeDisabled();
    await user.click(redoButton);
    await waitFor(() =>
      expect(
        (screen.getByRole('combobox', { name: 'Visual count-in' }) as HTMLSelectElement).value,
      ).toBe('4'),
    );
  });

  it('undoes and redoes a session rename through the global bar', async () => {
    window.prompt = vi.fn(() => 'My Project');
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Untitled Scratch/ }));

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /My Project/ })).toBeInTheDocument(),
    );

    const undoButton = screen.getByRole('button', { name: 'Undo' });
    expect(undoButton).not.toBeDisabled();
    await user.click(undoButton);

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Untitled Scratch/ })).toBeInTheDocument(),
    );

    const redoButton = screen.getByRole('button', { name: 'Redo' });
    expect(redoButton).not.toBeDisabled();
    await user.click(redoButton);

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /My Project/ })).toBeInTheDocument(),
    );
  });
});
