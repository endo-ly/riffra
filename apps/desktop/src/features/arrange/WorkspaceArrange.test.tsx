// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useArrangeLowerAreaController } from './hooks/useArrangeLowerAreaController';
import { WorkspaceArrange } from './WorkspaceArrange';
import { useArrangementTransport } from '@/features/transport/hooks/useArrangementTransport';
import {
  type AudioClip,
  type AudioClipMove,
  type ArrangementMutationResult,
  type CreativeSession,
  type MidiClip,
  type MidiEvent,
  type RuntimeProjectionStatus,
  type Track,
} from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/native/contracts';
import type { TransportStatus } from '@/model/domain';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import { TRACK_HEADER_WIDTH } from '@/features/arrange/model/arrange-timeline';
import { TRACK_COLOR_PALETTE } from '@/features/arrange/model/track-colors';
import { ToastStack } from '@/shared/ui/ToastStack';

const noopRetryRuntimeProjection = async (): Promise<void> => undefined;

afterEach(() => {
  cleanup();
});

function Harness({
  api,
  initialSession,
  onToggleTransport,
  runtimeProjectionStatus,
  runtimeProjectionFailure,
  runtimeProjectionRetrying,
  onRetryRuntimeProjection,
  initialFocusedTrackId,
}: {
  api: FakeNativeApi;
  initialSession?: CreativeSession;
  onToggleTransport?: () => void;
  runtimeProjectionStatus?: RuntimeProjectionStatus;
  runtimeProjectionFailure?: string | null;
  runtimeProjectionRetrying?: boolean;
  onRetryRuntimeProjection?: () => Promise<void>;
  initialFocusedTrackId?: string | null;
}) {
  const initial = initialSession ?? defaultSession();
  const [session, setSession] = useState<CreativeSession>(initial);
  const [selection, setSelection] = useState<ArrangeSelection>({ kind: 'none' });
  const [focusedTrackId, setFocusedTrackId] = useState<string | null>(
    initialFocusedTrackId ?? null,
  );
  const lower = useArrangeLowerAreaController({
    midiClips: session.arrangement.midiClips,
    selectClip: (clipId) => setSelection({ kind: 'clips', clipIds: [clipId] }),
  });
  const transport = useArrangementTransport(api, session.arrangement.timebase);
  return (
    <>
      <WorkspaceArrange
        lower={lower}
        onDisableMissingPlugin={async () => undefined}
        onReplaceMissingPlugin={async () => undefined}
        onRescanMissingPlugins={async () => undefined}
        transport={transport}
        session={session}
        applyCanonicalState={(canonical) => {
          setSession(canonical.session);
          return true;
        }}
        selection={selection}
        setSelection={setSelection}
        api={api}
        audio={api.audio}
        focusedTrackId={focusedTrackId}
        onFocusTrack={setFocusedTrackId}
        onToggleTransport={onToggleTransport ?? (() => undefined)}
        runtimeProjectionStatus={runtimeProjectionStatus ?? api.runtimeProjection}
        runtimeProjectionFailure={
          runtimeProjectionFailure ??
          (runtimeProjectionStatus?.state === 'failed'
            ? 'Audio preparation failed. Retry to prepare audio.'
            : null)
        }
        runtimeProjectionRetrying={runtimeProjectionRetrying ?? false}
        onRetryRuntimeProjection={onRetryRuntimeProjection ?? noopRetryRuntimeProjection}
      />
      <ToastStack />
    </>
  );
}

function mutationResult(session: CreativeSession): ArrangementMutationResult {
  return {
    canonical: {
      projectId: '01900000-0000-7000-8000-000000000001',
      session,
      sequence: 0,
      history: { canUndo: false, canRedo: false },
    },
    createdEntityIds: {},
    projection: { state: 'notRequired' },
  };
}

function offGridAudioMoveSession(): CreativeSession {
  const session = defaultSession();
  for (const [id, name] of [
    ['track:source', 'Source'],
    ['track:target', 'Target'],
  ]) {
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower',
      id,
      name,
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
  }
  session.arrangement.audioClips.push({
    id: 'clip:off-grid',
    name: 'Off-grid clip',
    trackId: 'track:source',
    assetId: toAssetId('asset:audio'),
    startTick: 1_100,
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
  return session;
}

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

describe('WorkspaceArrange', () => {
  it.each(['next', 'previous'] as const)(
    'merges adjacent Audio clips with the %s clip into the previous clip',
    async (direction) => {
      const session = defaultSession();
      session.arrangement.tracks.push(mergeTrack('track:audio', 'audio'));
      session.arrangement.audioClips.push(
        mergeAudioClip('clip:audio:first', 0, 0),
        mergeAudioClip('clip:audio:second', 1_920, 48_000),
      );
      const api = new FakeNativeApi();
      const update = vi.spyOn(api, 'updateAudioClip').mockResolvedValue(mutationResult(session));
      const remove = vi
        .spyOn(api, 'removeTimelineClips')
        .mockResolvedValue(mutationResult(session));
      const { container } = render(<Harness api={api} initialSession={session} />);
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
      const api = new FakeNativeApi();
      const update = vi.spyOn(api, 'updateMidiClip').mockResolvedValue(mutationResult(session));
      const remove = vi
        .spyOn(api, 'removeTimelineClips')
        .mockResolvedValue(mutationResult(session));
      const { container } = render(<Harness api={api} initialSession={session} />);
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
    const api = new FakeNativeApi();
    const { container } = render(<Harness api={api} />);
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const updateTrack = vi.spyOn(api, 'updateTrack');

    // Act
    const { container } = render(<Harness api={api} initialSession={session} />);

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
      await act(async () => resolveUpdate(mutationResult(next)));
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
    const api = new FakeNativeApi();
    render(<Harness api={api} />);
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
      const api = new FakeNativeApi();
      const { container } = render(<Harness api={api} />);
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

  it('keeps the MIDI editor view following the playhead during playback', async () => {
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
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:follow',
      name: 'Follow',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = render(<Harness api={api} initialSession={session} />);
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:follow"]')!);
    await screen.findByLabelText('MIDI Editor');
    const viewport = document.querySelector('[data-midi-pitch-viewport]') as HTMLDivElement;
    Object.defineProperty(viewport, 'clientWidth', { configurable: true, value: 800 });

    vi.useFakeTimers({ toFake: ['requestAnimationFrame', 'performance'] });
    try {
      // At 0.18 px/tick the follow line (800 px * 0.32) is crossed after 427 px,
      // i.e. while the playhead is still inside the 1,920-tick clip.
      await act(async () => {
        api.emitTransportStatus({ state: 'playing', timelineTick: 600, discontinuity: 1 });
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(viewport.scrollLeft).toBe(0);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(viewport.scrollLeft).toBeGreaterThan(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it('deletes a selected note from the MIDI editor context menu', async () => {
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
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:midi-range',
      name: 'MIDI Range',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 1_920,
      notes: [
        { id: 'note:low', note: 0, startTick: 0, durationTicks: 240, velocity: 1, channel: 0 },
        {
          id: 'note:high',
          note: 127,
          startTick: 480,
          durationTicks: 240,
          velocity: 127,
          channel: 0,
        },
      ],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:midi-range"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    const highNote = editor.querySelector('[data-note-id="note:high"]')!;
    fireEvent.click(highNote);

    expect(
      [...editor.querySelectorAll('[data-note-id]')].filter((note) =>
        note.className.includes('selected'),
      ),
    ).toHaveLength(1);
    expect(screen.getByLabelText('Selected MIDI note velocity')).toHaveValue('127');

    fireEvent.keyDown(screen.getByLabelText('Selected MIDI note velocity'), { key: 'Delete' });
    expect(api.calls).not.toContain('removeMidiNote');

    fireEvent.contextMenu(highNote, { clientX: 120, clientY: 80 });
    expect(api.calls).not.toContain('removeMidiNote');
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    expect(api.calls).toContain('removeMidiNotes');
  });

  it('quantizes only the notes that sit off the grid', async () => {
    // Arrange: one Clip holds a note on the grid, another an off-grid note.
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
    session.arrangement.midiClips.push(
      {
        instrumentControlEvents: [],
        id: 'clip:quantize',
        name: 'Quantize',
        trackId: 'track:instrument',
        startTick: 2200,
        durationTicks: 1_920,
        notes: [
          {
            id: 'note:off-grid',
            note: 60,
            startTick: 240,
            durationTicks: 240,
            velocity: 96,
            channel: 0,
          },
        ],
        events: [],
        muted: false,
        loopEnabled: false,
      },
      {
        instrumentControlEvents: [],
        id: 'clip:quantize-aligned',
        name: 'Quantize aligned',
        trackId: 'track:instrument',
        startTick: 2200,
        durationTicks: 1_920,
        notes: [
          {
            id: 'note:on-grid',
            note: 60,
            startTick: 200,
            durationTicks: 240,
            velocity: 96,
            channel: 0,
          },
        ],
        events: [],
        muted: false,
        loopEnabled: false,
      },
    );
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let finishQuantize: ((next: ArrangementMutationResult) => void) | undefined;
    api.quantizeMidiNotes = async (_clipId, _noteIds, _gridTicks) => {
      api.calls.push('quantizeMidiNotes');
      return new Promise<ArrangementMutationResult>((resolve) => {
        finishQuantize = resolve;
      });
    };
    const { container } = render(<Harness api={api} initialSession={session} />);

    // Act: quantize the note that already sits on the grid.
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:quantize-aligned"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    fireEvent.click(editor.querySelector('[data-note-id="note:on-grid"]')!);
    fireEvent.click(within(editor).getByRole('button', { name: 'Quantize' }));

    // Assert
    await screen.findByText('Selected notes are already on the grid.');
    expect(api.calls).not.toContain('quantizeMidiNotes');

    // Act: open the off-grid Clip, select the note, and hit Quantize.
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:quantize"]')!);
    const offGridNote = await waitFor(() => {
      const note = container.querySelector('[data-note-id="note:off-grid"]');
      expect(note).not.toBeNull();
      return note as HTMLElement;
    });
    fireEvent.click(offGridNote);
    fireEvent.click(
      within(screen.getByLabelText('MIDI Editor')).getByRole('button', { name: 'Quantize' }),
    );

    // Assert
    await waitFor(() => expect(finishQuantize).toBeDefined());
    expect(screen.queryByText('Quantized 1 note to 1/16.')).not.toBeInTheDocument();
    finishQuantize!(mutationResult(session));
    await screen.findByText('Quantized 1 note to 1/16.');
    expect(api.calls.filter((call) => call === 'quantizeMidiNotes')).toHaveLength(1);
  });

  it('keeps MIDI note editing shortcuts inside the focused editor', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument-shortcuts',
      name: 'Instrument Shortcuts',
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
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:shortcuts',
      name: 'Shortcuts',
      trackId: 'track:instrument-shortcuts',
      startTick: 0,
      durationTicks: 1_920,
      notes: [
        {
          id: 'note:shortcut-a',
          note: 60,
          startTick: 0,
          durationTicks: 240,
          velocity: 96,
          channel: 0,
        },
        {
          id: 'note:shortcut-b',
          note: 64,
          startTick: 480,
          durationTicks: 240,
          velocity: 96,
          channel: 0,
        },
      ],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({
      bootstrapState: { canonical: canonicalState(session) },
    });
    let updateArgs: Parameters<FakeNativeApi['updateMidiNotes']> | undefined;
    api.updateMidiNotes = async (...args) => {
      updateArgs = args;
      api.calls.push('updateMidiNotes');
      return mutationResult(session);
    };
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:shortcuts"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    fireEvent.keyDown(editor, { key: 'a', ctrlKey: true });

    expect(
      [...editor.querySelectorAll('[data-note-id]')].every((note) =>
        note.className.includes('selected'),
      ),
    ).toBe(true);

    fireEvent.keyDown(editor, { key: 'ArrowDown', shiftKey: true });

    await waitFor(() => expect(updateArgs).toBeDefined());
    expect(updateArgs?.[1].map((update) => update.patch.note)).toEqual([48, 52]);

    fireEvent.keyDown(editor, { key: 'Delete' });

    expect(api.calls).toContain('removeMidiNotes');
    expect(api.calls).not.toContain('removeTimelineClips');
  });

  it('creates an empty MIDI clip from an instrument lane and opens its editor', async () => {
    const session = defaultSession();
    const track = {
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument-empty',
      name: 'Instrument Empty',
      kind: 'instrument' as const,
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off' as const,
      midiInput: {},
      effects: [],
    };
    session.arrangement.tracks.push(track);
    const createdSession: CreativeSession = {
      ...session,
      arrangement: {
        ...session.arrangement,
        midiClips: [
          {
            instrumentControlEvents: [],
            id: 'clip:created-empty',
            name: 'MIDI Clip',
            trackId: track.id,
            startTick: 480,
            durationTicks: 1_920,
            notes: [],
            events: [],
            muted: false,
            loopEnabled: false,
          },
        ],
      },
    };
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let createArgs: Parameters<FakeNativeApi['createMidiClip']> | undefined;
    api.createMidiClip = async (...args) => {
      createArgs = args;
      api.calls.push('createMidiClip');
      return mutationResult(createdSession);
    };
    const { container } = render(<Harness api={api} initialSession={session} />);
    const lane = container.querySelector(`[data-track-id="${track.id}"] > div[class*="lane_"]`)!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 100, top: 0, right: 800, bottom: 80, width: 700, height: 80 }),
    });

    fireEvent.doubleClick(lane, { clientX: 196, clientY: 40 });

    await waitFor(() => expect(createArgs).toBeDefined());
    expect(createArgs?.[0]).toBe(track.id);
    expect(createArgs?.[2]).toBe(session.arrangement.timebase.ppq * 4);
    expect(await screen.findByLabelText('MIDI Editor')).toBeInTheDocument();
    expect(container.querySelector('[data-clip-id="clip:created-empty"]')).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('uses the active Time Selection for an empty MIDI clip', async () => {
    const session = defaultSession();
    const track = {
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument-selection',
      name: 'Instrument Selection',
      kind: 'instrument' as const,
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off' as const,
      midiInput: {},
      effects: [],
    };
    session.arrangement.tracks.push(track);
    const createdSession: CreativeSession = {
      ...session,
      arrangement: {
        ...session.arrangement,
        midiClips: [
          {
            instrumentControlEvents: [],
            id: 'clip:selection-created',
            name: 'Selection Clip',
            trackId: track.id,
            startTick: 960,
            durationTicks: 960,
            notes: [],
            events: [],
            muted: false,
            loopEnabled: false,
          },
        ],
      },
    };
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let createArgs: Parameters<FakeNativeApi['createMidiClip']> | undefined;
    api.createMidiClip = async (...args) => {
      createArgs = args;
      api.calls.push('createMidiClip');
      return mutationResult(createdSession);
    };
    const { container } = render(<Harness api={api} initialSession={session} />);
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 2_400, top: 0, bottom: 30, right: 2_400, height: 30 }),
    });

    fireEvent.pointerDown(ruler, { clientX: 96 });
    fireEvent.pointerMove(window, { clientX: 192 });
    fireEvent.pointerUp(window, { clientX: 192 });

    const lane = container.querySelector(`[data-track-id="${track.id}"] > div[class*="lane_"]`)!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 100, top: 0, right: 800, bottom: 80, width: 700, height: 80 }),
    });
    fireEvent.doubleClick(lane, { clientX: 200, clientY: 40 });

    await waitFor(() => expect(createArgs).toBeDefined());
    expect(createArgs?.[1]).toBe(960);
    expect(createArgs?.[2]).toBe(960);
    expect(await screen.findByLabelText('MIDI Editor')).toBeInTheDocument();
  });

  it('keeps the active MIDI Clip while selecting Audio or appending another MIDI Clip', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push(
      {
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id: 'track:audio-selection',
        name: 'Audio Selection',
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
        id: 'track:midi-selection',
        name: 'MIDI Selection',
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
      id: 'clip:audio-selection',
      name: 'Audio Selection Clip',
      trackId: 'track:audio-selection',
      assetId: toAssetId('asset:audio-selection'),
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
    session.arrangement.midiClips.push(
      {
        instrumentControlEvents: [],
        id: 'clip:midi-selection-a',
        name: 'MIDI Selection A',
        trackId: 'track:midi-selection',
        startTick: 0,
        durationTicks: 1_920,
        notes: [],
        events: [],
        muted: false,
        loopEnabled: false,
      },
      {
        instrumentControlEvents: [],
        id: 'clip:midi-selection-b',
        name: 'MIDI Selection B',
        trackId: 'track:midi-selection',
        startTick: 2_400,
        durationTicks: 1_920,
        notes: [],
        events: [],
        muted: false,
        loopEnabled: false,
      },
    );
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:midi-selection-a"]')!);
    await screen.findByLabelText('MIDI Editor');
    fireEvent.click(container.querySelector('[data-clip-id="clip:midi-selection-b"]')!, {
      ctrlKey: true,
    });
    expect(screen.getByLabelText('MIDI Editor')).toHaveAttribute(
      'data-midi-editor-clip-id',
      'clip:midi-selection-a',
    );

    fireEvent.click(container.querySelector('[data-clip-id="clip:audio-selection"]')!);
    expect(screen.getByLabelText('MIDI Editor')).toBeInTheDocument();
    expect(screen.getByLabelText('MIDI Editor')).toHaveAttribute(
      'data-midi-editor-clip-id',
      'clip:midi-selection-a',
    );

    fireEvent.click(container.querySelector('[data-clip-id="clip:midi-selection-b"]')!);
    expect(screen.getByLabelText('MIDI Editor')).toHaveAttribute(
      'data-midi-editor-clip-id',
      'clip:midi-selection-b',
    );
  });

  it('uses Pointer blank clicks for selection and Draw drags for note creation', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument-draw',
      name: 'Instrument Draw',
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
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:draw',
      name: 'Draw',
      trackId: 'track:instrument-draw',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let addArgs: Parameters<FakeNativeApi['addMidiNote']> | undefined;
    api.addMidiNote = async (...args) => {
      addArgs = args;
      api.calls.push('addMidiNote');
      return mutationResult(session);
    };
    const { container } = render(<Harness api={api} initialSession={session} />);
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:draw"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    const lane = editor.querySelector('[data-midi-lane]')!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 0, top: 0, right: 400, bottom: 1_536, width: 400, height: 1_536 }),
    });

    fireEvent.pointerDown(lane, { clientX: 100, clientY: 804, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 100, clientY: 804, pointerId: 1 });
    expect(api.calls).not.toContain('addMidiNote');

    fireEvent.click(within(editor).getByRole('button', { name: 'Draw' }));
    fireEvent.pointerDown(lane, { clientX: 100, clientY: 804, pointerId: 2 });
    fireEvent.pointerMove(window, { clientX: 160, clientY: 804, pointerId: 2 });
    const drawPreview = editor.querySelector('[class*="drawPreview"]') as HTMLElement;
    expect(Number.parseFloat(drawPreview.style.top)).toBe(804);
    fireEvent.pointerUp(window, { clientX: 160, clientY: 804, pointerId: 2 });

    await waitFor(() => expect(addArgs).toBeDefined());
    expect(addArgs?.[2]).toBe(60);
    expect(addArgs?.[3]).toBeGreaterThan(0);
    expect(addArgs?.[4]).toBe(96);
  });

  it('auditions a newly placed note on the active instrument', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:audition',
      name: 'Audition Track',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:audition',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:audition',
      name: 'Audition Clip',
      trackId: 'track:audition',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sent: { trackId: string; bytes: number[] }[] = [];
    api.sendMidiToTrack = async (trackId, bytes) => {
      api.calls.push('sendMidiToTrack');
      sent.push({ trackId, bytes });
      return api.audio;
    };
    const { container } = render(<Harness api={api} initialSession={session} />);
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:audition"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    const lane = editor.querySelector('[data-midi-lane]')!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 0, top: 0, right: 400, bottom: 1_536, width: 400, height: 1_536 }),
    });

    fireEvent.doubleClick(lane, { clientX: 100, clientY: 804 });

    await waitFor(() => expect(api.calls).toContain('addMidiNote'));
    const noteOn = sent.find((message) => message.bytes[0] === 0x90);
    expect(noteOn?.trackId).toBe('track:audition');
    expect(noteOn?.bytes).toEqual([0x90, 60, 96]);
  });

  it('shows overlapping notes from other tracks behind the active clip', async () => {
    const session = defaultSession();
    for (const [trackId, name] of [
      ['track:melody', 'Melody'],
      ['track:chords', 'Chords'],
    ] as const) {
      session.arrangement.tracks.push({
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id: trackId,
        name,
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
    }
    session.arrangement.midiClips.push(
      {
        instrumentControlEvents: [],
        id: 'clip:melody',
        name: 'Melody Clip',
        trackId: 'track:melody',
        startTick: 0,
        durationTicks: 1_920,
        notes: [],
        events: [],
        muted: false,
        loopEnabled: false,
      },
      {
        instrumentControlEvents: [],
        id: 'clip:chords',
        name: 'Chords Clip',
        trackId: 'track:chords',
        startTick: 0,
        durationTicks: 1_920,
        notes: [
          {
            id: 'note:chords',
            note: 64,
            startTick: 480,
            durationTicks: 240,
            velocity: 80,
            channel: 0,
          },
        ],
        events: [],
        muted: false,
        loopEnabled: false,
      },
    );
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = render(<Harness api={api} initialSession={session} />);
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:melody"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');

    expect(editor.querySelector('[data-note-id]')).toBeNull();
    expect(editor.querySelector('[data-ghost-note-id]')?.getAttribute('style')).toContain(
      `--ghost-color: ${TRACK_COLOR_PALETTE[1]}`,
    );

    fireEvent.click(within(editor).getByRole('button', { name: 'Reference notes' }));
    expect(editor.querySelector('[data-ghost-note-id]')).toBeNull();
  });

  it('previews a dragged note group until the canonical update lands', async () => {
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
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:midi-group',
      name: 'MIDI Group',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 1_920,
      notes: [
        {
          id: 'note:group-a',
          note: 60,
          startTick: 0,
          durationTicks: 240,
          velocity: 96,
          channel: 0,
        },
        {
          id: 'note:group-b',
          note: 64,
          startTick: 480,
          durationTicks: 240,
          velocity: 96,
          channel: 0,
        },
      ],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const originalUpdateMidiNotes = api.updateMidiNotes.bind(api);
    let releaseUpdate!: () => void;
    api.updateMidiNotes = (...args) => {
      const canonical = originalUpdateMidiNotes(...args);
      return new Promise<ArrangementMutationResult>((resolve) => {
        releaseUpdate = () => {
          void canonical.then((next) =>
            resolve({
              ...next,
              canonical: {
                ...next.canonical,
                session: {
                  ...next.canonical.session,
                  arrangement: {
                    ...next.canonical.session.arrangement,
                    midiClips: next.canonical.session.arrangement.midiClips.map((clip) =>
                      clip.id === 'clip:midi-group'
                        ? {
                            ...clip,
                            notes: clip.notes.map((note) =>
                              note.id === 'note:group-a' ? { ...note, startTick: 120 } : note,
                            ),
                          }
                        : clip,
                    ),
                  },
                },
              },
            }),
          );
        };
      });
    };
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:midi-group"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    const lane = editor.querySelector('div[class*="laneViewport"] div[class*="lane_"]')!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 0, top: 0, right: 400, bottom: 1_536, width: 400, height: 1_536 }),
    });
    const first = editor.querySelector('[data-note-id="note:group-a"]')! as HTMLElement;
    const second = editor.querySelector('[data-note-id="note:group-b"]')! as HTMLElement;
    fireEvent.click(first);
    fireEvent.click(second, { ctrlKey: true });

    fireEvent.pointerDown(first, { clientX: 100, clientY: 804, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 148, clientY: 792, pointerId: 1 });
    expect(Number.parseFloat(first.style.left)).toBeCloseTo(43.2);
    expect(Number.parseFloat(second.style.left)).toBeCloseTo(129.6);
    expect(second.style.top).toBe('744px');

    fireEvent.pointerUp(window, { clientX: 148, clientY: 792, pointerId: 1 });
    expect(api.calls).toContain('updateMidiNotes');
    expect(Number.parseFloat(first.style.left)).toBeCloseTo(43.2);
    expect(Number.parseFloat(second.style.left)).toBeCloseTo(129.6);
    expect(container.querySelector('[class*="toast_"]')).not.toBeInTheDocument();

    releaseUpdate();
    await waitFor(() => expect(Number.parseFloat(first.style.left)).toBeCloseTo(21.6));
  });

  it('reports clips when confirming Track deletion and closes the selected Track Devices', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument-delete',
      name: 'Instrument Delete',
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
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:midi-delete',
      name: 'MIDI Delete',
      trackId: 'track:instrument-delete',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const afterDelete = structuredClone(session);
    afterDelete.arrangement.tracks = afterDelete.arrangement.tracks.filter(
      (track) => track.id !== 'track:instrument-delete',
    );
    afterDelete.arrangement.midiClips = [];
    api.removeTrack = vi.fn().mockResolvedValue(mutationResult(afterDelete));
    render(<Harness api={api} initialSession={session} />);

    // Act
    fireEvent.click(screen.getByLabelText('Instrument Delete track menu'));
    fireEvent.click(screen.getByRole('button', { name: 'Open Devices' }));
    expect(screen.getByRole('region', { name: 'Arrange lower area' })).toHaveAttribute(
      'data-view',
      'devices',
    );
    fireEvent.click(screen.getByLabelText('Instrument Delete track menu'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    // Assert
    expect(screen.getByText(/This also removes 1 Clip from the Timeline/)).toBeInTheDocument();
    expect(screen.getByText(/Source assets will be kept\./)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Delete Track' }));
    await waitFor(() =>
      expect(screen.queryByRole('region', { name: 'Arrange lower area' })).not.toBeInTheDocument(),
    );
  });

  it('opens Devices from the track menu with available and disabled plugins', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:guitar',
      name: 'Guitar',
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [
        {
          id: 'device:amp',
          name: 'Amplitube',
          bypassed: false,
          plugin: {
            path: 'C:/Plugins/Amplitube.vst3',
            parameterValues: [],
            disabledPlaceholder: false,
          },
        },
        {
          id: 'device:disabled',
          name: 'Disabled Effect',
          bypassed: false,
          plugin: {
            path: 'C:/Plugins/Disabled.vst3',
            parameterValues: [],
            disabledPlaceholder: true,
          },
        },
      ],
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    render(<Harness api={api} initialSession={session} />);

    // Act
    fireEvent.click(screen.getByLabelText('Guitar track menu'));
    fireEvent.click(screen.getByRole('button', { name: 'Open Devices' }));

    // Assert
    expect(screen.getByRole('region', { name: 'Arrange lower area' })).toHaveAttribute(
      'data-view',
      'devices',
    );
    expect(screen.getByRole('button', { name: 'Amplitube' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Disabled Effect' })).toBeInTheDocument();
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
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

    render(
      <Harness
        api={api}
        initialSession={session}
        runtimeProjectionStatus={runtimeProjectionStatus}
      />,
    );

    expect(await screen.findByText(/Playback skipped 1 missing source/)).toBeInTheDocument();
    expect(await screen.findByText('Lost Take')).toBeInTheDocument();
    expect((await screen.findAllByText('MISSING SOURCE')).length).toBeGreaterThan(0);
    expect(document.querySelector('[data-clip-id="clip:missing"]')).toBeInTheDocument();
  });

  it('reports a projection failure in place and recovers through the retry action', async () => {
    // Arrange: the projection fails and the message takes over the status area.
    const api = new FakeNativeApi();
    const failure = 'Audio preparation failed. Retry to prepare audio.';
    const failedStatus: RuntimeProjectionStatus = {
      ...api.runtimeProjection,
      state: 'failed',
      operationId: 1,
      lastError: 'native rejected',
      lastErrorCode: 'nativeRejected',
    };
    let retried = false;
    const onRetryRuntimeProjection = async () => {
      retried = true;
    };

    const { rerender } = render(
      <Harness
        api={api}
        runtimeProjectionStatus={failedStatus}
        runtimeProjectionFailure={failure}
        onRetryRuntimeProjection={onRetryRuntimeProjection}
      />,
    );

    // Assert
    expect(await screen.findByText(failure)).toBeInTheDocument();
    expect(screen.queryByText('native rejected')).not.toBeInTheDocument();

    // Act: Retry reports progress instead of a raw error while it is running.
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(retried).toBe(true);

    rerender(
      <Harness
        api={api}
        runtimeProjectionStatus={failedStatus}
        runtimeProjectionRetrying
        onRetryRuntimeProjection={onRetryRuntimeProjection}
      />,
    );
    expect(await screen.findByText('Retrying audio preparation…')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument();
    expect(screen.queryByText('native rejected')).not.toBeInTheDocument();

    // Assert: the failure no longer covers the Arrangement once preparation succeeds.
    rerender(<Harness api={api} onRetryRuntimeProjection={onRetryRuntimeProjection} />);
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = render(<Harness api={api} initialSession={session} />);
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.setResponse('moveAudioClips', (moves: AudioClipMove[]) => {
      movedClips = moves;
      return mutationResult(session);
    });
    const { container } = render(<Harness api={api} initialSession={session} />);
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.setResponse('moveAudioClips', (moves: AudioClipMove[]) => {
      movedClips = moves;
      return mutationResult(session);
    });
    const { container } = render(<Harness api={api} initialSession={session} />);
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let addArgs: Parameters<FakeNativeApi['addTrack']> | undefined;
    api.addTrack = async (...args) => {
      addArgs = args;
      api.calls.push('addTrack');
      return mutationResult(session);
    };
    const { container } = render(<Harness api={api} initialSession={session} />);
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
    const api = new FakeNativeApi({
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
      return mutationResult(canonical);
    });
    const widenedPunch = structuredClone(session);
    widenedPunch.arrangement.punchRange = { startTick: 0, endTick: 2880 };
    api.setResponse('updateTimelinePunchRange', () => mutationResult(widenedPunch));
    render(<Harness api={api} initialSession={session} />);

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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = render(<Harness api={api} initialSession={session} />);
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
    const api = new FakeNativeApi({
      bootstrapState: { canonical: canonicalState(initial) },
      responses: {
        addMarker: mutationResult(added),
        updateMarker: mutationResult(renamed),
        removeMarker: mutationResult(removed),
      },
    });
    render(<Harness api={api} />);
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

  it('commits a velocity lane drag once and previews the active instrument', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:velocity',
      name: 'Velocity Track',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:velocity',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:velocity',
      name: 'Velocity Clip',
      trackId: 'track:velocity',
      startTick: 960,
      durationTicks: 1_920,
      notes: [
        {
          id: 'note:velocity',
          note: 60,
          startTick: 0,
          durationTicks: 240,
          velocity: 64,
          channel: 0,
        },
        {
          id: 'note:velocity-secondary',
          note: 64,
          startTick: 480,
          durationTicks: 240,
          velocity: 80,
          channel: 0,
        },
      ],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const committedSession: CreativeSession = {
      ...session,
      arrangement: {
        ...session.arrangement,
        midiClips: session.arrangement.midiClips.map((clip) =>
          clip.id === 'clip:velocity'
            ? {
                ...clip,
                notes: clip.notes.map((note) =>
                  note.id === 'note:velocity'
                    ? { ...note, velocity: 76 }
                    : note.id === 'note:velocity-secondary'
                      ? { ...note, velocity: 92 }
                      : note,
                ),
              }
            : clip,
        ),
      },
    };
    let resolveUpdate!: (next: ArrangementMutationResult) => void;
    const pendingUpdate = new Promise<ArrangementMutationResult>((resolve) => {
      resolveUpdate = resolve;
    });
    let updateArgs: Parameters<FakeNativeApi['updateMidiNotes']> | undefined;
    api.updateMidiNotes = async (...args) => {
      updateArgs = args;
      api.calls.push('updateMidiNotes');
      return pendingUpdate;
    };
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:velocity"]')!);
    const velocityLane = await screen.findByLabelText('MIDI Editor');
    const lane = velocityLane.querySelector('[data-velocity-lane]') as HTMLElement;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 0, top: 0, width: 400, height: 88, right: 400, bottom: 88 }),
    });
    const bar = lane.querySelector('[data-velocity-note-id="note:velocity"]') as HTMLElement;
    const secondaryBar = lane.querySelector(
      '[data-velocity-note-id="note:velocity-secondary"]',
    ) as HTMLElement;
    fireEvent.click(velocityLane.querySelector('[data-note-id="note:velocity"]')!);
    fireEvent.click(velocityLane.querySelector('[data-note-id="note:velocity-secondary"]')!, {
      ctrlKey: true,
    });

    fireEvent.pointerDown(bar, { pointerId: 1, clientY: 40 });
    expect(bar).toHaveAttribute('aria-label', 'C4 velocity 64');
    expect(secondaryBar).toHaveAttribute('aria-label', 'E4 velocity 80');
    fireEvent.pointerUp(window, { pointerId: 1, clientY: 40 });
    expect(api.calls).not.toContain('updateMidiNotes');

    fireEvent.pointerDown(bar, { pointerId: 1, clientY: 40 });
    expect(bar).toHaveAttribute('aria-label', 'C4 velocity 64');
    expect(secondaryBar).toHaveAttribute('aria-label', 'E4 velocity 80');
    fireEvent.pointerMove(window, { pointerId: 1, clientY: 32 });
    expect(bar).toHaveAttribute('aria-label', 'C4 velocity 76');
    expect(secondaryBar).toHaveAttribute('aria-label', 'E4 velocity 92');
    fireEvent.pointerUp(window, { pointerId: 1, clientY: 32 });

    await waitFor(() => expect(api.calls).toContain('updateMidiNotes'));
    expect(api.calls.filter((call) => call === 'updateMidiNotes')).toHaveLength(1);
    expect(updateArgs?.[1].map((update) => update.patch.velocity)).toEqual([76, 92]);
    expect(bar).toHaveAttribute('aria-label', 'C4 velocity 76');
    expect(secondaryBar).toHaveAttribute('aria-label', 'E4 velocity 92');

    resolveUpdate(mutationResult(committedSession));
    await waitFor(() => expect(bar).toHaveAttribute('aria-label', 'C4 velocity 76'));

    const pianoKey = velocityLane.querySelector('[data-piano-key="60"]') as HTMLElement;
    fireEvent.pointerDown(pianoKey, { pointerId: 2 });
    fireEvent.pointerUp(pianoKey, { pointerId: 2 });
    expect(api.calls.filter((call) => call === 'sendMidiToTrack')).toHaveLength(2);
  });

  it('resizes, collapses, maximizes, and closes the Arrange lower area', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:navigation',
      name: 'Navigation Track',
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
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:navigation',
      name: 'Navigation Clip',
      trackId: 'track:navigation',
      startTick: 1_920,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:navigation"]')!);
    await screen.findByLabelText('MIDI Editor');

    const workspace = screen.getByLabelText('Arrange timeline');
    Object.defineProperty(workspace, 'clientHeight', {
      configurable: true,
      value: 600,
    });
    const resizeHandle = screen.getByRole('separator', { name: 'Resize Arrange lower area' });
    // The default lower area height is 380: dragging 210px further down lands below the
    // 180px minimum and collapses instead of shrinking.
    fireEvent.pointerDown(resizeHandle, { clientY: 500 });
    fireEvent.pointerMove(window, { clientY: 710 });
    expect(screen.getByRole('button', { name: 'Restore lower area' })).toBeInTheDocument();
    fireEvent.pointerUp(window, { clientY: 710 });

    const lowerArea = screen.getByRole('region', { name: 'Arrange lower area' });
    fireEvent.click(screen.getByRole('button', { name: 'Restore lower area' }));
    expect(lowerArea.querySelector('[hidden]')).not.toBeInTheDocument();

    fireEvent.pointerDown(resizeHandle, { clientY: 500 });
    fireEvent.pointerMove(window, { clientY: -200 });
    fireEvent.pointerUp(window, { clientY: -200 });
    expect(lowerArea).toHaveStyle('--lower-height: 558px');

    fireEvent.click(screen.getByRole('button', { name: 'Collapse lower area' }));
    expect(screen.getByRole('button', { name: 'Restore lower area' })).toBeInTheDocument();
    expect(lowerArea.querySelector('[hidden]')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Maximize lower area' }));
    expect(screen.getByRole('button', { name: 'Restore lower area size' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close lower area' }));
    expect(screen.queryByRole('region', { name: 'Arrange lower area' })).not.toBeInTheDocument();
  });

  it('shows the Mixer and the MIDI editor in the shared lower area', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:focused',
      name: 'Focused Instrument',
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
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:mixer',
      name: 'Mix Track',
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    render(<Harness api={api} initialSession={session} initialFocusedTrackId="track:focused" />);

    const mixerToggle = screen.getByRole('button', { name: 'Mixer' });
    fireEvent.click(mixerToggle);

    const lowerArea = screen.getByRole('region', { name: 'Arrange lower area' });
    expect(lowerArea.querySelector('[data-mixer-panel]')).toBeInTheDocument();
    const trackChannel = screen.getByRole('article', { name: 'Mix Track mixer channel' });
    expect(screen.getByRole('complementary', { name: 'Master mixer channel' })).toBeInTheDocument();

    fireEvent.click(within(trackChannel).getByTitle('Open Track Devices'));
    await waitFor(() => expect(lowerArea).toHaveAttribute('data-view', 'devices'));
    fireEvent.click(mixerToggle);
    expect(lowerArea).toHaveAttribute('data-view', 'mixer');
    fireEvent.click(mixerToggle);
    expect(lowerArea).toHaveAttribute('data-view', 'devices');
  });

  it('plays the focused instrument from the keyboard and drum pads on their MIDI channels', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:live',
      name: 'Live Keys',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:live',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:live',
      name: 'Live Clip',
      trackId: 'track:live',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    const setLiveTarget = vi.spyOn(api, 'setLiveMidiTarget');
    const { container } = render(<Harness api={api} initialSession={session} />);
    fireEvent.click(screen.getByText('Live Keys'));
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });

    // Act: play without opening any panel.
    fireEvent.click(computerKeyboard);
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });

    // Assert
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'true');
    expect(computerKeyboard).toHaveTextContent('C4');
    expect(screen.queryByRole('region', { name: 'Play Surface' })).not.toBeInTheDocument();
    await waitFor(() => expect(sendMidi).toHaveBeenCalledWith('track:live', [0x90, 60, 100]));
    expect(setLiveTarget).toHaveBeenLastCalledWith('track:live');

    // Act: the Play Surface opens over the MIDI Editor and returns to it.
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:live"]')!);
    const lowerArea = screen.getByRole('region', { name: 'Arrange lower area' });
    fireEvent.click(screen.getByRole('button', { name: 'Play Surface' }));

    // Assert
    expect(lowerArea).toHaveAttribute('data-view', 'playSurface');
    expect(screen.getByRole('region', { name: 'Play Surface' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Drum Pads' }));
    const pad = screen.getByRole('gridcell', {
      name: 'Crash Cymbal 1 (MIDI 49, key Q)',
    });
    Object.defineProperty(pad, 'setPointerCapture', {
      configurable: true,
      value: vi.fn(),
    });
    fireEvent.pointerDown(pad, { pointerId: 1 });
    fireEvent.keyDown(window, { key: 'q' });
    await waitFor(() => expect(sendMidi).toHaveBeenCalledWith('track:live', [0x99, 49, 100]));
    fireEvent.keyUp(window, { key: 'q' });
    expect(
      sendMidi.mock.calls.filter(([, bytes]) => bytes[0] === 0x89 && bytes[1] === 49),
    ).toHaveLength(0);
    fireEvent.pointerUp(pad, { pointerId: 1 });
    await waitFor(() => expect(sendMidi).toHaveBeenCalledWith('track:live', [0x89, 49, 0]));

    fireEvent.click(screen.getByRole('button', { name: 'Play Surface' }));
    expect(lowerArea).toHaveAttribute('data-view', 'midiEditor');
  });

  it('keeps a held performance key away from the Arrange zoom shortcuts', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:live',
      name: 'Live Keys',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:live',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:live',
      name: 'Live Clip',
      trackId: 'track:live',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    const { container } = render(<Harness api={api} initialSession={session} />);
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    fireEvent.click(screen.getByText('Live Keys'));
    fireEvent.click(screen.getByRole('button', { name: 'Computer keyboard input' }));
    const zoom = screen.getByRole('group', { name: 'Timeline zoom' });
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });
    const before = zoom.textContent;

    // Act: hold the typing keys, which are also Arrange zoom shortcuts.
    fireEvent.keyDown(window, { key: 'f' });
    fireEvent.keyDown(window, { key: 'f', repeat: true });
    fireEvent.keyUp(window, { key: 'f' });
    fireEvent.keyDown(window, { key: 'z', repeat: true });

    // Assert: the repeat never zooms, sounds one note, or shifts the octave.
    expect(zoom.textContent).toBe(before);
    expect(computerKeyboard).toHaveTextContent('C4');
    expect(sendMidi.mock.calls.filter(([, bytes]) => bytes[0] === 0x90)).toHaveLength(1);
  });

  it('never arms live input on a track that cannot play', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push(
      {
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id: 'track:audio',
        name: 'Audio Track',
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
        id: 'track:live',
        name: 'Live Keys',
        kind: 'instrument',
        gainDb: 0,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        monitoring: 'off',
        midiInput: {},
        instrument: {
          id: 'device:live',
          name: 'Fake Synth',
          bypassed: false,
          midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
          source: {
            type: 'vst3',
            path: 'C:\\Plugins\\FakeSynth.vst3',
            parameterValues: [],
            disabledPlaceholder: false,
          },
        },
        effects: [],
      },
    );
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    render(<Harness api={api} initialSession={session} />);
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });

    // Act: select an Audio Track, which cannot receive performance MIDI.
    fireEvent.click(screen.getByText('Audio Track'));

    // Assert: the toggle reports the reason instead of showing itself engaged.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'false');
    expect(computerKeyboard).toHaveAttribute(
      'title',
      expect.stringContaining('No Track is focused.'),
    );

    // Act: ask for live input there, then focus an Instrument Track.
    fireEvent.click(computerKeyboard);
    fireEvent.click(screen.getByText('Live Keys'));

    // Assert: the Instrument Track does not inherit an input the user never armed.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'false');
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });
    expect(sendMidi).not.toHaveBeenCalled();

    // Act: arm it on the Instrument Track, then move through a Track that cannot play.
    fireEvent.click(computerKeyboard);
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByText('Audio Track'));

    // Assert: leaving a playable Track releases live input, label included.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'false');
    expect(computerKeyboard).not.toHaveTextContent('C4');

    // Act: return to the Instrument Track.
    fireEvent.click(screen.getByText('Live Keys'));

    // Assert: live input stayed off instead of coming back on its own.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'false');
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });
    expect(sendMidi).not.toHaveBeenCalled();
  });

  it('keeps live input armed while moving between playable tracks', async () => {
    // Arrange
    const session = defaultSession();
    const playable = (id: string, name: string) => ({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id,
      name,
      kind: 'instrument' as const,
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off' as const,
      midiInput: {},
      instrument: {
        id: `device:${id}`,
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3' as const,
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.tracks.push(playable('track:a', 'Keys A'), playable('track:b', 'Keys B'));
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    render(<Harness api={api} initialSession={session} />);
    fireEvent.click(screen.getByText('Keys A'));
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });
    fireEvent.click(computerKeyboard);

    // Act: move to the other Instrument Track.
    fireEvent.click(screen.getByText('Keys B'));
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });

    // Assert: the input the user turned on followed the focus.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'true');
    await waitFor(() => expect(sendMidi).toHaveBeenCalledWith('track:b', [0x90, 60, 100]));
  });

  it('toggles live input once for a held computer keyboard shortcut', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:live',
      name: 'Live Keys',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:live',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    render(<Harness api={api} initialSession={session} />);
    fireEvent.click(screen.getByText('Live Keys'));
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });

    // Act: hold Ctrl+Shift+K.
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true, shiftKey: true });
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true, shiftKey: true, repeat: true });
    fireEvent.keyUp(window, { key: 'k', ctrlKey: true, shiftKey: true });

    // Assert: the press turned it on and the repeats did not turn it back off.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'true');
  });

  it('routes Space to the shared transport controller from the Arrange editor', async () => {
    const api = new FakeNativeApi();
    let toggles = 0;
    render(<Harness api={api} onToggleTransport={() => (toggles += 1)} />);
    const workspace = screen.getByLabelText('Arrange timeline');

    fireEvent.keyDown(workspace, { key: ' ' });

    expect(toggles).toBe(1);
  });
});
