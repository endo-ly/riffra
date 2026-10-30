// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  ArrangementMutationResult,
  AudioClip,
  AudioClipMove,
  MidiClip,
  MidiEvent,
  RuntimeProjectionStatus,
  Track,
} from '@/model/generated';
import type { TransportStatus } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/model/asset-id';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { TRACK_HEADER_WIDTH } from '@/features/timeline/model/timeline-layout';
import { offGridAudioMoveSession, renderWorkspace } from '@/test/support/workspace';

afterEach(cleanup);

function mergeTrack(id: string, kind: Track['kind']): Track {
  return {
    externalAudioSourceTrackIds: [],
    panLaw: 'equalPower',
    id,
    name: id,
    kind,
    gainDb: 0,
    pan: 0,
    muted: false,
    solo: false,
    armed: false,
    monitoring: 'off',
    midiInput: {},
    effects: [],
  };
}

function mergeAudioClip(id: string, startTick: number, sourceStart: number): AudioClip {
  return {
    id,
    name: id,
    trackId: 'track:audio',
    assetId: toAssetId('asset:audio'),
    startTick,
    sourceRange: { start: sourceStart, end: sourceStart + 48_000 },
    sourceSampleRate: 48_000,
    timelineDuration: { frames: 48_000, sampleRate: 48_000 },
    gainDb: 0,
    pan: 0,
    fadeIn: { frames: 0, sampleRate: 48_000 },
    fadeOut: { frames: 0, sampleRate: 48_000 },
    fadeShape: 'equalPower',
    loopEnabled: false,
    muted: false,
    takeVariant: 'raw',
  };
}

function mergeMidiClip(
  id: string,
  startTick: number,
  durationTicks: number,
  noteId: string,
  noteStartTick: number,
  eventId: string,
  eventTick: number,
): MidiClip {
  const event: MidiEvent = {
    id: eventId,
    kind: 'controlChange',
    tick: eventTick,
    channel: 0,
    data1: 1,
    data2: 64,
  };
  return {
    instrumentControlEvents: [],
    id,
    name: id,
    trackId: 'track:instrument',
    startTick,
    durationTicks,
    notes: [
      {
        id: noteId,
        note: 60,
        startTick: noteStartTick,
        durationTicks: 120,
        velocity: 96,
        channel: 0,
      },
    ],
    events: [event],
    muted: false,
    loopEnabled: false,
  };
}

describe('TimelineView', () => {
  it.each(['next', 'previous'] as const)(
    'merges adjacent Audio clips with the %s clip into the previous clip',
    async (direction) => {
      const session = defaultSession();
      session.arrangement.tracks.push(mergeTrack('track:audio', 'audio'));
      session.arrangement.audioClips.push(
        mergeAudioClip('clip:audio:first', 0, 0),
        mergeAudioClip('clip:audio:second', 1_920, 48_000),
      );
      const api = createFakeNativeApi();
      const update = vi
        .spyOn(api, 'updateAudioClip')
        .mockResolvedValue(api.mutationResult(session));
      const remove = vi
        .spyOn(api, 'removeTimelineClips')
        .mockResolvedValue(api.mutationResult(session));
      const { container } = renderWorkspace(api, { session });
      const selectedClipId = direction === 'next' ? 'clip:audio:first' : 'clip:audio:second';

      fireEvent.contextMenu(container.querySelector(`[data-clip-id="${selectedClipId}"]`)!);
      fireEvent.click(
        screen.getByRole('menuitem', {
          name: `Merge with ${direction === 'next' ? 'Next' : 'Previous'}`,
        }),
      );

      await waitFor(() => expect(remove).toHaveBeenCalledWith(['clip:audio:second'], []));
      expect(update).toHaveBeenCalledWith('clip:audio:first', {
        sourceRange: { start: 0, end: 96_000 },
        timelineDuration: { frames: 96_000, sampleRate: 48_000 },
      });
    },
  );

  it.each(['next', 'previous'] as const)(
    'merges adjacent MIDI notes and events with the %s clip into the previous clip',
    async (direction) => {
      const session = defaultSession();
      session.arrangement.tracks.push(mergeTrack('track:instrument', 'instrument'));
      session.arrangement.midiClips.push(
        mergeMidiClip('clip:midi:first', 0, 960, 'note:first', 120, 'event:first', 100),
        mergeMidiClip('clip:midi:second', 960, 1_920, 'note:second', 60, 'event:second', 200),
      );
      const api = createFakeNativeApi();
      const update = vi.spyOn(api, 'updateMidiClip').mockResolvedValue(api.mutationResult(session));
      const remove = vi
        .spyOn(api, 'removeTimelineClips')
        .mockResolvedValue(api.mutationResult(session));
      const { container } = renderWorkspace(api, { session });
      const selectedClipId = direction === 'next' ? 'clip:midi:first' : 'clip:midi:second';

      fireEvent.contextMenu(container.querySelector(`[data-clip-id="${selectedClipId}"]`)!);
      fireEvent.click(
        screen.getByRole('menuitem', {
          name: `Merge with ${direction === 'next' ? 'Next' : 'Previous'}`,
        }),
      );

      await waitFor(() => expect(remove).toHaveBeenCalledWith([], ['clip:midi:second']));
      expect(update).toHaveBeenCalledWith('clip:midi:first', {
        durationTicks: 2_880,
        notes: [
          expect.objectContaining({ id: 'note:first', startTick: 120 }),
          expect.objectContaining({ id: 'note:second', startTick: 1_020 }),
        ],
        events: [
          expect.objectContaining({ id: 'event:first', tick: 100 }),
          expect.objectContaining({ id: 'event:second', tick: 1_160 }),
        ],
      });
    },
  );

  it('returns the playhead and timeline viewport to start on a transport discontinuity', async () => {
    const api = createFakeNativeApi();
    const { container } = renderWorkspace(api);
    const scroller = container.querySelector('[class*="scroller"]') as HTMLDivElement;
    Object.defineProperty(scroller, 'clientWidth', { configurable: true, value: 800 });

    api.emitTransportStatus({ timelineTick: 3_840, discontinuity: 2 });
    await waitFor(() => expect(scroller.scrollLeft).toBe(0));
    scroller.scrollLeft = 700;

    api.emitTransportStatus({ timelineTick: 0, discontinuity: 3 });

    await waitFor(() => expect(scroller.scrollLeft).toBe(0));
    await waitFor(() =>
      expect(container.querySelector('[class*="playhead"]')?.getAttribute('style')).toContain(
        `translate3d(${TRACK_HEADER_WIDTH}px`,
      ),
    );
  });

  it('marks inaudible tracks from canonical state while solo and mute edits are pending', async () => {
    // Arrange
    const session = defaultSession();
    for (const [id, muted, solo] of [
      ['track:muted', true, false],
      ['track:soloed', false, true],
      ['track:other', false, false],
    ] as const) {
      session.arrangement.tracks.push({
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id,
        name: id,
        kind: 'instrument',
        gainDb: 0,
        pan: 0,
        muted,
        solo,
        armed: false,
        monitoring: 'off',
        midiInput: {},
        effects: [],
      });
    }
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const updateTrack = vi.spyOn(api, 'updateTrack');

    // Act
    const { container } = renderWorkspace(api, { session });

    // Assert
    const inaudible = [...container.querySelectorAll('[data-arrange-track][data-inaudible]')].map(
      (row) => row.getAttribute('data-track-id'),
    );
    expect(inaudible).toEqual(['track:muted', 'track:other']);

    for (const solo of [false, true]) {
      let resolveUpdate!: (result: ArrangementMutationResult) => void;
      updateTrack.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveUpdate = resolve;
          }),
      );
      fireEvent.click(screen.getByRole('button', { name: 'Solo track:soloed' }));

      expect(screen.getByRole('button', { name: 'Solo track:soloed' })).toHaveAttribute(
        'aria-pressed',
        String(solo),
      );
      expect(container.querySelector('[data-track-id="track:soloed"]')).not.toHaveAttribute(
        'data-inaudible',
      );
      expect(
        container.querySelector('[data-track-id="track:other"]')?.hasAttribute('data-inaudible'),
      ).toBe(!solo);

      const next = structuredClone(session);
      next.arrangement.tracks.find((track) => track.id === 'track:soloed')!.solo = solo;
      await act(async () => resolveUpdate(api.mutationResult(next)));
      expect(
        container.querySelector('[data-track-id="track:other"]')?.hasAttribute('data-inaudible'),
      ).toBe(solo);
    }

    updateTrack.mockImplementationOnce(() => new Promise(() => undefined));
    fireEvent.click(screen.getByRole('button', { name: 'Mute track:soloed' }));
    expect(screen.getByRole('button', { name: 'Mute track:soloed' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(container.querySelector('[data-track-id="track:soloed"]')).not.toHaveAttribute(
      'data-inaudible',
    );
  });

  it('seeks the native timeline from the musical ruler', () => {
    const api = createFakeNativeApi();
    renderWorkspace(api);
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 1472, top: 0, bottom: 30, right: 1472, height: 30 }),
    });

    fireEvent.pointerDown(ruler, { clientX: 92 });

    expect(api.calls).toContain('seekTimeline');
  });

  it('keeps the timeline view following the playhead during playback', async () => {
    // At 120 bpm / ppq 960 the playhead advances 1,920 ticks (192 px at 0.1
    // px/tick) per second; with an 800 px viewport the follow line sits at 256 px.
    vi.useFakeTimers({ toFake: ['requestAnimationFrame', 'performance'] });
    try {
      const api = createFakeNativeApi();
      const { container } = renderWorkspace(api);
      const scroller = container.querySelector('[class*="scroller"]') as HTMLDivElement;
      Object.defineProperty(scroller, 'clientWidth', { configurable: true, value: 800 });

      await act(async () => {
        api.emitTransportStatus({ state: 'playing', timelineTick: 2_000, discontinuity: 1 });
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(500);
      });
      // The seek jump alone leaves the view at 139 px; playback has moved it further.
      expect(scroller.scrollLeft).toBeGreaterThan(150);

      // A manual scroll pauses the follow while the playhead stays in view.
      // jsdom does not fire scroll events for programmatic scrollLeft
      // assignments, so the event the browser would have raised for the follow
      // scroll is dispatched first, then the manual one.
      scroller.dispatchEvent(new Event('scroll'));
      scroller.scrollLeft = 0;
      scroller.dispatchEvent(new Event('scroll'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(scroller.scrollLeft).toBe(0);

      // Once the playhead leaves the viewport the follow resumes automatically.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(4_000);
      });
      expect(scroller.scrollLeft).toBeGreaterThan(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it('keeps an unavailable clip on the timeline and labels its missing source', async () => {
    const session = defaultSession();
    const assetId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a');
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:audio',
      name: 'Audio',
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [],
    });
    session.arrangement.audioClips.push({
      id: 'clip:missing',
      name: 'Lost Take',
      trackId: 'track:audio',
      assetId,
      startTick: 0,
      sourceRange: { start: 0, end: 48_000 },
      sourceSampleRate: 48_000,
      timelineDuration: { frames: 48_000, sampleRate: 48_000 },
      gainDb: 0,
      pan: 0,
      fadeIn: { frames: 0, sampleRate: 48_000 },
      fadeOut: { frames: 0, sampleRate: 48_000 },
      loopEnabled: false,
      muted: false,
      fadeShape: 'equalPower',
      takeVariant: 'raw',
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const status: TransportStatus = {
      state: 'stopped',
      revision: session.arrangement.revision,
      timelineTick: 0,
      timelineSample: 0,
      audioClockSample: 0,
      sampleRate: 48_000,
      appliedCommandSequence: 1,
      recordingPhase: 'idle',
      recordingStartTick: 0,
      recordingPassOrdinal: 0,
      armedTrackIds: [],
      instrumentFaults: [],
      clockGeneration: 1,
      discontinuity: 1,
    };
    const runtimeProjectionStatus = {
      ...api.runtimeProjection,
      activeDiagnostics: {
        unavailableClipIds: ['clip:missing'],
        missingDeviceIds: [],
      },
    } satisfies RuntimeProjectionStatus;
    api.onTransportStatus = (callback) => {
      queueMicrotask(() => callback(status));
      return () => undefined;
    };

    api.emitRuntimeProjectionStatus(runtimeProjectionStatus);
    renderWorkspace(api, { session });

    expect(await screen.findByText(/Playback skipped 1 missing source/)).toBeInTheDocument();
    expect(await screen.findByText('Lost Take')).toBeInTheDocument();
    expect((await screen.findAllByText('MISSING SOURCE')).length).toBeGreaterThan(0);
    expect(document.querySelector('[data-clip-id="clip:missing"]')).toBeInTheDocument();
  });

  it('reports a projection failure in place and recovers through the retry action', async () => {
    // Arrange: the projection fails and its retry notice is shown.
    const api = createFakeNativeApi();
    const failure = 'Audio preparation failed. Retry to prepare audio.';
    api.emitRuntimeProjectionStatus({
      ...api.runtimeProjection,
      state: 'failed',
      operationId: 1,
      activeProjectionSequence: null,
    });
    let resolveRetry!: (status: RuntimeProjectionStatus) => void;
    api.setResponse(
      'retryRuntimeProjection',
      new Promise<RuntimeProjectionStatus>((resolve) => {
        resolveRetry = resolve;
      }),
    );
    renderWorkspace(api);

    // Assert
    expect(await screen.findByText(failure)).toBeInTheDocument();

    // Act: Retry reports progress instead of a raw error while it is running.
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(api.calls).toContain('retryRuntimeProjection');
    expect(await screen.findByText('Retrying audio preparation…')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument();

    // Assert: the failure no longer covers the Arrangement once preparation succeeds.
    act(() =>
      resolveRetry({
        ...api.runtimeProjection,
        state: 'active',
        operationId: 1,
        runningOperationId: null,
        activeProjectionSequence: 1,
        completedAtMs: 1,
      }),
    );
    await waitFor(() => expect(screen.queryByText(failure)).not.toBeInTheDocument());
  });

  it('does not send an Audio clip move to an Instrument Track', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push(
      {
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id: 'track:audio',
        name: 'Audio',
        kind: 'audio',
        gainDb: 0,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        monitoring: 'off',
        midiInput: {},
        effects: [],
      },
      {
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id: 'track:instrument',
        name: 'Instrument',
        kind: 'instrument',
        gainDb: 0,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        monitoring: 'off',
        midiInput: {},
        effects: [],
      },
    );
    session.arrangement.audioClips.push({
      id: 'clip:movable-audio',
      name: 'Movable audio',
      trackId: 'track:audio',
      assetId: toAssetId('asset:audio'),
      startTick: 0,
      sourceRange: { start: 0, end: 48_000 },
      sourceSampleRate: 48_000,
      timelineDuration: { frames: 48_000, sampleRate: 48_000 },
      gainDb: 0,
      pan: 0,
      fadeIn: { frames: 0, sampleRate: 48_000 },
      fadeOut: { frames: 0, sampleRate: 48_000 },
      loopEnabled: false,
      muted: false,
      fadeShape: 'equalPower',
      takeVariant: 'raw',
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = renderWorkspace(api, { session });
    const clip = container.querySelector<HTMLElement>('[data-clip-id="clip:movable-audio"]')!;

    fireEvent.pointerDown(clip, { pointerId: 1, clientX: 0, clientY: 0 });
    fireEvent.pointerMove(clip, { pointerId: 1, clientX: 0, clientY: 0 });
    fireEvent.pointerUp(clip, { pointerId: 1, clientX: 0, clientY: 0 });

    await waitFor(() =>
      expect(
        screen.getByText('Audio Clips can only be placed on an Audio Track.'),
      ).toBeInTheDocument(),
    );
    expect(api.calls).not.toContain('moveAudioClips');
  });

  it('preserves an off-grid clip tick when dragging it vertically to another track', async () => {
    const session = offGridAudioMoveSession();
    let movedClips: AudioClipMove[] = [];
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.setResponse('moveAudioClips', (moves: AudioClipMove[]) => {
      movedClips = moves;
      return api.mutationResult(session);
    });
    const { container } = renderWorkspace(api, { session });
    const rows = [...container.querySelectorAll<HTMLElement>('[data-arrange-track]')];
    rows.forEach((row, index) => {
      vi.spyOn(row, 'getBoundingClientRect').mockReturnValue({
        top: index * 70,
        bottom: (index + 1) * 70,
      } as DOMRect);
    });
    const clip = container.querySelector<HTMLElement>('[data-clip-id="clip:off-grid"]')!;

    fireEvent.pointerDown(clip, { pointerId: 1, clientX: 100, clientY: 35 });
    fireEvent.pointerMove(clip, { pointerId: 1, clientX: 102, clientY: 85 });
    fireEvent.pointerMove(clip, { pointerId: 1, clientX: 107, clientY: 115 });
    fireEvent.pointerUp(clip, { pointerId: 1, clientX: 107, clientY: 115 });

    await waitFor(() => expect(api.calls).toContain('moveAudioClips'));
    expect(movedClips).toEqual([
      { clipId: 'clip:off-grid', startTick: 1_100, trackId: 'track:target' },
    ]);
  });

  it('recognizes a horizontal move after the clip has moved vertically', async () => {
    const session = offGridAudioMoveSession();
    let movedClips: AudioClipMove[] = [];
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.setResponse('moveAudioClips', (moves: AudioClipMove[]) => {
      movedClips = moves;
      return api.mutationResult(session);
    });
    const { container } = renderWorkspace(api, { session });
    const rows = [...container.querySelectorAll<HTMLElement>('[data-arrange-track]')];
    rows.forEach((row, index) => {
      vi.spyOn(row, 'getBoundingClientRect').mockReturnValue({
        top: index * 70,
        bottom: (index + 1) * 70,
      } as DOMRect);
    });
    const clip = container.querySelector<HTMLElement>('[data-clip-id="clip:off-grid"]')!;

    fireEvent.pointerDown(clip, { pointerId: 1, clientX: 100, clientY: 35 });
    fireEvent.pointerMove(clip, { pointerId: 1, clientX: 102, clientY: 105 });
    fireEvent.pointerMove(clip, { pointerId: 1, clientX: 132, clientY: 105 });
    fireEvent.pointerUp(clip, { pointerId: 1, clientX: 132, clientY: 105 });

    await waitFor(() => expect(api.calls).toContain('moveAudioClips'));
    expect(movedClips).toHaveLength(1);
    expect(movedClips[0]).toMatchObject({
      clipId: 'clip:off-grid',
      trackId: 'track:target',
    });
    expect(movedClips[0].startTick).not.toBe(1_100);
  });

  it('adds a Track from the context menu on the empty area below the Tracks', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument',
      name: 'Instrument',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [],
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let addArgs: Parameters<FakeNativeApi['addTrack']> | undefined;
    api.addTrack = async (...args) => {
      addArgs = args;
      api.calls.push('addTrack');
      return api.mutationResult(session);
    };
    const { container } = renderWorkspace(api, { session });
    const timeline = container.querySelector('[data-arrange-timeline]')!;

    // Act
    fireEvent.contextMenu(timeline, { clientX: 300, clientY: 300 });
    fireEvent.click(screen.getByRole('menuitem', { name: 'Add Audio Track' }));

    // Assert: the first Audio Track is numbered independently of Instrument Tracks
    await waitFor(() => expect(addArgs).toBeDefined());
    expect(addArgs?.[0]).toBe('Audio 1');
    expect(addArgs?.[1]).toBe('audio');
  });

  it('updates the loop and punch ranges from the Arrange timeline', async () => {
    const session = defaultSession();
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    let loopUpdates = 0;
    const api = createFakeNativeApi({
      bootstrapState: { canonical: canonicalState(session) },
    });
    api.setResponse('updateTimelineLoopRange', (enabled: boolean) => {
      const canonical = structuredClone(session);
      loopUpdates += 1;
      canonical.arrangement.loopRange = enabled
        ? {
            enabled,
            startTick: loopUpdates === 1 ? 960 : 0,
            endTick: loopUpdates === 1 ? 3840 : 2880,
          }
        : { enabled: false, startTick: 0, endTick: 0 };
      return api.mutationResult(canonical);
    });
    const widenedPunch = structuredClone(session);
    widenedPunch.arrangement.punchRange = { startTick: 0, endTick: 2880 };
    api.setResponse('updateTimelinePunchRange', () => api.mutationResult(widenedPunch));
    renderWorkspace(api, { session });

    const startHandle = screen.getByRole('slider', { name: 'Loop start' });
    fireEvent.pointerDown(startHandle, { clientX: 0 });
    fireEvent.pointerMove(window, { clientX: 100 });
    fireEvent.pointerUp(window, { clientX: 100 });

    const loopRange = screen.getByText('LOOP').parentElement!;
    await waitFor(() => expect(loopRange).toHaveStyle({ left: '96px' }));

    const endHandle = screen.getByRole('slider', { name: 'Loop end' });
    fireEvent.pointerDown(endHandle, { clientX: 0 });
    fireEvent.pointerMove(window, { clientX: -100 });
    fireEvent.pointerUp(window, { clientX: -100 });

    await waitFor(() => expect(loopRange).toHaveStyle({ width: '288px' }));
    expect(api.calls.filter((call) => call === 'updateTimelineLoopRange')).toHaveLength(2);

    const ruler = screen.getByLabelText('Timeline ruler');
    fireEvent.contextMenu(ruler, { clientX: 200, clientY: 8, button: 2 });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Clear Loop' }));

    await waitFor(() => expect(screen.queryByText('LOOP')).not.toBeInTheDocument());

    const punchEnd = screen.getByRole('slider', { name: 'Punch end' });
    fireEvent.pointerDown(punchEnd, { clientX: 0 });
    fireEvent.pointerMove(window, { clientX: 100 });
    fireEvent.pointerUp(window, { clientX: 100 });

    await waitFor(() => expect(punchEnd.parentElement!).toHaveStyle({ width: '288px' }));
  });

  it('fits the timeline view to the time selection or to the clip extent', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.midiClips.push(
      {
        instrumentControlEvents: [],
        id: 'clip:fit-start',
        name: 'Fit start',
        trackId: 'track:unused',
        startTick: 0,
        durationTicks: 960,
        notes: [],
        events: [],
        muted: false,
        loopEnabled: false,
      },
      {
        instrumentControlEvents: [],
        id: 'clip:fit-end',
        name: 'Fit end',
        trackId: 'track:unused',
        startTick: 2880,
        durationTicks: 960,
        notes: [],
        events: [],
        muted: false,
        loopEnabled: false,
      },
    );
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = renderWorkspace(api, { session });
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 2400, top: 0, bottom: 30, right: 2400, height: 30 }),
    });
    const zoom = screen.getByRole('group', { name: 'Timeline zoom' });

    // Act: a time selection fits the selected range.
    fireEvent.pointerDown(ruler, { clientX: 96 });
    fireEvent.pointerMove(window, { clientX: 124 });
    fireEvent.pointerUp(window, { clientX: 124 });
    fireEvent.keyDown(window, { key: 'z' });

    // Assert: 960..1260 ticks fitted into 1184 usable px needs zoom > 4, so it clamps to 4.
    expect(zoom.textContent).toBe('400%');

    // Act: the clip extent fits the whole arrangement.
    fireEvent.keyDown(window, { key: 'f' });

    // Assert: 0..3840 ticks fitted into the usable width.
    const usableWidth = 1408 - TRACK_HEADER_WIDTH - 32;
    expect(zoom.textContent).toBe(`${Math.round((usableWidth / 3840) * 10 * 100)}%`);
  });

  it('adds, renames, and removes a marker through the Arrange timeline', async () => {
    const initial = defaultSession();
    const added = structuredClone(initial);
    added.arrangement.markers.push({ id: 'marker:1', name: 'Marker 1', tick: 960 });
    const renamed = structuredClone(added);
    renamed.arrangement.markers[0].name = 'Verse';
    const removed = structuredClone(renamed);
    removed.arrangement.markers = [];
    const api = createFakeNativeApi({
      bootstrapState: { canonical: canonicalState(initial) },
      responses: {
        addMarker: () => api.mutationResult(added),
        updateMarker: () => api.mutationResult(renamed),
        removeMarker: () => api.mutationResult(removed),
      },
    });
    renderWorkspace(api);
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 2_400, top: 0, bottom: 30, right: 2_400, height: 30 }),
    });

    fireEvent.pointerDown(ruler, { clientX: 96 });
    fireEvent.keyDown(window, { key: 'm' });
    expect(await screen.findByText('Marker 1')).toBeInTheDocument();

    fireEvent.doubleClick(screen.getByText('Marker 1').closest('[data-marker-id]')!);
    const nameInput = screen.getByLabelText('Name');
    nameInput.focus();
    fireEvent.change(nameInput, { target: { value: 'Verse' } });
    fireEvent.keyDown(nameInput, { key: 'm' });
    expect(api.calls.filter((call) => call === 'addMarker')).toHaveLength(1);
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Verse')).toBeInTheDocument();

    fireEvent.contextMenu(screen.getByText('Verse').closest('[data-marker-id]')!, {
      clientX: 40,
      clientY: 12,
    });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Delete' }));

    await waitFor(() => expect(api.calls).toContain('removeMarker'));
    await waitFor(() => expect(screen.queryByText('Verse')).not.toBeInTheDocument());
  });
});
