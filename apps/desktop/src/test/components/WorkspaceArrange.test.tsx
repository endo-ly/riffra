// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { WorkspaceArrange } from '@/components';
import {
  defaultSession,
  toAssetId,
  type CreativeSession,
  type TransportStatus,
} from '@/lib/domain';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';

afterEach(cleanup);

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

    fireEvent.contextMenu(editor.querySelector('[data-note-id="note:high"]')!, {
      clientX: 120,
      clientY: 80,
    });

    expect(api.calls).not.toContain('removeMidiNote');
    expect(screen.getByRole('menuitem', { name: 'Delete' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Duplicate' }));
    expect(api.calls).toContain('duplicateMidiNotes');
  });

  it('keeps a MIDI note at its preview position until the canonical update arrives', async () => {
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
      void originalUpdateMidiNote(...args);
      return new Promise<CreativeSession>((resolve) => {
        releaseUpdate = () => resolve(session);
      });
    };
    const { container } = render(<Harness api={api} initialSession={session} />);

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
    expect(container.querySelector('[class*="statusToast_"]')).not.toBeInTheDocument();

    releaseUpdate();
    await waitFor(() => expect(Number.parseFloat(note.style.left)).toBeCloseTo(43.2));
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

    fireEvent.change(screen.getByLabelText('Add track'), { target: { value: 'audio' } });
    fireEvent.click(await screen.findByLabelText('Audio 1 track menu'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete Track' }));

    await waitFor(() => expect(api.calls).toContain('removeTrack'));
    expect(screen.queryByText(/Source Audio Assets will be kept/)).not.toBeInTheDocument();
    expect(screen.queryByText('Audio 1')).not.toBeInTheDocument();
  });

  it('uses the latest pending value when Track controls are clicked rapidly', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    render(<Harness api={api} />);

    fireEvent.change(screen.getByLabelText('Add track'), { target: { value: 'audio' } });
    const mute = await screen.findByRole('button', { name: 'Mute Audio 1' });
    const solo = screen.getByRole('button', { name: 'Solo Audio 1' });

    fireEvent.click(mute);
    fireEvent.click(mute);
    fireEvent.click(mute);
    fireEvent.click(solo);
    fireEvent.click(solo);

    await waitFor(() => expect(mute).toHaveAttribute('aria-pressed', 'true'));
    expect(solo).toHaveAttribute('aria-pressed', 'false');
    expect(api.calls.filter((call) => call === 'updateTrack')).toHaveLength(5);
  });

  it('edits Track Automation with one Session commit per gesture', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    render(<Harness api={api} />);

    fireEvent.change(screen.getByLabelText('Add track'), { target: { value: 'audio' } });
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
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Disable loop' })).not.toBeInTheDocument(),
    );
  });

  it('disables the loop from the band close button', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    const closeButton = await screen.findByRole('button', { name: 'Disable loop' });
    fireEvent.click(closeButton);

    await waitFor(() => expect(api.calls).toContain('updateTimelineLoopRange'));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Disable loop' })).not.toBeInTheDocument(),
    );
  });

  it('clears the punch range from the band close button without a time selection', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    expect(screen.queryByText(/Selection/)).not.toBeInTheDocument();
    const closeButton = await screen.findByRole('button', { name: 'Clear punch range' });
    fireEvent.click(closeButton);

    await waitFor(() => expect(api.calls).toContain('updateTimelinePunchRange'));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Clear punch range' })).not.toBeInTheDocument(),
    );
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
    const punchRange = screen.getByRole('button', { name: 'Clear punch range' }).parentElement!;
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

    fireEvent.change(screen.getByLabelText('Add track'), { target: { value: 'audio' } });
    fireEvent.click(await screen.findByLabelText('Audio 1 track menu'));

    expect(screen.getByText('Delete')).toBeInTheDocument();

    fireEvent.click(document.body);

    await waitFor(() => expect(screen.getByText('Delete')).not.toBeVisible());
  });
});
