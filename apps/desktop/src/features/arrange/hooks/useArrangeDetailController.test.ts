// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { MidiClip } from '@/model/domain';
import { useArrangeDetailController } from './useArrangeDetailController';

const clip: MidiClip = {
  id: 'clip:detail',
  name: 'Detail Clip',
  trackId: 'track:instrument',
  startTick: 0,
  durationTicks: 960,
  notes: [],
  events: [],
  muted: false,
  loopEnabled: false,
};

describe('useArrangeDetailController', () => {
  it('keeps detail transitions together and closes when the active clip disappears', () => {
    const selectClip = vi.fn();
    const { result, rerender } = renderHook(
      ({ midiClips }) => useArrangeDetailController({ midiClips, selectClip }),
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

    rerender({ midiClips: [] });
    expect(result.current.activeMidiClip).toBeNull();
    expect(result.current.view).toBe('closed');
  });
});
