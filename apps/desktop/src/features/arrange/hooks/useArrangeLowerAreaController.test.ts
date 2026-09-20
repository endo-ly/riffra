// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { MidiClip } from '@/model/domain';
import { useArrangeLowerAreaController } from './useArrangeLowerAreaController';

const clip: MidiClip = {
  id: 'clip:lower-area',
  name: 'Lower Area Clip',
  trackId: 'track:instrument',
  startTick: 0,
  durationTicks: 960,
  notes: [],
  events: [],
  muted: false,
  loopEnabled: false,
};

describe('useArrangeLowerAreaController', () => {
  it('switches between the Mixer and MIDI editor while keeping resize state coherent', () => {
    const selectClip = vi.fn();
    const { result, rerender } = renderHook(
      ({ midiClips }) => useArrangeLowerAreaController({ midiClips, selectClip }),
      { initialProps: { midiClips: [clip] } },
    );

    act(() => result.current.openMidiEditor(clip));
    expect(result.current.view).toBe('midiEditor');
    expect(result.current.activeMidiClip?.id).toBe(clip.id);
    expect(selectClip).toHaveBeenCalledWith(clip.id);

    act(() => result.current.setCollapsed(true));
    expect(result.current.collapsed).toBe(true);
    expect(result.current.maximized).toBe(false);

    act(() => result.current.setMaximized(true));
    expect(result.current.maximized).toBe(true);
    expect(result.current.collapsed).toBe(false);

    act(() => result.current.toggleMixer());
    expect(result.current.view).toBe('mixer');

    act(() => result.current.toggleMixer());
    expect(result.current.view).toBe('midiEditor');
    expect(result.current.maximized).toBe(true);

    act(() => result.current.close());
    act(() => result.current.toggleMixer());
    expect(result.current.view).toBe('mixer');

    act(() => result.current.toggleMixer());
    expect(result.current.view).toBe('closed');

    rerender({ midiClips: [] });
    expect(result.current.activeMidiClip).toBeNull();
    expect(result.current.view).toBe('closed');
  });
});
