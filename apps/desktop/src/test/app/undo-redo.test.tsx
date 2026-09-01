// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { FakeNativeApi } from '@/native/native-api-fake';
import App from '@/app/App';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import type { CreativeSession, ProjectState } from '@/model/domain';

afterEach(cleanup);

function renderApp(fake: FakeNativeApi) {
  return render(<App api={fake} />);
}

function mutationResult(session: CreativeSession) {
  return {
    canonical: { session, sequence: 0, history: { canUndo: false, canRedo: false } },
    projection: { state: 'notRequired' as const },
  };
}

async function waitForAppShell() {
  await waitFor(() => expect(screen.getByRole('main')).toBeInTheDocument());
}

describe('Undo/Redo (PRJ-003)', () => {
  it('undoes and redoes a session rename through the global bar', async () => {
    const original = defaultSession();
    const renamed = { ...original, projectName: 'My Project' };
    const activeProjectId = '01900000-0000-7000-8000-000000000001';
    const projectState = (name: string): ProjectState => ({
      activeProjectId,
      projects: [{ projectId: activeProjectId, name, updatedAtMs: 1, error: null }],
    });
    const fake = new FakeNativeApi({
      bootstrapState: {
        canonical: canonicalState(original),
        projectState: projectState('Untitled Project'),
      },
      responses: {
        renameProject: () => {
          fake.emitCanonicalStateChanged({
            ...canonicalState(renamed),
            history: { canUndo: true, canRedo: false },
          });
          fake.emitProjectStateChanged(projectState('My Project'));
          return projectState('My Project');
        },
        undoSession: () => {
          fake.emitCanonicalStateChanged({
            ...canonicalState(original),
            history: { canUndo: false, canRedo: true },
          });
          fake.emitProjectStateChanged(projectState('Untitled Project'));
          return mutationResult(original);
        },
        redoSession: () => {
          fake.emitCanonicalStateChanged({
            ...canonicalState(renamed),
            history: { canUndo: true, canRedo: false },
          });
          fake.emitProjectStateChanged(projectState('My Project'));
          return mutationResult(renamed);
        },
        getHistoryState: () => {
          const lastOperation = [...fake.calls]
            .reverse()
            .find((call) => ['renameProject', 'undoSession', 'redoSession'].includes(call));
          if (lastOperation === 'undoSession') return { canUndo: false, canRedo: true };
          if (lastOperation) return { canUndo: true, canRedo: false };
          return { canUndo: false, canRedo: false };
        },
      },
    });
    renderApp(fake);
    await waitForAppShell();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    const nameInput = await screen.findByRole('textbox', { name: 'Project name' });
    await user.clear(nameInput);
    await user.type(nameInput, 'My Project');
    await user.keyboard('{Enter}');

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Project: My Project/ })).toBeInTheDocument(),
    );

    const undoButton = screen.getByRole('button', { name: 'Undo' });
    expect(undoButton).not.toBeDisabled();
    await user.click(undoButton);

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Project: Untitled Project/ })).toBeInTheDocument(),
    );

    const redoButton = screen.getByRole('button', { name: 'Redo' });
    expect(redoButton).not.toBeDisabled();
    await user.click(redoButton);

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Project: My Project/ })).toBeInTheDocument(),
    );
  });
});
