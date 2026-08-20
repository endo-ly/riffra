// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ArrangementMutationResult } from '@/model/domain';
import { defaultSession } from '@/native/browser-defaults';
import { useArrangeCommands } from './useArrangeCommands';

function result(projection: ArrangementMutationResult['projection']): ArrangementMutationResult {
  return { session: defaultSession(), projection };
}

describe('useArrangeCommands', () => {
  it('applies the canonical session and reports an immediate projection failure', async () => {
    const sessions: string[] = [];
    const { result: hook } = renderHook(() =>
      useArrangeCommands({
        setSession: (session) => sessions.push(session.sessionId),
      }),
    );

    await act(async () => {
      await hook.current.commit(
        Promise.resolve(
          result({
            state: 'failed',
            message: 'native rejected',
          }),
        ),
      );
    });

    expect(sessions).toEqual([defaultSession().sessionId]);
    expect(hook.current.message).toBe('native rejected');
  });

  it('does not report a queued or non-projecting mutation as an error', async () => {
    const { result: hook } = renderHook(() => useArrangeCommands({ setSession: () => undefined }));

    await act(async () => {
      await hook.current.commit(Promise.resolve(result({ state: 'queued' })));
    });
    expect(hook.current.message).toBe('');

    await act(async () => {
      await hook.current.commit(Promise.resolve(result({ state: 'notRequired' })));
    });
    expect(hook.current.message).toBe('');
  });
});
