// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CreativeSession } from '@/model/generated';
import { TRACK_COLOR_PALETTE } from '@/model/track-color';
import type { MidiClip } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { MidiEditorPanel } from '@/features/midi-editor/MidiEditorPanel';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { renderWithSession } from '@/test/support/session-provider';
import { renderWorkspace } from '@/test/support/workspace';

afterEach(cleanup);

function setup(startTick: number) {
  const clip: MidiClip = {
    instrumentControlEvents: [],
    id: 'clip:1',
    name: 'Clip',
    trackId: 'track:1',
    startTick,
    durationTicks: 3840,
    notes: [
      { id: 'note:1', note: 60, startTick: 100, durationTicks: 200, velocity: 96, channel: 1 },
    ],
    events: [],
    muted: false,
    loopEnabled: false,
  };
  const session = defaultSession();
  session.arrangement.tracks.push({
    id: 'track:1',
    name: 'Keys',
    kind: 'instrument',
    gainDb: 0,
    pan: 0,
    externalAudioSourceTrackIds: [],
    panLaw: 'equalPower',
    muted: false,
    solo: false,
    armed: false,
    monitoring: 'off',
    midiInput: {},
    effects: [],
  });
  session.arrangement.midiClips = [clip];
  const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
  api.setResponse('getInstrumentMidiSettings', null);
  const addMidiNote = vi.fn(api.addMidiNote);
  const updateMidiNote = vi.fn(api.updateMidiNote);
  const updateMidiNotes = vi.fn(api.updateMidiNotes);
  const insertMidiNotes = vi.fn(api.insertMidiNotes);
  api.addMidiNote = addMidiNote;
  api.updateMidiNote = updateMidiNote;
  api.updateMidiNotes = updateMidiNotes;
  api.insertMidiNotes = insertMidiNotes;
  const { store } = renderWithSession(<MidiEditorPanel clip={clip} trackColor={null} />, { api });
  const lane = document.querySelector<HTMLElement>('[data-midi-lane]')!;
  Object.defineProperty(lane, 'getBoundingClientRect', {
    value: () => ({ left: 0, top: 0, width: 691.2, height: 1536 }),
  });
  return {
    lane,
    addMidiNote,
    updateMidiNote,
    updateMidiNotes,
    insertMidiNotes,
    playhead: store.getState().transport.playhead,
  };
}

function positions(root: Element, selector: string) {
  return [...root.querySelectorAll<HTMLElement>(selector)].map((line) =>
    Number.parseFloat(line.style.left),
  );
}

describe('MidiEditorPanel', () => {
  it.each([1920, 2160])(
    'aligns all grid layers to Arrangement boundaries at clip tick %i',
    (startTick) => {
      // Arrange / Act
      const { lane } = setup(startTick);
      const velocity = document.querySelector('[data-velocity-lane]')!;
      const bar = (3840 - startTick) * 0.18;
      const beats = [2880, 3840, 4800, ...(startTick === 1920 ? [1920] : [5760])]
        .sort((a, b) => a - b)
        .map((tick) => (tick - startTick) * 0.18);

      // Assert
      expect(positions(lane, '[class*="barLine"]')).toEqual([bar]);
      expect(positions(velocity, '[class*="velocityBarLine"]')).toEqual([bar]);
      expect(positions(lane, '[class*="beatLine"]')).toEqual(beats);
      expect(positions(velocity, '[class*="velocityBeatLine"]')).toEqual(beats);
      const ruler = screen.getByLabelText('MIDI editor ruler');
      expect(positions(ruler, 'i:has(strong)').slice(1)).toEqual([bar]);
      fireEvent.change(screen.getByLabelText('Snap'), { target: { value: '1/8t' } });
      const subdivisions = positions(lane, '[data-grid-subdivision]');
      expect(subdivisions[0]).toBeCloseTo((startTick === 2160 ? 80 : 320) * 0.18);
      for (const left of subdivisions) {
        expect((startTick + left / 0.18) / 320).toBeCloseTo(
          Math.round((startTick + left / 0.18) / 320),
        );
      }
    },
  );

  it('selects the notes identified by the committed edit', async () => {
    // Arrange
    const { lane, addMidiNote } = setup(0);
    addMidiNote.mockResolvedValue({
      canonical: canonicalState(defaultSession()),
      createdEntityIds: { midiNotes: ['note:1'] },
      projection: { state: 'notRequired' },
    });

    // Act
    fireEvent.doubleClick(lane, { clientX: 100, clientY: 804 });

    // Assert
    expect(await screen.findByLabelText('Selected MIDI note velocity')).toBeInTheDocument();
  });

  it('snaps creation, drawing, movement and right-edge resizing to the visible beat lines', () => {
    // Arrange
    const { lane, addMidiNote, updateMidiNote, updateMidiNotes, insertMidiNotes, playhead } =
      setup(2160);
    fireEvent.change(screen.getByLabelText('Snap'), { target: { value: '1/4' } });
    const note = lane.querySelector<HTMLElement>('[data-note-id="note:1"]')!;

    // Act / Assert: double-click and Draw use the same boundaries.
    fireEvent.doubleClick(lane, { clientX: 70, clientY: 804 });
    expect(addMidiNote).toHaveBeenLastCalledWith('clip:1', 720, 60, 960, 96, 1);
    fireEvent.click(screen.getByRole('button', { name: 'Draw' }));
    fireEvent.pointerDown(lane, { clientX: 70, clientY: 804, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 320, clientY: 804, pointerId: 1 });
    expect(addMidiNote).toHaveBeenLastCalledWith('clip:1', 720, 60, 960, 96, 1);

    fireEvent.click(screen.getByRole('button', { name: 'Pointer' }));
    fireEvent.pointerDown(note, { clientX: 100, clientY: 804, pointerId: 2 });
    fireEvent.pointerUp(window, { clientX: 170, clientY: 804, pointerId: 2 });
    expect(updateMidiNote).toHaveBeenLastCalledWith(
      'clip:1',
      'note:1',
      expect.objectContaining({ startTick: 720 }),
    );
    const resize = note.querySelector<HTMLElement>('[class*="resizeEnd"]')!;
    fireEvent.pointerDown(resize, { clientX: 100, clientY: 804, pointerId: 3 });
    fireEvent.pointerUp(window, { clientX: 250, clientY: 804, pointerId: 3 });
    expect(updateMidiNote).toHaveBeenLastCalledWith(
      'clip:1',
      'note:1',
      expect.objectContaining({ startTick: 100, durationTicks: 620 }),
    );

    fireEvent.pointerDown(note, { clientX: 100, clientY: 804, pointerId: 4 });
    fireEvent.pointerUp(window, { clientX: 100, clientY: 804, pointerId: 4 });
    fireEvent.click(note);
    fireEvent.keyDown(screen.getByLabelText('MIDI Editor'), { key: 'ArrowRight' });
    expect(updateMidiNotes).toHaveBeenLastCalledWith('clip:1', [
      { noteId: 'note:1', patch: { startTick: 720, note: 60 } },
    ]);
    playhead.current = 2160 + 350;
    fireEvent.keyDown(screen.getByLabelText('MIDI Editor'), { key: 'c', ctrlKey: true });
    fireEvent.keyDown(screen.getByLabelText('MIDI Editor'), { key: 'v', ctrlKey: true });
    expect(insertMidiNotes).toHaveBeenLastCalledWith('clip:1', [
      { pitch: 60, startTick: 720, durationTicks: 200, velocity: 96, channel: 1 },
    ]);
  });

  it('resizes from the start edge, places freely with Alt, copies on Ctrl-drag and reuses the last length', () => {
    // Arrange
    const { lane, addMidiNote, updateMidiNote, insertMidiNotes } = setup(2160);
    fireEvent.change(screen.getByLabelText('Snap'), { target: { value: '1/4' } });
    const note = lane.querySelector<HTMLElement>('[data-note-id="note:1"]')!;
    const resizeStart = note.querySelector<HTMLElement>('[class*="resizeStart"]')!;

    // Act
    fireEvent.pointerDown(resizeStart, { clientX: 18, clientY: 804, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 27, clientY: 804, pointerId: 1, altKey: true });
    fireEvent.doubleClick(lane, { clientX: 70, clientY: 804 });
    fireEvent.pointerDown(note, { clientX: 100, clientY: 804, pointerId: 2, ctrlKey: true });
    fireEvent.pointerUp(window, { clientX: 170, clientY: 804, pointerId: 2 });

    // Assert
    expect(updateMidiNote).toHaveBeenCalledTimes(1);
    expect(updateMidiNote).toHaveBeenLastCalledWith(
      'clip:1',
      'note:1',
      expect.objectContaining({ startTick: 150, durationTicks: 150 }),
    );
    expect(addMidiNote).toHaveBeenLastCalledWith('clip:1', 720, 60, 150, 96, 1);
    expect(insertMidiNotes).toHaveBeenLastCalledWith('clip:1', [
      { pitch: 60, startTick: 720, durationTicks: 200, velocity: 96, channel: 1 },
    ]);
  });

  it('keeps a held preview note sounding while the editor re-renders', () => {
    // Arrange: an Instrument Track that can preview, and a playhead the panel reads.
    const clip: MidiClip = {
      instrumentControlEvents: [],
      id: 'clip:1',
      name: 'Clip',
      trackId: 'track:1',
      startTick: 0,
      durationTicks: 3_840,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    };
    const session = defaultSession();
    session.arrangement.tracks.push({
      id: 'track:1',
      name: 'Keys',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower',
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:1',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:/Plugins/Fake.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.midiClips = [clip];
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.setResponse('getInstrumentMidiSettings', null);
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    const panicMidi = vi.spyOn(api, 'panicMidiTrack');
    const { store } = renderWithSession(<MidiEditorPanel clip={clip} trackColor={null} />, { api });
    const key = screen.getByRole('button', { name: 'Preview C4' });

    // Act: hold the key, then move the playhead so the panel renders again.
    fireEvent.pointerDown(key, { pointerId: 1 });
    act(() => store.getState().transport.seekLocally(480));

    // Assert: the held note is neither released nor panicked by the re-render.
    expect(sendMidi).toHaveBeenCalledTimes(1);
    expect(sendMidi).toHaveBeenLastCalledWith('track:1', [0x90, 60, 96]);
    expect(panicMidi).not.toHaveBeenCalled();

    // Act
    fireEvent.pointerUp(key, { pointerId: 1 });

    // Assert: the release sends the matching note off.
    expect(sendMidi).toHaveBeenLastCalledWith('track:1', [0x80, 60, 0]);
    expect(panicMidi).not.toHaveBeenCalled();
  });

  it('creates a note off the grid with Alt, like every other placement', () => {
    // Arrange
    const { lane, addMidiNote } = setup(2160);
    fireEvent.change(screen.getByLabelText('Snap'), { target: { value: '1/4' } });

    // Act
    fireEvent.doubleClick(lane, { clientX: 70, clientY: 804 });
    fireEvent.doubleClick(lane, { clientX: 100, clientY: 804, altKey: true });

    // Assert: the snapped start lands on 720, the free one keeps its raw tick.
    expect(addMidiNote).toHaveBeenNthCalledWith(1, 'clip:1', 720, 60, 960, 96, 1);
    expect(addMidiNote).toHaveBeenNthCalledWith(2, 'clip:1', 556, 60, 960, 96, 1);
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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = renderWorkspace(api, { session });
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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = renderWorkspace(api, { session });

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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let finishQuantize: ((next: ArrangementMutationResult) => void) | undefined;
    api.quantizeMidiNotes = async (_clipId, _noteIds, _gridTicks) => {
      api.calls.push('quantizeMidiNotes');
      return new Promise<ArrangementMutationResult>((resolve) => {
        finishQuantize = resolve;
      });
    };
    const { container } = renderWorkspace(api, { session });

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
    finishQuantize!(api.mutationResult(session));
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
    const api = createFakeNativeApi({
      bootstrapState: { canonical: canonicalState(session) },
    });
    let updateArgs: Parameters<FakeNativeApi['updateMidiNotes']> | undefined;
    api.updateMidiNotes = async (...args) => {
      updateArgs = args;
      api.calls.push('updateMidiNotes');
      return api.mutationResult(session);
    };
    const { container } = renderWorkspace(api, { session });

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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let addArgs: Parameters<FakeNativeApi['addMidiNote']> | undefined;
    api.addMidiNote = async (...args) => {
      addArgs = args;
      api.calls.push('addMidiNote');
      return api.mutationResult(session);
    };
    const { container } = renderWorkspace(api, { session });
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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sent: { trackId: string; bytes: number[] }[] = [];
    api.sendMidiToTrack = async (trackId, bytes) => {
      api.calls.push('sendMidiToTrack');
      sent.push({ trackId, bytes });
    };
    const { container } = renderWorkspace(api, { session });
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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = renderWorkspace(api, { session });
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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
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
    const { container } = renderWorkspace(api, { session });

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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
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
    const { container } = renderWorkspace(api, { session });

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

    resolveUpdate(api.mutationResult(committedSession));
    await waitFor(() => expect(bar).toHaveAttribute('aria-label', 'C4 velocity 76'));

    const pianoKey = velocityLane.querySelector('[data-piano-key="60"]') as HTMLElement;
    fireEvent.pointerDown(pianoKey, { pointerId: 2 });
    fireEvent.pointerUp(pianoKey, { pointerId: 2 });
    expect(api.calls.filter((call) => call === 'sendMidiToTrack')).toHaveLength(2);
  });
});
