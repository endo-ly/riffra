import { describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { HostConnectionChangedError } from '@/native/invoke';
import { createSessionCommit } from './commit';

describe('session commit', () => {
  it('reports a failed projection and returns the result even when canonical state was already accepted', async () => {
    const apply = vi.fn(() => false);
    const onError = vi.fn();
    const result: ArrangementMutationResult = {
      canonical: canonicalState(defaultSession()),
      createdEntityIds: {},
      projection: { state: 'failed', message: 'native rejected' },
    };
    expect(await createSessionCommit(apply)(Promise.resolve(result), { onError })).toBe(result);
    expect(apply).toHaveBeenCalledWith(result.canonical);
    expect(onError).toHaveBeenCalledWith('native rejected');
  });

  it('silences a changed Host and reports other command failures', async () => {
    const onError = vi.fn();
    const commit = createSessionCommit(vi.fn());
    expect(await commit(Promise.reject(new HostConnectionChangedError()), { onError })).toBeNull();
    expect(onError).not.toHaveBeenCalled();
    expect(await commit(Promise.reject(new Error('failed')), { onError })).toBeNull();
    expect(onError).toHaveBeenCalledWith('failed');
  });
});
