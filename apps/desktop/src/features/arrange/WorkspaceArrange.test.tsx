// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { WorkspaceArrange } from './WorkspaceArrange';
import { type CreativeSession, type Track } from '@/model/domain';
import { defaultSession } from '@/native/browser-defaults';
import { toAssetId, type TransportStatus } from '@/native/contracts';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import { ToastStack } from '@/shared/ui/ToastStack';

afterEach(() => {
  cleanup();
});

function Harness({
  api,
  initialSession,
  onToggleTransport,
}: {
  api: FakeNativeApi;
  initialSession?: CreativeSession;
  onToggleTransport?: () => void;
}) {
  const initial = initialSession ?? defaultSession();
  const [session, setSession] = useState<CreativeSession>(initial);
  const [selection, setSelection] = useState<ArrangeSelection>({ kind: 'none' });
  const [focusedTrackId, setFocusedTrackId] = useState<string | null>(null);
  const [playSurfaceHost, setPlaySurfaceHost] = useState<HTMLDivElement | null>(null);
  return (
    <>
      <WorkspaceArrange
        session={session}
        setSession={setSession}
        selection={selection}
        setSelection={setSelection}
        api={api}
        audio={api.audio}
        focusedTrackId={focusedTrackId}
        onFocusTrack={setFocusedTrackId}
        onToggleTransport={onToggleTransport ?? (() => undefined)}
        playSurfaceHost={playSurfaceHost}
      />
      <div ref={setPlaySurfaceHost} data-play-surface-host />
      <ToastStack />
    </>
  );
}

describe('WorkspaceArrange', () => {
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

  it('keeps the full MIDI pitch range and uses a context menu for note deletion', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:midi-range"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');

    expect(screen.getByLabelText('MIDI piano keyboard')).toBeInTheDocument();
    expect(editor.querySelectorAll('[data-note-id]')).toHaveLength(2);

    const highNote = editor.querySelector('[data-note-id="note:high"]')!;
    fireEvent.click(highNote);
    expect(screen.getByLabelText('Selected MIDI note velocity')).toHaveValue('127');
    fireEvent.keyDown(screen.getByLabelText('Selected MIDI note velocity'), { key: 'Delete' });
    expect(api.calls).not.toContain('removeMidiNote');

    fireEvent.contextMenu(highNote, {
      clientX: 120,
      clientY: 80,
    });

    expect(api.calls).not.toContain('removeMidiNote');
    expect(screen.getByRole('menuitem', { name: 'Delete' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Duplicate' }));
    expect(api.calls).toContain('duplicateMidiNotes');
  });

  it('quantizes an off-grid note from the MIDI editor and reports the grid', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
      id: 'clip:quantize',
      name: 'Quantize',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 1_920,
      notes: [
        {
          id: 'note:off-grid',
          note: 60,
          startTick: 100,
          durationTicks: 240,
          velocity: 96,
          channel: 0,
        },
      ],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    let finishQuantize: ((next: CreativeSession) => void) | undefined;
    api.quantizeMidiNotes = async (_clipId, _noteIds, _gridTicks) => {
      api.calls.push('quantizeMidiNotes');
      return new Promise<CreativeSession>((resolve) => {
        finishQuantize = resolve;
      });
    };
    const { container } = render(<Harness api={api} initialSession={session} />);

    // Act: open the MIDI Editor, select the note, and hit Quantize.
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:quantize"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    const note = editor.querySelector('[data-note-id="note:off-grid"]')! as HTMLElement;
    fireEvent.click(note);
    fireEvent.click(within(editor).getByRole('button', { name: 'Quantize' }));

    // Assert
    await waitFor(() => expect(finishQuantize).toBeDefined());
    expect(screen.queryByText('Quantized 1 note to 1/16.')).not.toBeInTheDocument();
    finishQuantize!(session);
    await screen.findByText('Quantized 1 note to 1/16.');
    await waitFor(() => expect(api.calls).toContain('quantizeMidiNotes'));
  });

  it('keeps MIDI editor shortcuts inside the focused editor', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:shortcuts"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    fireEvent.keyDown(editor, { key: 'a', ctrlKey: true });

    expect(
      [...editor.querySelectorAll('[data-note-id]')].every((note) =>
        note.className.includes('selected'),
      ),
    ).toBe(true);

    fireEvent.keyDown(editor, { key: 'Delete' });

    expect(api.calls).toContain('removeMidiNotes');
    expect(api.calls).not.toContain('removeTimelineClips');
  });

  it('creates an empty MIDI clip from an instrument lane and opens its editor', async () => {
    const session = defaultSession();
    const track = {
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
      rack: { devices: [], macros: [] },
    };
    session.arrangement.tracks.push(track);
    const createdSession: CreativeSession = {
      ...session,
      arrangement: {
        ...session.arrangement,
        midiClips: [
          {
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    let createArgs: Parameters<FakeNativeApi['createMidiClip']> | undefined;
    api.createMidiClip = async (...args) => {
      createArgs = args;
      api.calls.push('createMidiClip');
      return createdSession;
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
      rack: { devices: [], macros: [] },
    };
    session.arrangement.tracks.push(track);
    const createdSession: CreativeSession = {
      ...session,
      arrangement: {
        ...session.arrangement,
        midiClips: [
          {
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    let createArgs: Parameters<FakeNativeApi['createMidiClip']> | undefined;
    api.createMidiClip = async (...args) => {
      createArgs = args;
      api.calls.push('createMidiClip');
      return createdSession;
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

  it('moves selected MIDI notes by semitones and octaves with the arrow shortcuts', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      id: 'track:instrument-arrows',
      name: 'Instrument Arrows',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
      id: 'clip:arrows',
      name: 'Arrows',
      trackId: 'track:instrument-arrows',
      startTick: 0,
      durationTicks: 1_920,
      notes: [
        {
          id: 'note:arrow-a',
          note: 60,
          startTick: 0,
          durationTicks: 240,
          velocity: 96,
          channel: 0,
        },
        {
          id: 'note:arrow-b',
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    let updateArgs: Parameters<FakeNativeApi['updateMidiNotes']> | undefined;
    api.updateMidiNotes = async (...args) => {
      updateArgs = args;
      api.calls.push('updateMidiNotes');
      return session;
    };
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:arrows"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    fireEvent.click(editor.querySelector('[data-note-id="note:arrow-a"]')!);
    fireEvent.click(editor.querySelector('[data-note-id="note:arrow-b"]')!, { ctrlKey: true });

    fireEvent.keyDown(editor, { key: 'ArrowUp' });
    expect(updateArgs?.[1].map((update) => update.patch.note)).toEqual([61, 65]);

    fireEvent.keyDown(editor, { key: 'ArrowDown', shiftKey: true });
    expect(updateArgs?.[1].map((update) => update.patch.note)).toEqual([48, 52]);
  });

  it('keeps the active MIDI Clip while selecting Audio or appending another MIDI Clip', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push(
      {
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
        rack: { devices: [], macros: [] },
      },
      {
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
        rack: { devices: [], macros: [] },
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
      takeVariant: 'raw',
    });
    session.arrangement.midiClips.push(
      {
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    let addArgs: Parameters<FakeNativeApi['addMidiNote']> | undefined;
    api.addMidiNote = async (...args) => {
      addArgs = args;
      api.calls.push('addMidiNote');
      return session;
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

  it('renders subdivision lines for the selected MIDI Editor grid', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      id: 'track:grid',
      name: 'Grid Track',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
      id: 'clip:grid',
      name: 'Grid',
      trackId: 'track:grid',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:grid"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    expect(editor.querySelector('[data-midi-pitch-viewport] [data-midi-lane]')).toBeInTheDocument();
    expect(editor.querySelector('[data-midi-pitch-viewport] [data-velocity-lane]')).toBeNull();
    expect(
      editor.querySelector('[data-midi-velocity-viewport] [data-velocity-lane]'),
    ).toBeInTheDocument();
    expect(editor.querySelectorAll('[data-grid-subdivision]')).toHaveLength(6);

    fireEvent.change(within(editor).getByRole('combobox'), { target: { value: '1/8' } });
    expect(editor.querySelectorAll('[data-grid-subdivision]')).toHaveLength(2);
  });

  it('reports grid-aligned notes without sending a quantize operation', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
      id: 'clip:quantize-aligned',
      name: 'Quantize aligned',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 1_920,
      notes: [
        {
          id: 'note:on-grid',
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
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    // Act
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:quantize-aligned"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    fireEvent.click(editor.querySelector('[data-note-id="note:on-grid"]')!);
    fireEvent.click(within(editor).getByRole('button', { name: 'Quantize' }));

    // Assert
    await screen.findByText('Selected notes are already on the grid.');
    expect(api.calls).not.toContain('quantizeMidiNotes');
  });

  it('clears a MIDI preview when the canonical response uses an effective value', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
      id: 'clip:midi-move',
      name: 'MIDI Move',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 1_920,
      notes: [
        { id: 'note:move', note: 60, startTick: 0, durationTicks: 240, velocity: 96, channel: 0 },
      ],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const originalUpdateMidiNote = api.updateMidiNote.bind(api);
    let releaseUpdate!: () => void;
    api.updateMidiNote = (...args) => {
      const canonical = originalUpdateMidiNote(...args);
      return new Promise<CreativeSession>((resolve) => {
        releaseUpdate = () => {
          void canonical.then((next) =>
            resolve({
              ...next,
              arrangement: {
                ...next.arrangement,
                midiClips: next.arrangement.midiClips.map((clip) =>
                  clip.id === 'clip:midi-move'
                    ? {
                        ...clip,
                        notes: clip.notes.map((note) =>
                          note.id === 'note:move' ? { ...note, startTick: 120 } : note,
                        ),
                      }
                    : clip,
                ),
              },
            }),
          );
        };
      });
    };
    const { container } = render(<Harness api={api} initialSession={session} />);
    api.emitTransportStatus({ revision: session.arrangement.revision });

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:midi-move"]')!);
    const editor = await screen.findByLabelText('MIDI Editor');
    const lane = editor.querySelector('div[class*="laneViewport"] div[class*="lane_"]')!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 0, top: 0, right: 400, bottom: 1_536, width: 400, height: 1_536 }),
    });
    const note = editor.querySelector('[data-note-id="note:move"]')! as HTMLElement;
    fireEvent.pointerDown(note, { clientX: 100, clientY: 804, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 148, clientY: 804, pointerId: 1 });
    expect(Number.parseFloat(note.style.left)).toBeCloseTo(43.2);
    expect(note.style.top).toBe('804px');

    fireEvent.pointerUp(window, { clientX: 148, clientY: 804, pointerId: 1 });
    expect(Number.parseFloat(note.style.left)).toBeCloseTo(43.2);
    expect(note.style.top).toBe('804px');
    expect(container.querySelector('[class*="toast_"]')).not.toBeInTheDocument();

    releaseUpdate();
    await waitFor(() => expect(Number.parseFloat(note.style.left)).toBeCloseTo(21.6));
  });

  it('previews a selected MIDI note group until the canonical update arrives', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const originalUpdateMidiNotes = api.updateMidiNotes.bind(api);
    let releaseUpdate!: () => void;
    api.updateMidiNotes = (...args) => {
      const canonical = originalUpdateMidiNotes(...args);
      return new Promise<CreativeSession>((resolve) => {
        releaseUpdate = () => {
          void canonical.then(resolve);
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

    releaseUpdate();
    await waitFor(() => expect(Number.parseFloat(second.style.left)).toBeCloseTo(129.6));
  });

  it('renders MIDI keyboard white keys beneath narrower black keys', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
      id: 'clip:midi-keyboard',
      name: 'MIDI Keyboard',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:midi-keyboard"]')!);
    const keyboard = (await screen.findByLabelText('MIDI piano keyboard')) as HTMLElement;

    expect(keyboard.querySelectorAll('[class*="pianoKey_"]')).toHaveLength(128);
    expect(keyboard.querySelectorAll('[class*="pianoWhiteKey_"]')).toHaveLength(128);
    expect(keyboard.querySelectorAll('[class*="pianoBlackKey_"]')).toHaveLength(53);
    expect(
      Array.from(keyboard.querySelectorAll('[class*="pianoBlackKey_"]')).every((key) =>
        Boolean(key.parentElement?.querySelector('[class*="pianoWhiteKey_"]')),
      ),
    ).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: 'MIDI Editor pitch zoom in' }));

    expect((keyboard.querySelector('[data-piano-key="60"]') as HTMLElement).style.height).toBe(
      '14px',
    );
    expect(
      (keyboard.querySelector('[data-piano-key="61"] [class*="pianoBlackKey_"]') as HTMLElement)
        .style.height,
    ).toBe('11px');
  });

  it('reports MIDI and Audio clips when confirming Track deletion', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    // Act
    fireEvent.click(screen.getByLabelText('Instrument Delete track menu'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    // Assert
    expect(screen.getByText(/This also removes 1 Clip from the Timeline/)).toBeInTheDocument();
    expect(screen.getByText(/Source assets will be kept\./)).toBeInTheDocument();
  });

  it('keeps an unavailable clip on the timeline and labels its missing source', async () => {
    const session = defaultSession();
    const assetId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a');
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
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
      takeVariant: 'raw',
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const status: TransportStatus = {
      type: 'transportStatus',
      state: 'stopped',
      revision: session.arrangement.revision,
      timelineTick: 0,
      timelineSample: 0,
      audioClockSample: 0,
      sampleRate: 48_000,
      sequence: 1,
      recordingPhase: 'idle',
      recordingStartTick: 0,
      recordingCurrentTick: 0,
      recordingPassOrdinal: 0,
      armedTrackIds: [],
      clockGeneration: 1,
      discontinuity: 1,
      unavailableClipIds: ['clip:missing'],
      missingDeviceIds: [],
    };
    api.onTransportStatus = (callback) => {
      queueMicrotask(() => callback(status));
      return () => undefined;
    };

    render(<Harness api={api} initialSession={session} />);

    expect(await screen.findByText('Lost Take')).toBeInTheDocument();
    expect((await screen.findAllByText('MISSING SOURCE')).length).toBeGreaterThan(0);
    expect(document.querySelector('[data-clip-id="clip:missing"]')).toBeInTheDocument();
  });

  it('reports a persistent transport revision mismatch after its grace period', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.revision = 1;
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);
    await waitFor(() => expect(api.calls).toContain('onTransportStatus'));

    // Act
    api.emitTransportStatus({ revision: 0 });

    // Assert
    await waitFor(
      () => expect(screen.getByText('Playback runtime is out of sync')).toBeInTheDocument(),
      { timeout: 2_000 },
    );
  });

  it('renders one shared grid for a long timeline regardless of Track count', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.loopRange.endTick = session.arrangement.timebase.ppq * 4 * 100;
    const tracks: Track[] = Array.from({ length: 50 }, (_, index) => ({
      id: `track:grid-${index}`,
      name: `Grid ${index}`,
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      rack: { devices: [], macros: [] },
    }));
    session.arrangement.tracks.push(...tracks);
    const api = new FakeNativeApi({ bootstrapState: { session } });

    // Act
    const { container } = render(<Harness api={api} initialSession={session} />);

    // Assert
    expect(container.querySelectorAll('[data-arrange-track]')).toHaveLength(50);
    expect(container.querySelectorAll('[data-timeline-grid]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-arrange-track] > [class*="lane_"] > i')).toHaveLength(
      0,
    );
  });

  it('blocks a MIDI Asset drop on an Audio Track before invoking native API', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);
    const track = container.querySelector('[data-arrange-track]')!;

    fireEvent.drop(track, {
      clientX: 200,
      dataTransfer: {
        getData: (type: string) =>
          type === 'application/x-riffra-asset'
            ? JSON.stringify({
                version: 1,
                assetId: 'asset:midi',
                name: 'MIDI',
                kind: 'midi',
              })
            : '',
      },
    });

    await waitFor(() =>
      expect(
        screen.getByText('MIDI Assets can only be placed on an Instrument Track.'),
      ).toBeInTheDocument(),
    );
    expect(api.calls).not.toContain('addMidiClipToArrangement');
  });

  it('does not send an Audio clip move to an Instrument Track', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push(
      {
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
        rack: { devices: [], macros: [] },
      },
      {
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
        rack: { devices: [], macros: [] },
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
      takeVariant: 'raw',
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
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

  it('disables the timeline loop from the ruler context menu', async () => {
    const session = defaultSession();
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    const ruler = screen.getByLabelText('Timeline ruler');
    fireEvent.contextMenu(ruler, { clientX: 200, clientY: 8, button: 2 });
    const clearLoop = await screen.findByRole('menuitem', { name: 'Clear Loop' });
    fireEvent.click(clearLoop);

    await waitFor(() => expect(api.calls).toContain('updateTimelineLoopRange'));
    expect(screen.queryByRole('button', { name: 'Clear loop' })).not.toBeInTheDocument();
  });

  it('clears an active loop from the ruler band', async () => {
    const session = defaultSession();
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    const loopRange = screen.getByText('LOOP').parentElement!;
    fireEvent.pointerDown(loopRange, { clientX: 100 });
    expect(loopRange).toHaveAttribute('data-range-selected', 'true');
    fireEvent.keyDown(window, { key: 'Delete' });

    await waitFor(() => expect(api.calls).toContain('updateTimelineLoopRange'));
  });

  it('clears an active punch range from the ruler band without a time selection', async () => {
    const session = defaultSession();
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    expect(screen.queryByText(/Selection/)).not.toBeInTheDocument();
    const punchRange = screen.getByText('PUNCH').parentElement!;
    fireEvent.pointerDown(punchRange, { clientX: 100 });
    expect(punchRange).toHaveAttribute('data-range-selected', 'true');
    fireEvent.keyDown(window, { key: 'Delete' });

    await waitFor(() => expect(api.calls).toContain('updateTimelinePunchRange'));
  });

  it('deletes a loop or punch range from the range context menu', async () => {
    const session = defaultSession();
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    fireEvent.contextMenu(screen.getByText('LOOP').parentElement!, {
      clientX: 100,
      clientY: 8,
      button: 2,
    });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Delete' }));
    await waitFor(() => expect(api.calls).toContain('updateTimelineLoopRange'));

    fireEvent.contextMenu(screen.getByText('PUNCH').parentElement!, {
      clientX: 100,
      clientY: 18,
      button: 2,
    });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Delete' }));
    await waitFor(() => expect(api.calls).toContain('updateTimelinePunchRange'));
  });

  it('renders draggable handles on the loop range band', () => {
    const session = defaultSession();
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    expect(screen.getByRole('slider', { name: 'Loop start' })).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: 'Loop end' })).toBeInTheDocument();
  });

  it('renders draggable handles on the punch range band', () => {
    const session = defaultSession();
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    expect(screen.getByText('PUNCH')).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: 'Punch start' })).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: 'Punch end' })).toBeInTheDocument();
  });

  it('drags the loop start handle to update the loop range', async () => {
    const session = defaultSession();
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const canonical = structuredClone(session);
    canonical.arrangement.loopRange = { enabled: true, startTick: 960, endTick: 3840 };
    const api = new FakeNativeApi({
      bootstrapState: { session },
      responses: { updateTimelineLoopRange: canonical },
    });
    render(<Harness api={api} initialSession={session} />);

    const startHandle = screen.getByRole('slider', { name: 'Loop start' });
    fireEvent.pointerDown(startHandle, { clientX: 0 });
    fireEvent.pointerMove(window, { clientX: 100 });
    fireEvent.pointerUp(window, { clientX: 100 });

    await waitFor(() => expect(api.calls).toContain('updateTimelineLoopRange'));
    const loopRange = screen.getByText('LOOP').parentElement!;
    await waitFor(() => expect(loopRange).toHaveStyle({ left: '96px' }));
  });

  it('drags the loop end handle to update the loop range', async () => {
    const session = defaultSession();
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const canonical = structuredClone(session);
    canonical.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 2880 };
    const api = new FakeNativeApi({
      bootstrapState: { session },
      responses: { updateTimelineLoopRange: canonical },
    });
    render(<Harness api={api} initialSession={session} />);

    const endHandle = screen.getByRole('slider', { name: 'Loop end' });
    fireEvent.pointerDown(endHandle, { clientX: 0 });
    fireEvent.pointerMove(window, { clientX: -100 });
    fireEvent.pointerUp(window, { clientX: -100 });

    await waitFor(() => expect(api.calls).toContain('updateTimelineLoopRange'));
    const loopRange = screen.getByText('LOOP').parentElement!;
    await waitFor(() => expect(loopRange).toHaveStyle({ width: '288px' }));
  });

  it('drags the punch end handle to update the punch range', async () => {
    const session = defaultSession();
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    const canonical = structuredClone(session);
    canonical.arrangement.punchRange = { startTick: 0, endTick: 2880 };
    const api = new FakeNativeApi({
      bootstrapState: { session },
      responses: { updateTimelinePunchRange: canonical },
    });
    render(<Harness api={api} initialSession={session} />);

    const endHandle = screen.getByRole('slider', { name: 'Punch end' });
    fireEvent.pointerDown(endHandle, { clientX: 0 });
    fireEvent.pointerMove(window, { clientX: 100 });
    fireEvent.pointerUp(window, { clientX: 100 });

    await waitFor(() => expect(api.calls).toContain('updateTimelinePunchRange'));
    const punchRange = screen.getByRole('slider', { name: 'Punch end' }).parentElement!;
    await waitFor(() => expect(punchRange).toHaveStyle({ width: '288px' }));
  });

  it('deletes the selected marker with the Delete key', async () => {
    const session = defaultSession();
    session.arrangement.markers.push({ id: 'marker:verse', name: 'Verse', tick: 0 });
    const canonical = structuredClone(session);
    canonical.arrangement.markers = [];
    const api = new FakeNativeApi({
      bootstrapState: { session },
      responses: { removeMarker: canonical },
    });
    render(<Harness api={api} initialSession={session} />);

    const marker = await screen.findByText('Verse');
    fireEvent.pointerDown(marker.closest('[data-marker-id]')!, { clientX: 0 });
    fireEvent.keyDown(window, { key: 'Delete' });

    await waitFor(() => expect(api.calls).toContain('removeMarker'));
    await waitFor(() => expect(screen.queryByText('Verse')).not.toBeInTheDocument());
  });

  it('adds a Marker at the playhead with the M key', async () => {
    // Arrange
    const canonical = defaultSession();
    canonical.arrangement.markers.push({ id: 'marker:1', name: 'Marker 1', tick: 960 });
    const api = new FakeNativeApi({ responses: { addMarker: canonical } });
    render(<Harness api={api} />);
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 2400, top: 0, bottom: 30, right: 2400, height: 30 }),
    });

    // Act
    fireEvent.pointerDown(ruler, { clientX: 96 });
    fireEvent.keyDown(window, { key: 'm' });

    // Assert
    await waitFor(() => expect(api.calls).toContain('addMarker'));
    expect(await screen.findByText('Marker 1')).toBeInTheDocument();
  });

  it('does not add a Marker with the M key while typing in a text input', () => {
    // Arrange
    const api = new FakeNativeApi();
    render(<Harness api={api} />);
    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();

    // Act
    fireEvent.keyDown(input, { key: 'm' });

    // Assert
    expect(api.calls).not.toContain('addMarker');
    input.remove();
  });

  it('zooms to the ruler time selection with the Z key', () => {
    // Arrange
    const api = new FakeNativeApi();
    const { container } = render(<Harness api={api} />);
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 2400, top: 0, bottom: 30, right: 2400, height: 30 }),
    });
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    const zoom = screen.getByRole('group', { name: 'Timeline zoom' });

    // Act
    fireEvent.pointerDown(ruler, { clientX: 96 });
    fireEvent.pointerMove(window, { clientX: 124 });
    fireEvent.pointerUp(window, { clientX: 124 });
    expect(screen.getByText('Set Loop')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'z' });

    // Assert: 960..1260 ticks fitted into 1184 usable px needs zoom > 4, so it clamps to 4.
    expect(zoom.textContent).toBe('400%');
  });

  it('does not zoom with the Z key without a time selection', () => {
    // Arrange
    const api = new FakeNativeApi();
    const { container } = render(<Harness api={api} />);
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    const zoom = screen.getByRole('group', { name: 'Timeline zoom' });

    // Act
    fireEvent.keyDown(window, { key: 'z' });

    // Assert
    expect(zoom.textContent).toBe('100%');
  });

  it('fits all Clips into view with the F key', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.midiClips.push(
      {
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    const zoom = screen.getByRole('group', { name: 'Timeline zoom' });

    // Act
    fireEvent.keyDown(window, { key: 'f' });

    // Assert: 0..3840 ticks fitted into 1184 usable px -> zoom = 1184/3840 * 10 = 3.083...
    expect(zoom.textContent).toBe('308%');
  });

  it('does not zoom with the F key when no Clip exists', () => {
    // Arrange
    const api = new FakeNativeApi();
    const { container } = render(<Harness api={api} />);
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    const zoom = screen.getByRole('group', { name: 'Timeline zoom' });

    // Act
    fireEvent.keyDown(window, { key: 'f' });

    // Assert
    expect(zoom.textContent).toBe('100%');
  });

  it('deletes a marker from its context menu without a success popup', async () => {
    const session = defaultSession();
    session.arrangement.markers.push({ id: 'marker:chorus', name: 'Chorus', tick: 0 });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    const marker = await screen.findByText('Chorus');
    fireEvent.contextMenu(marker.closest('[data-marker-id]')!, { clientX: 40, clientY: 12 });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Delete' }));

    await waitFor(() => expect(api.calls).toContain('removeMarker'));
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('renames a marker in the Arrange dialog', async () => {
    const session = defaultSession();
    session.arrangement.markers.push({ id: 'marker:intro', name: 'Intro', tick: 0 });
    const canonical = structuredClone(session);
    canonical.arrangement.markers[0].name = 'Verse';
    const api = new FakeNativeApi({
      bootstrapState: { session },
      responses: { updateMarker: canonical },
    });
    render(<Harness api={api} initialSession={session} />);

    const marker = await screen.findByText('Intro');
    fireEvent.doubleClick(marker.closest('[data-marker-id]')!);
    const input = screen.getByLabelText('Name');
    fireEvent.change(input, { target: { value: 'Verse' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(api.calls).toContain('updateMarker'));
    expect(await screen.findByText('Verse')).toBeInTheDocument();
  });

  it('clears the time selection chip when clicking outside the ruler', async () => {
    const api = new FakeNativeApi();
    render(<Harness api={api} />);
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 1472, top: 0, bottom: 30, right: 1472, height: 30 }),
    });

    fireEvent.pointerDown(ruler, { clientX: 100 });
    fireEvent.pointerMove(window, { clientX: 200 });
    fireEvent.pointerUp(window, { clientX: 200 });
    fireEvent.click(ruler);

    expect(screen.getByText('Set Loop')).toBeInTheDocument();

    fireEvent.click(document.body);

    await waitFor(() => expect(screen.queryByText('Set Loop')).not.toBeInTheDocument());
  });

  it('closes the track menu when clicking outside', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      id: 'track:audio',
      name: 'Audio 1',
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      rack: { devices: [], macros: [] },
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    fireEvent.click(await screen.findByLabelText('Audio 1 track menu'));

    expect(screen.getByText('Delete')).toBeInTheDocument();

    fireEvent.click(document.body);

    await waitFor(() => expect(screen.getByText('Delete')).not.toBeVisible());
  });

  it('commits a velocity lane drag once and previews the active instrument', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
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
        kind: 'plugin',
        bypassed: false,
        gainDb: 0,
        parameterValues: [],
        disabledPlaceholder: false,
      },
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
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
    let resolveUpdate!: (next: CreativeSession) => void;
    const pendingUpdate = new Promise<CreativeSession>((resolve) => {
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

    resolveUpdate(committedSession);
    await waitFor(() => expect(bar).toHaveAttribute('aria-label', 'C4 velocity 76'));

    const pianoKey = velocityLane.querySelector('[data-piano-key="60"]') as HTMLElement;
    fireEvent.pointerDown(pianoKey, { pointerId: 2 });
    fireEvent.pointerUp(pianoKey, { pointerId: 2 });
    expect(api.calls.filter((call) => call === 'sendMidiToTrack')).toHaveLength(2);
  });

  it('seeks from the MIDI ruler and supports detail area resize, collapse, maximize, and close', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
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
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:navigation"]')!);
    const ruler = await screen.findByLabelText('MIDI editor ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, top: 0, width: 400, height: 32, right: 400, bottom: 32 }),
    });
    fireEvent.pointerDown(ruler, { clientX: 96 });
    expect(api.calls).toContain('seekTimeline');

    const workspace = screen.getByLabelText('Arrange timeline');
    Object.defineProperty(workspace, 'clientHeight', {
      configurable: true,
      value: 600,
    });
    const resizeHandle = screen.getByRole('button', { name: 'Resize detail area' });
    fireEvent.pointerDown(resizeHandle, { clientY: 500 });
    fireEvent.pointerMove(window, { clientY: -200 });
    fireEvent.pointerUp(window, { clientY: -200 });
    expect(screen.getByRole('region', { name: 'Arrange detail area' })).toHaveStyle(
      '--detail-height: 558px',
    );

    const detailArea = screen.getByRole('region', { name: 'Arrange detail area' });
    fireEvent.click(screen.getByRole('button', { name: 'Collapse detail area' }));
    expect(screen.getByRole('button', { name: 'Restore detail area' })).toBeInTheDocument();
    expect(detailArea.querySelector('[hidden]')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Restore detail area' }));
    expect(detailArea.querySelector('[hidden]')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Maximize detail area' }));
    expect(screen.getByRole('button', { name: 'Restore detail area size' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close detail area' }));
    expect(screen.queryByRole('region', { name: 'Arrange detail area' })).not.toBeInTheDocument();
  });

  it('keeps the Play Surface independent from the MIDI detail area', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      id: 'track:play-surface',
      name: 'Play Surface Instrument',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      rack: { devices: [], macros: [] },
    });
    session.arrangement.midiClips.push({
      id: 'clip:play-surface',
      name: 'Play Surface Clip',
      trackId: 'track:play-surface',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);

    fireEvent.click(screen.getByRole('button', { name: 'Open Play Surface' }));
    expect(screen.getByRole('region', { name: 'Play Surface' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Compact Play Surface' }));
    expect(screen.getByRole('button', { name: 'Expand Play Surface' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Expand Play Surface' }));

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:play-surface"]')!);
    expect(screen.getByRole('region', { name: 'Arrange detail area' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Play Surface' })).toBeInTheDocument();
    expect(
      screen.queryByText('Play Surface Instrument · Play Surface Clip'),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Close detail area' }));
    expect(screen.queryByRole('region', { name: 'Arrange detail area' })).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Play Surface' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close Play Surface' }));
    expect(screen.queryByRole('region', { name: 'Play Surface' })).not.toBeInTheDocument();
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
