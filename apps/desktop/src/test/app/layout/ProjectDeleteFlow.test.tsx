// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ProjectHostSelector } from '@/app/layout/ProjectHostSelector';
import { ToastStack } from '@/shared/ui/message/ToastStack';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { renderWithSession } from '@/test/support/session-provider';

vi.mock('@/native/dialog', () => ({ openHostDataRoot: vi.fn().mockResolvedValue(null) }));

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe = vi.fn();
      disconnect = vi.fn();
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('project deletion flow', () => {
  it('shows a toast after the real store deletes a project', async () => {
    const api = createFakeNativeApi();
    const state = api.bootstrapState.projectState;
    api.bootstrapState = {
      ...api.bootstrapState,
      projectState: {
        ...state,
        projects: [
          ...state.projects,
          { projectId: 'other', name: 'Other', updatedAtMs: 1, error: null },
        ],
      },
    };
    const user = userEvent.setup();
    renderWithSession(
      <>
        <ProjectHostSelector />
        <ToastStack />
      </>,
      { api },
    );

    await user.click(screen.getByRole('button', { name: /Project:/ }));
    fireEvent.contextMenu(
      within(screen.getByRole('group', { name: 'Projects' })).getByRole('button', {
        name: /Other/,
      }),
    );
    await user.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    await user.click(
      within(screen.getByRole('dialog', { name: 'Delete Project permanently?' })).getByRole(
        'button',
        { name: 'Delete' },
      ),
    );

    const toast = (await screen.findByText('Project deleted')).closest('[role="status"]');
    expect(toast).toHaveTextContent('Other');
  });
});
