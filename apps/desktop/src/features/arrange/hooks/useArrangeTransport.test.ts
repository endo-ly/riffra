// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FakeNativeApi } from '@/native/native-api-fake';
import { useArrangeTransport } from './useArrangeTransport';

afterEach(() => {
  vi.restoreAllMocks();
});

describe('useArrangeTransport', () => {
  it('publishes a stopped transport discontinuity to the clock and playhead', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() =>
      useArrangeTransport(api, {
        bpm: 120,
        ppq: 960,
        timeSignatureNumerator: 4,
        timeSignatureDenominator: 4,
      }),
    );

    act(() => {
      api.emitTransportStatus({ timelineTick: 3_840, discontinuity: 2, sequence: 1 });
    });
    await waitFor(() => expect(result.current.displayTick).toBe(3_840));

    act(() => {
      api.emitTransportStatus({ timelineTick: 0, discontinuity: 3, sequence: 2 });
    });

    await waitFor(() => expect(result.current.displayTick).toBe(0));
    expect(result.current.displayTickRef.current).toBe(0);
  });

  it('ignores an older transport status', () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() =>
      useArrangeTransport(api, {
        bpm: 120,
        ppq: 960,
        timeSignatureNumerator: 4,
        timeSignatureDenominator: 4,
      }),
    );

    act(() => api.emitTransportStatus({ state: 'playing', timelineTick: 2_000, sequence: 2 }));
    act(() => api.emitTransportStatus({ state: 'stopped', timelineTick: 0, sequence: 1 }));

    expect(result.current.transport?.state).toBe('playing');
    expect(result.current.displayTickRef.current).toBe(2_000);
  });
});
