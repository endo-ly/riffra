// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FakeNativeApi } from '@/native/native-api-fake';
import App from '@/app/App';
import { defaultSession } from '@/native/browser-defaults';
import type { CreativeSession } from '@/model/domain';

afterEach(cleanup);

function renderApp(fake: FakeNativeApi) {
  return render(<App api={fake} />);
}

function mutationResult(session: CreativeSession) {
  return {
    canonical: { session, sequence: 0, history: { canUndo: false, canRedo: false } },
    session,
    projection: { state: 'notRequired' as const },
  };
}

async function waitForAppShell() {
  await waitFor(() => expect(screen.getByRole('main')).toBeInTheDocument());
}

describe('Undo/Redo (PRJ-003)', () => {
  it('undoes and redoes a session rename through the global bar', async () => {
    window.prompt = vi.fn(() => 'My Project');
    const original = defaultSession();
    const renamed = { ...original, projectName: 'My Project' };
    const fake = new FakeNativeApi({
      bootstrapState: { session: original },
      responses: {
        updateSessionSettings: mutationResult(renamed),
        undoSession: mutationResult(original),
        redoSession: mutationResult(renamed),
        getHistoryState: () => {
          const lastOperation = [...fake.calls]
            .reverse()
            .find((call) => ['updateSessionSettings', 'undoSession', 'redoSession'].includes(call));
          if (lastOperation === 'undoSession') return { canUndo: false, canRedo: true };
          if (lastOperation) return { canUndo: true, canRedo: false };
          return { canUndo: false, canRedo: false };
        },
      },
    });
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
