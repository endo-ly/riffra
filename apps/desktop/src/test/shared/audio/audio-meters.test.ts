// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { AudioMeterFrame } from '@/model/generated';
import {
  markAudioMetersUnavailable,
  publishAudioMeters,
  resetAudioMeters,
  useAudioMeters,
  useAudioChannelMeter,
} from '@/shared/audio/audio-meters';

const frame: AudioMeterFrame = {
  projectId: 'project:meter-test',
  inputPeak: 0.1,
  outputPeak: 0.2,
  outputPeakLeft: 0.15,
  outputPeakRight: 0.2,
  preLimiterPeak: 0.25,
  limiterGainReductionDb: 1.5,
  hardClipSamples: 4,
  invalidSamples: 0,
  feedbackSuspected: false,
  busMeters: [],
  trackMeters: [
    { trackId: 'track:meter-test', peakLeft: 0.1, peakRight: 0.2, rmsLeft: 0.05, rmsRight: 0.1 },
  ],
};

describe('audio meter availability', () => {
  afterEach(() => {
    act(() => resetAudioMeters());
  });

  it('updates only the affected channel and clears removed or unavailable channels', async () => {
    const initial = {
      ...frame,
      trackMeters: [...frame.trackMeters, { ...frame.trackMeters[0], trackId: 'track:other' }],
      busMeters: [{ busId: 'bus:1', peakLeft: 0.3, peakRight: 0.4, rmsLeft: 0.1, rmsRight: 0.2 }],
    };
    act(() => publishAudioMeters(initial, frame.projectId));
    let trackRenders = 0;
    let busRenders = 0;
    let masterRenders = 0;
    const track = renderHook(() => {
      trackRenders += 1;
      return useAudioChannelMeter({ kind: 'track', id: 'track:meter-test' });
    });
    const bus = renderHook(() => {
      busRenders += 1;
      return useAudioChannelMeter({ kind: 'bus', id: 'bus:1' });
    });
    const master = renderHook(() => {
      masterRenders += 1;
      return useAudioChannelMeter({ kind: 'master' });
    });
    const before = [trackRenders, busRenders, masterRenders];
    const updated = {
      ...initial,
      trackMeters: [...initial.trackMeters].reverse().map((meter) => ({
        ...meter,
        peakLeft: meter.trackId === 'track:meter-test' ? 0.6 : meter.peakLeft,
      })),
      busMeters: initial.busMeters.map((meter) => ({ ...meter })),
    };

    act(() => publishAudioMeters(updated, frame.projectId));
    await waitFor(() => expect(track.result.current?.peakLeft).toBe(0.6));

    expect(trackRenders).toBeGreaterThan(before[0]);
    expect([busRenders, masterRenders]).toEqual(before.slice(1));
    expect(bus.result.current?.peakLeft).toBe(0.3);
    expect(master.result.current?.peakLeft).toBe(frame.outputPeakLeft);

    act(() => publishAudioMeters({ ...updated, trackMeters: [] }, frame.projectId));
    await waitFor(() => expect(track.result.current).toBeNull());
    act(() => markAudioMetersUnavailable());
    await waitFor(() => expect(bus.result.current).toBeNull());
    expect(master.result.current).toBeNull();
  });

  it('marks a real frame available and clears it when the runtime stops', async () => {
    const { result } = renderHook(() => useAudioMeters());

    act(() => publishAudioMeters({ ...frame, projectId: 'project:other' }, frame.projectId));
    expect(result.current.available).toBe(false);

    act(() => publishAudioMeters(frame, frame.projectId));
    await waitFor(() => expect(result.current.available).toBe(true));
    expect(result.current.hardClipSamples).toBe(4);
    expect(result.current.trackMeters).toHaveLength(1);

    act(() => markAudioMetersUnavailable());
    await waitFor(() => expect(result.current.available).toBe(false));
    expect(result.current.hardClipSamples).toBe(0);
    expect(result.current.trackMeters).toHaveLength(0);
  });
});
