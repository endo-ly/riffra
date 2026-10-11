// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ArrangementMutationResult } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { useArrangeCommands } from './useArrangeCommands';

describe('useArrangeCommands', () => {
  it('applies the canonical session and reports an immediate projection failure', async () => {
    const sessions: string[] = [];
    const { result: hook } = renderHook(() =>
      useArrangeCommands({
        applyCanonicalState: (canonical) => {
          sessions.push(canonical.session.sessionId);
          return true;
        },
      }),
    );

    const mutation: ArrangementMutationResult = {
      canonical: canonicalState(defaultSession()),
      createdEntityIds: {},
      projection: { state: 'failed', message: 'native rejected' },
    };
    await act(async () => {
      await hook.current.commit(Promise.resolve(mutation));
    });

    expect(sessions).toEqual([defaultSession().sessionId]);
    expect(hook.current.message).toBe('native rejected');
  });
});
