// @vitest-environment jsdom

import { waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { defaultSession } from '@/native/browser-defaults';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { bindSessionRuntime } from '@/shared/session/runtime';
import { createSessionStore } from '@/shared/session/store';

let unbind: (() => void) | undefined;
afterEach(() => {
  unbind?.();
  unbind = undefined;
});

async function setup() {
  const api = createFakeNativeApi();
  const store = createSessionStore(api);
  unbind = bindSessionRuntime(store, api);
  await waitFor(() => expect(store.getState().project.boot).not.toBeNull());
  return { api, store };
}

describe('transport', () => {
  it('reports Starting until the native Playing status arrives', async () => {
    const { api, store } = await setup();
    await store.getState().transport.play();
    expect(api.calls).toContain('playTimeline');
    api.emitTransportStatus({ state: 'starting' });
    expect(store.getState().transport.status?.state).toBe('starting');
    api.emitTransportStatus({ state: 'playing' });
    expect(store.getState().transport.status?.state).toBe('playing');
    await store.getState().transport.pause();
    expect(api.calls).toContain('pauseTimeline');
    api.emitTransportStatus({ state: 'paused' });
    expect(store.getState().transport.status?.state).toBe('paused');
    await store.getState().transport.play();
    expect(api.calls.filter((call) => call === 'playTimeline')).toHaveLength(2);
  });

  it('stops a timeline play request before the playing status arrives', async () => {
    const { api, store } = await setup();
    await store.getState().transport.play();
    await store.getState().transport.stop();
    expect(api.calls.filter((call) => call === 'playTimeline')).toHaveLength(1);
    expect(api.calls.filter((call) => call === 'stopTimeline')).toHaveLength(1);
  });

  it('moves a timeline play request to the start before the playing status arrives', async () => {
    const { api, store } = await setup();
    await store.getState().transport.play();
    await store.getState().transport.goToStart();
    expect(api.calls.filter((call) => call === 'playTimeline')).toHaveLength(1);
    expect(api.calls.filter((call) => call === 'goToStartTimeline')).toHaveLength(1);
  });

  it('starts a newer Play intent while Stop is still pending', async () => {
    const { api, store } = await setup();
    let resolveStop!: () => void;
    api.setResponse(
      'stopTimeline',
      () =>
        new Promise<void>((resolve) => {
          resolveStop = resolve;
        }),
    );
    await store.getState().transport.play();
    api.emitTransportStatus({ state: 'playing' });
    const stopping = store.getState().transport.stop();
    await waitFor(() => expect(api.calls).toContain('stopTimeline'));
    await store.getState().transport.play();
    expect(api.calls.filter((call) => call === 'playTimeline')).toHaveLength(2);
    resolveStop();
    await stopping;
  });

  it('publishes a stopped transport discontinuity to the clock and playhead', async () => {
    const { api, store } = await setup();
    api.emitTransportStatus({ timelineTick: 3_840, discontinuity: 2 });
    expect(store.getState().transport.displayTick).toBe(3_840);
    api.emitTransportStatus({ timelineTick: 0, discontinuity: 3 });
    expect(store.getState().transport.displayTick).toBe(0);
    expect(store.getState().transport.playhead.current).toBe(0);
  });

  it('keeps the transport state when the Host repeats its status', async () => {
    // Arrange
    const { api, store } = await setup();
    api.emitTransportStatus({ timelineTick: 960 });
    const settled = store.getState().transport;
    let updates = 0;
    const stop = store.subscribe(() => (updates += 1));

    // Act
    api.emitTransportStatus({ timelineTick: 960 });

    // Assert
    stop();
    expect(updates).toBe(0);
    expect(store.getState().transport).toBe(settled);
  });

  it('publishes a display tick only while the playhead moves', () => {
    // Arrange
    const api = createFakeNativeApi();
    const store = createSessionStore(api);
    const timebase = defaultSession().arrangement.timebase;
    let updates = 0;
    const stop = store.subscribe(() => (updates += 1));

    // Act: a stopped playhead stays on the same tick across frames.
    store.getState().transport.advance(1_000, timebase);
    store.getState().transport.advance(2_000, timebase);

    // Assert
    expect(updates).toBe(0);
    expect(store.getState().transport.displayTick).toBe(0);
    stop();
  });

  it('restarts the playhead when the Active Project changes', async () => {
    const { api, store } = await setup();
    const boot = store.getState().project.boot!;
    api.emitTransportStatus({ timelineTick: 3_840, discontinuity: 2 });
    expect(store.getState().transport.displayTick).toBe(3_840);
    store.getState().project.acceptActivation({
      projectState: { ...boot.projectState, activeProjectId: 'another-project' },
      canonical: {
        ...boot.canonical,
        projectId: 'another-project',
        sequence: boot.canonical.sequence + 1,
      },
      recovery: boot.recovery,
    });
    expect(store.getState().transport.displayTick).toBe(0);
    expect(store.getState().transport.status).toBeNull();
  });
});
