// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FakeNativeApi } from '@/native/native-api-fake';
import { useArrangementTransport } from './useArrangementTransport';

afterEach(() => {
  vi.restoreAllMocks();
});

describe('useArrangementTransport', () => {
  it('publishes a stopped transport discontinuity to the clock and playhead', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() =>
      useArrangementTransport(api, {
        tempoChanges: [{ tick: 0, bpm: 120 }],
        ppq: 960,
        timeSignatureChanges: [{ tick: 0, numerator: 4, denominator: 4 }],
      }),
    );

    act(() => {
      api.emitTransportStatus({ timelineTick: 3_840, discontinuity: 2 });
    });
    await waitFor(() => expect(result.current.displayTick).toBe(3_840));

    act(() => {
      api.emitTransportStatus({ timelineTick: 0, discontinuity: 3 });
    });

    await waitFor(() => expect(result.current.displayTick).toBe(0));
    expect(result.current.displayTickRef.current).toBe(0);
  });

  it('restarts the playhead when the Active Project changes', async () => {
    const api = new FakeNativeApi();
    const timebase = {
      tempoChanges: [{ tick: 0, bpm: 120 }],
      ppq: 960,
      timeSignatureChanges: [{ tick: 0, numerator: 4, denominator: 4 }],
    };
    const { result, rerender } = renderHook(
      ({ projectId }) => useArrangementTransport(api, timebase, 0, projectId),
      { initialProps: { projectId: 'project-a' } },
    );
    act(() => {
      api.emitTransportStatus({ timelineTick: 3_840, discontinuity: 2 });
    });
    await waitFor(() => expect(result.current.displayTick).toBe(3_840));

    rerender({ projectId: 'project-b' });

    await waitFor(() => expect(result.current.displayTick).toBe(0));
    expect(result.current.transport).toBeNull();
  });
});
