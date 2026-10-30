// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import type { MidiClip } from '@/model/generated';
import { createSessionStore } from '@/shared/session/store';
import { createFakeNativeApi } from '@/test/support/fake-native-api';

const clip: MidiClip = {
  instrumentControlEvents: [],
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

describe('workspace', () => {
  it('switches between the Mixer and MIDI editor while keeping resize state coherent', () => {
    const api = createFakeNativeApi();
    const store = createSessionStore(api);
    const session = {
      ...api.bootstrapState.canonical.session,
      arrangement: { ...api.bootstrapState.canonical.session.arrangement, midiClips: [clip] },
    };
    store.getState().project.acceptBootstrap({
      ...api.bootstrapState,
      canonical: { ...api.bootstrapState.canonical, session },
    });
    store.getState().workspace.openMidiEditor(clip.id);
    expect(store.getState().workspace.lower.view).toBe('midiEditor');
    expect(store.getState().workspace.lower.activeMidiClipId).toBe(clip.id);
    expect(store.getState().workspace.selection).toEqual({ kind: 'clips', clipIds: [clip.id] });
    store.getState().workspace.setLowerCollapsed(true);
    expect(store.getState().workspace.lower.collapsed).toBe(true);
    expect(store.getState().workspace.lower.maximized).toBe(false);
    store.getState().workspace.setLowerMaximized(true);
    expect(store.getState().workspace.lower.maximized).toBe(true);
    expect(store.getState().workspace.lower.collapsed).toBe(false);
    store.getState().workspace.toggleMixer();
    expect(store.getState().workspace.lower.view).toBe('mixer');
    store.getState().workspace.toggleMixer();
    expect(store.getState().workspace.lower.view).toBe('midiEditor');
    expect(store.getState().workspace.lower.maximized).toBe(true);
    store.getState().workspace.closeLower();
    store.getState().workspace.toggleMixer();
    expect(store.getState().workspace.lower.view).toBe('mixer');
    store.getState().workspace.toggleMixer();
    expect(store.getState().workspace.lower.view).toBe('closed');
    store.getState().project.acceptCanonical({
      ...api.bootstrapState.canonical,
      sequence: api.bootstrapState.canonical.sequence + 1,
      session: { ...session, arrangement: { ...session.arrangement, midiClips: [] } },
    });
    expect(store.getState().workspace.lower.activeMidiClipId).toBeNull();
    expect(store.getState().workspace.lower.view).toBe('closed');
  });
});
