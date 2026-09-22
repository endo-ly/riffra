// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { LocalHostInfo } from '@/model/domain';
import { setHostConnectionAvailability, setHostGeneration } from '@/native/invoke';
import { FakeNativeApi } from '@/native/native-api-fake';
import { useHostConnection } from './useHostConnection';

const hostA: LocalHostInfo = {
  instanceId: 'host-a',
  pid: 18420,
  dataRoot: 'D:\\Music\\project-a',
  startedAtMs: 1,
  projectName: 'project-a',
  safeMode: false,
  status: 'Ready',
};

const hostB: LocalHostInfo = {
  ...hostA,
  instanceId: 'host-b',
  pid: 18421,
  dataRoot: 'D:\\Music\\project-b',
  projectName: 'project-b',
};

describe('useHostConnection', () => {
  beforeEach(() => {
    setHostGeneration(0);
    setHostConnectionAvailability(true);
  });

  afterEach(() => {
    setHostGeneration(0);
    setHostConnectionAvailability(true);
  });

  it('refreshes the Host list after a disconnected connection event', async () => {
    const api = new FakeNativeApi();
    const responses = [[hostA], [hostB]];
    api.setResponse('listLocalHosts', () => responses.shift() ?? [hostB]);
    const { result } = renderHook(() => useHostConnection(api));

    await waitFor(() => expect(result.current.hosts).toEqual([hostA]));

    act(() => {
      api.emitHostConnectionChanged(
        {
          mode: 'disconnected',
          generation: 2,
          reason: 'Host event connection closed',
        },
        null,
      );
    });

    await waitFor(() => expect(result.current.hosts).toEqual([hostB]));
    expect(api.calls.filter((call) => call === 'listLocalHosts')).toHaveLength(2);
  });
});
