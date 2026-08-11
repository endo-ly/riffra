// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { WorkspaceArrange } from '@/components';
import {
  defaultSession,
  toAssetId,
  type CreativeSession,
  type Track,
  type TransportStatus,
} from '@/lib/domain';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';
import { ToastStack } from '@/components/shared/ToastStack';

afterEach(() => {
  cleanup();
});

function Harness({
  api,
  initialSession,
}: {
  api: FakeNativeApi;
  initialSession?: CreativeSession;
}) {
  const initial = initialSession ?? defaultSession();
  initial.workspace = 'arrange';
  const [session, setSession] = useState<CreativeSession>(initial);
  const [selection, setSelection] = useState<ArrangeSelection>({ kind: 'none' });
  return (
    <>
      <WorkspaceArrange
        session={session}
        setSession={setSession}
        selection={selection}
        setSelection={setSelection}
        api={api}
        audio={api.audio}
        focusedTrackId={null}
        onFocusTrack={() => undefined}
      />
      <ToastStack />
    </>
  );
}

describe('WorkspaceArrange', () => {
  it('creates the first audio track when an audio Asset is dropped on an empty timeline', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    const { container } = render(<Harness api={api} />);
    const empty = screen.getByText('Start arranging').parentElement!;
    const timeline = empty.closest('[class*="timeline"]')!;
    Object.defineProperty(timeline, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 800, top: 0, bottom: 180, right: 800, height: 180 }),
    });

    fireEvent.drop(empty, {
      clientX: 172,
      dataTransfer: {
        getData: () =>
          JSON.stringify({
            version: 1,
            assetId: toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a'),
            name: 'Take',
            kind: 'audio',
          }),
      },
    });

    await waitFor(() => expect(api.calls).toContain('addAudioClipToArrangement'));
    expect(await screen.findByText('Audio 1')).toBeInTheDocument();
    const clipName = await screen.findByText('Take');
    await waitFor(() => expect(container.querySelector('svg')).toBeInTheDocument());
    const clip = clipName.closest('button')!;
    fireEvent.click(clip);
    expect(clip).toHaveAttribute('aria-pressed', 'true');
    fireEvent.keyDown(window, { key: 'd', ctrlKey: true });
    await waitFor(() => expect(api.calls).toContain('pasteTimelineClips'));
    expect(await screen.findByText('Take copy')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'c', ctrlKey: true });
    fireEvent.keyDown(window, { key: 'v', ctrlKey: true });
    await waitFor(() => expect(api.calls).toContain('pasteTimelineClips'));
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

  it('opens the MIDI Editor and adds a note from an empty piano-roll cell', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
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
      id: 'clip:midi',
      name: 'MIDI Clip',
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

    const clip = container.querySelector('[data-clip-id="clip:midi"]')!;
    fireEvent.doubleClick(clip);

    const editor = await screen.findByLabelText('MIDI Editor');
    const lane = editor.querySelector('div[class*="laneViewport"] div[class*="lane_"]')!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 0, top: 0, right: 400, bottom: 864, width: 400, height: 864 }),
    });
    fireEvent.pointerDown(lane, { clientX: 30, clientY: 36 });
    fireEvent.pointerUp(window, { clientX: 30, clientY: 36 });

    await waitFor(() => expect(api.calls).toContain('addMidiNote'));
    expect(api.bootstrapState.session.arrangement.midiClips[0]?.notes).toHaveLength(1);
    expect(api.bootstrapState.session.arrangement.midiClips[0]?.notes[0]?.startTick).toBe(240);
  });

  it('keeps the full MIDI pitch range and uses a context menu for note deletion', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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

  it('reports grid-aligned notes without sending a quantize operation', async () => {
    // Arrange
    const session = defaultSession();
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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
  });

  it('deletes an empty Audio Track from its Track Header', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: '＋ Add Audio Track' }));
    fireEvent.click(await screen.findByLabelText('Audio 1 track menu'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete Track' }));

    await waitFor(() => expect(api.calls).toContain('removeTrack'));
    expect(screen.queryByText(/Source Audio Assets will be kept/)).not.toBeInTheDocument();
    expect(screen.queryByText('Audio 1')).not.toBeInTheDocument();
  });

  it('reports MIDI and Audio clips when confirming Track deletion', () => {
    // Arrange
    const session = defaultSession();
    session.workspace = 'arrange';
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

  it('uses the latest pending value when Track controls are clicked rapidly', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: '＋ Add Audio Track' }));
    const mute = await screen.findByRole('button', { name: 'Mute Audio 1' });
    const solo = screen.getByRole('button', { name: 'Solo Audio 1' });

    fireEvent.click(mute);
    fireEvent.click(mute);
    fireEvent.click(mute);
    fireEvent.click(solo);
    fireEvent.click(solo);

    expect(
      screen.getByRole('button', { name: 'Cycle input monitoring for Audio 1' }),
    ).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: 'Audio 1 gain' })).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: 'Audio 1 pan' })).toBeInTheDocument();
    await waitFor(() => expect(mute).toHaveAttribute('aria-pressed', 'true'));
    expect(solo).toHaveAttribute('aria-pressed', 'false');
    expect(api.calls.filter((call) => call === 'updateTrack')).toHaveLength(5);
  });

  it('edits Track Automation with one Session commit per gesture', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: '＋ Add Audio Track' }));
    fireEvent.click(await screen.findByText('Audio 1'));
    fireEvent.click(screen.getByRole('button', { name: 'Automation' }));
    const lane = screen.getByLabelText('Audio 1 volume automation');
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 1200, top: 0, bottom: 84, right: 1200, height: 84 }),
    });

    fireEvent.pointerDown(lane, { button: 0, clientX: 120, clientY: 42 });
    await waitFor(() => expect(api.calls).toContain('setTrackAutomation'));
    const point = screen.getByRole('button', { name: /volume .* at tick/ });
    const commitsBeforeDrag = api.calls.filter((call) => call === 'setTrackAutomation').length;
    fireEvent.pointerDown(point, { button: 0, clientX: 120, clientY: 42 });
    fireEvent.pointerMove(window, { clientX: 180, clientY: 24 });
    expect(api.calls.filter((call) => call === 'setTrackAutomation')).toHaveLength(
      commitsBeforeDrag,
    );
    fireEvent.pointerUp(window, { clientX: 180, clientY: 24 });
    await waitFor(() =>
      expect(api.calls.filter((call) => call === 'setTrackAutomation')).toHaveLength(
        commitsBeforeDrag + 1,
      ),
    );
  });

  it('keeps an unavailable clip on the timeline and labels its missing source', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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

  it('places overlapping Audio and MIDI clips on separate lanes', () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    // This is an intentionally invalid legacy snapshot: the renderer must not
    // overlay mixed timeline items even before the domain repair is applied.
    session.arrangement.tracks.push({
      id: 'track:audio-midi',
      name: 'Audio and MIDI',
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
      id: 'clip:audio-overlap',
      name: 'Audio overlap',
      trackId: 'track:audio-midi',
      assetId: toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a'),
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
    session.arrangement.midiClips.push({
      id: 'clip:midi-overlap',
      name: 'MIDI overlap',
      trackId: 'track:audio-midi',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });

    const api = new FakeNativeApi({ bootstrapState: { session } });
    const { container } = render(<Harness api={api} initialSession={session} />);
    const audioClip = container.querySelector<HTMLElement>('[data-clip-id="clip:audio-overlap"]');
    const midiClip = container.querySelector<HTMLElement>('[data-clip-id="clip:midi-overlap"]');

    expect(audioClip).toBeInTheDocument();
    expect(midiClip).toBeInTheDocument();
    expect(audioClip?.style.top).not.toBe(midiClip?.style.top);
  });

  it('renders one shared grid for a long timeline regardless of Track count', () => {
    // Arrange
    const session = defaultSession();
    session.workspace = 'arrange';
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

  it('rejects placing a clip on a Track with the wrong source kind', async () => {
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
    const api = new FakeNativeApi({ bootstrapState: { session } });

    await expect(
      api.addMidiClipToArrangement(toAssetId('asset:midi'), 'MIDI', 0, 'track:audio'),
    ).rejects.toThrow('Instrument Track');
    await expect(
      api.addAudioClipToArrangement(toAssetId('asset:audio'), 'Audio', 0, 'track:instrument'),
    ).rejects.toThrow('Audio Track');
    expect(api.bootstrapState.session.arrangement.audioClips).toHaveLength(0);
    expect(api.bootstrapState.session.arrangement.midiClips).toHaveLength(0);
  });

  it('blocks a MIDI Asset drop on an Audio Track before invoking native API', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    expect(screen.getByRole('slider', { name: 'Loop start' })).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: 'Loop end' })).toBeInTheDocument();
  });

  it('renders draggable handles on the punch range band', () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    expect(screen.getByText('PUNCH')).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: 'Punch start' })).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: 'Punch end' })).toBeInTheDocument();
  });

  it('drags the loop start handle to update the loop range', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
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
    session.workspace = 'arrange';
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
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
    session.workspace = 'arrange';
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
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
    session.workspace = 'arrange';
    session.arrangement.markers.push({ id: 'marker:verse', name: 'Verse', tick: 0 });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    const marker = await screen.findByText('Verse');
    fireEvent.pointerDown(marker.closest('[data-marker-id]')!, { clientX: 0 });
    fireEvent.keyDown(window, { key: 'Delete' });

    await waitFor(() => expect(api.calls).toContain('removeMarker'));
    await waitFor(() => expect(screen.queryByText('Verse')).not.toBeInTheDocument());
  });

  it('adds a Marker at the playhead with the M key', async () => {
    // Arrange
    const api = new FakeNativeApi();
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
    expect(api.bootstrapState.session.arrangement.markers[0]?.tick).toBe(960);
    expect(api.bootstrapState.session.arrangement.markers[0]?.name).toBe('Marker 1');

    // Act
    await screen.findByText('Marker 1');
    fireEvent.keyDown(window, { key: 'm' });

    // Assert
    await waitFor(() =>
      expect(api.bootstrapState.session.arrangement.markers[1]?.name).toBe('Marker 2'),
    );
    expect(api.bootstrapState.session.arrangement.markers[1]?.tick).toBe(960);
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
    const zoom = screen.getByLabelText('Timeline zoom') as HTMLInputElement;

    // Act
    fireEvent.pointerDown(ruler, { clientX: 96 });
    fireEvent.pointerMove(window, { clientX: 124 });
    fireEvent.pointerUp(window, { clientX: 124 });
    expect(screen.getByText('Set Loop')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'z' });

    // Assert: 960..1260 ticks fitted into 1184 usable px needs zoom > 4, so it clamps to 4.
    expect(zoom.value).toBe('4');
  });

  it('does not zoom with the Z key without a time selection', () => {
    // Arrange
    const api = new FakeNativeApi();
    const { container } = render(<Harness api={api} />);
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    const zoom = screen.getByLabelText('Timeline zoom') as HTMLInputElement;

    // Act
    fireEvent.keyDown(window, { key: 'z' });

    // Assert
    expect(zoom.value).toBe('1');
  });

  it('fits all Clips into view with the F key', () => {
    // Arrange
    const session = defaultSession();
    session.workspace = 'arrange';
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
    const zoom = screen.getByLabelText('Timeline zoom') as HTMLInputElement;

    // Act
    fireEvent.keyDown(window, { key: 'f' });

    // Assert: 0..3840 ticks fitted into 1184 usable px -> zoom = 1184/3840 * 10 = 3.083...
    expect(Number(zoom.value)).toBeCloseTo(3.083, 3);
  });

  it('does not zoom with the F key when no Clip exists', () => {
    // Arrange
    const api = new FakeNativeApi();
    const { container } = render(<Harness api={api} />);
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    const zoom = screen.getByLabelText('Timeline zoom') as HTMLInputElement;

    // Act
    fireEvent.keyDown(window, { key: 'f' });

    // Assert
    expect(zoom.value).toBe('1');
  });

  it('deletes a marker from its context menu without a success popup', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
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
    session.workspace = 'arrange';
    session.arrangement.markers.push({ id: 'marker:intro', name: 'Intro', tick: 0 });
    const api = new FakeNativeApi({ bootstrapState: { session } });
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
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: '＋ Add Audio Track' }));
    fireEvent.click(await screen.findByLabelText('Audio 1 track menu'));

    expect(screen.getByText('Delete')).toBeInTheDocument();

    fireEvent.click(document.body);

    await waitFor(() => expect(screen.getByText('Delete')).not.toBeVisible());
  });
});
