// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import {
  markAudioMetersUnavailable,
  publishAudioMeters,
  resetAudioMeters,
  useAudioMeters,
  type AudioMeterFrame,
} from './audio-meters';

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
  trackMeters: [
    { trackId: 'track:meter-test', peakLeft: 0.1, peakRight: 0.2, rmsLeft: 0.05, rmsRight: 0.1 },
  ],
};

describe('audio meter availability', () => {
  afterEach(() => {
    act(() => resetAudioMeters());
  });

  it('marks a real frame available and clears it when the runtime stops', async () => {
    const { result } = renderHook(() => useAudioMeters());

    act(() => publishAudioMeters(frame));
    await waitFor(() => expect(result.current.available).toBe(true));
    expect(result.current.hardClipSamples).toBe(4);
    expect(result.current.trackMeters).toHaveLength(1);

    act(() => markAudioMetersUnavailable());
    await waitFor(() => expect(result.current.available).toBe(false));
    expect(result.current.hardClipSamples).toBe(0);
    expect(result.current.trackMeters).toHaveLength(0);
  });
});
