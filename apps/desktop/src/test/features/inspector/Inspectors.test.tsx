import { createTestSessionStore, TestSessionProvider } from '@/test/support/session-provider';
import { createSessionStore } from '@/shared/session/store';
import { bindSessionRuntime } from '@/shared/session/runtime';
import type { ReactElement, ReactNode } from 'react';
// @vitest-environment jsdom

import type { EditorSelection } from '@/model/editor-selection';
import type { CanonicalState, CreativeSession } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/model/asset-id';
import {
  createFakeNativeApi,
  type FakeNativeApi,
  fakeAudioStatus,
} from '@/test/support/fake-native-api';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AudioClipInspector } from '@/features/inspector/AudioClipInspector';
import { MidiClipInspector } from '@/features/inspector/MidiClipInspector';
import { TakeInspector } from '@/features/inspector/TakeInspector';
import { TrackInspector } from '@/features/inspector/TrackInspector';

afterEach(cleanup);

function recordingSession(): CreativeSession {
  const session = defaultSession();
  const rawId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a');
  const processedId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41b');
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
  session.arrangement.takes.push({
    id: 'take:1',
    sessionId: 'recording:1',
    passId: 'pass:1',
    trackId: 'track:audio',
    startTick: 0,
    durationTicks: 960,
    sourceStartSample: 0,
    sourceEndSample: 1_000,
    rawAudio: {
      assetId: rawId,
      sourceStartSample: 0,
      sourceEndSample: 1_000,
      tailEndSample: 1_000,
      sampleRate: 48_000,
    },
    processedAudio: {
      assetId: processedId,
      sourceStartSample: 128,
      sourceEndSample: 1_256,
      tailEndSample: 1_256,
      sampleRate: 48_000,
    },
  });
  for (const id of ['clip:a', 'clip:b']) {
    session.arrangement.audioClips.push({
      id,
      name: id,
      trackId: 'track:audio',
      assetId: rawId,
      startTick: 0,
      sourceRange: { start: 0, end: 1_000 },
      sourceSampleRate: 48_000,
      timelineDuration: { frames: 1_000, sampleRate: 48_000 },
      gainDb: 0,
      pan: 0,
      fadeIn: { frames: 0, sampleRate: 48_000 },
      fadeOut: { frames: 0, sampleRate: 48_000 },
      loopEnabled: false,
      muted: false,
      recordingTakeId: 'take:1',
      fadeShape: 'equalPower',
      takeVariant: 'raw',
    });
  }
  session.arrangement.recordingSessions.push({
    id: 'recording:1',
    startTick: 0,
    passIds: ['pass:1'],
    trackSlots: [
      {
        trackId: 'track:audio',
        activeTakeId: 'take:1',
        timelineClipId: 'clip:a',
      },
    ],
  });
  return session;
}

function renderWithSession(api: FakeNativeApi, element: ReactElement, canonical?: CanonicalState) {
  const store = createTestSessionStore(api, {
    readCanonical: canonical ? () => canonical : undefined,
  });
  return render(element, {
    wrapper: ({ children }: { children: ReactNode }) => (
      <TestSessionProvider api={api} store={store}>
        {children}
      </TestSessionProvider>
    ),
  });
}

describe('Arrange Inspectors', () => {
  it('surfaces Track operation failures in the Inspector status', async () => {
    const session = defaultSession();
    const track = {
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument',
      name: 'Keys',
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
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.setTrackMidiInput = vi.fn().mockRejectedValue(new Error('MIDI route failed'));

    renderWithSession(
      api,
      <TrackInspector
        track={track}
        session={session}
        audio={fakeAudioStatus()}
        onOpenDevices={() => undefined}
      />,
    );

    fireEvent.change(screen.getByLabelText('MIDI channel'), { target: { value: '1' } });
    expect(await screen.findByRole('status')).toHaveTextContent('MIDI route failed');
  });

  it('edits Clip position and length for Audio and MIDI Clips', async () => {
    // Arrange
    const session = recordingSession();
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'midi:1',
      name: 'Phrase',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 3_840,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.updateMidiClip = vi.fn().mockResolvedValue({
      canonical: canonicalState(session),
      createdEntityIds: {},
      projection: { state: 'notRequired' },
    });
    api.updateAudioClip = vi.fn().mockResolvedValue({
      canonical: canonicalState(session),
      createdEntityIds: {},
      projection: { state: 'notRequired' },
    });
    const clipProps = {
      session,
      selectedClipIds: ['midi:1'] as string[],
      setSelectedClipIds: () => undefined,
    };
    const audioView = renderWithSession(
      api,
      <AudioClipInspector {...clipProps} selectedClipIds={['clip:a']} />,
    );
    const start = screen.getByLabelText('Start');

    // Act: move the Audio Clip by bars.
    fireEvent.change(start, { target: { value: '2' } });
    fireEvent.blur(start);

    // Assert: the length stays derived from the recorded audio.
    await waitFor(() =>
      expect(api.updateAudioClip).toHaveBeenCalledWith('clip:a', { startTick: 3_840 }),
    );
    expect(screen.getByText('0.0.040')).toBeInTheDocument();

    // Act: commit a new MIDI Clip start and discard a cancelled length edit.
    audioView.rerender(<MidiClipInspector {...clipProps} />);
    const midiStart = screen.getByLabelText('Start');
    const midiLength = screen.getByLabelText('Length');
    fireEvent.change(midiStart, { target: { value: '3.2' } });
    fireEvent.keyDown(midiStart, { key: 'Enter' });
    fireEvent.blur(midiStart);
    fireEvent.change(midiLength, { target: { value: '9' } });
    fireEvent.keyDown(midiLength, { key: 'Escape' });
    fireEvent.blur(midiLength);

    // Assert
    expect(midiStart).toHaveValue('1.1.000');
    expect(midiLength).toHaveValue('1.0.000');
    expect(api.updateMidiClip).toHaveBeenCalledTimes(1);
    expect(api.updateMidiClip).toHaveBeenCalledWith('midi:1', { startTick: 2 * 3_840 + 960 });
  });

  it('toggles MIDI Clip playback and deletes it from the header', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'midi:1',
      name: 'Phrase',
      trackId: 'track:instrument',
      startTick: 0,
      durationTicks: 3_840,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: true,
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.updateMidiClip = vi.fn().mockResolvedValue({
      canonical: canonicalState(session),
      createdEntityIds: {},
      projection: { state: 'notRequired' },
    });
    api.removeTimelineClips = vi.fn().mockResolvedValue({
      canonical: canonicalState(session),
      createdEntityIds: {},
      projection: { state: 'notRequired' },
    });
    const setSelectedClipIds = vi.fn();
    renderWithSession(
      api,
      <MidiClipInspector
        session={session}
        selectedClipIds={['midi:1']}
        setSelectedClipIds={setSelectedClipIds}
      />,
    );

    // Act
    fireEvent.click(screen.getByRole('button', { name: 'Mute clip' }));
    fireEvent.click(screen.getByRole('button', { name: 'Loop clip' }));
    vi.mocked(api.removeTimelineClips).mockRejectedValueOnce(new Error('MIDI deletion failed'));
    fireEvent.click(screen.getByRole('button', { name: 'Clip actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    expect(await screen.findByText('MIDI deletion failed')).toBeInTheDocument();
    expect(setSelectedClipIds).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Clip actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));

    // Assert
    expect(api.updateMidiClip).toHaveBeenCalledWith('midi:1', { muted: true });
    expect(api.updateMidiClip).toHaveBeenCalledWith('midi:1', { loopEnabled: false });
    expect(api.removeTimelineClips).toHaveBeenCalledWith([], ['midi:1']);
    await waitFor(() => expect(setSelectedClipIds).toHaveBeenCalledWith([]));
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();

    vi.mocked(api.updateMidiClip).mockRejectedValueOnce(new Error('MIDI edit failed'));
    fireEvent.click(screen.getByRole('button', { name: 'Mute clip' }));
    expect(await screen.findByText('MIDI edit failed')).toBeInTheDocument();
  });

  it('commits Clip gain from the keyboard and typed values, and discards a cancelled edit', async () => {
    // Arrange
    const session = recordingSession();
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.updateAudioClip = vi.fn().mockResolvedValue({
      canonical: canonicalState(session),
      createdEntityIds: {},
      projection: { state: 'notRequired' },
    });
    renderWithSession(
      api,
      <AudioClipInspector
        session={session}
        selectedClipIds={['clip:a']}
        setSelectedClipIds={() => undefined}
      />,
      canonicalState(session),
    );
    const slider = screen.getByLabelText('Clip gain');

    // Act
    fireEvent.change(slider, { target: { value: '-6' } });
    fireEvent.keyUp(slider, { key: 'ArrowLeft' });
    await waitFor(() => expect(api.updateAudioClip).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole('button', { name: 'Edit clip gain' }));
    const typed = screen.getByRole('spinbutton', { name: 'Edit clip gain' });
    fireEvent.change(typed, { target: { value: '3.2' } });
    fireEvent.blur(typed);
    await waitFor(() => expect(api.updateAudioClip).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole('button', { name: 'Edit clip gain' }));
    const cancelled = screen.getByRole('spinbutton', { name: 'Edit clip gain' });
    fireEvent.change(cancelled, { target: { value: '9' } });
    fireEvent.keyDown(cancelled, { key: 'Escape' });
    fireEvent.blur(cancelled);
    fireEvent.click(screen.getByRole('button', { name: 'Edit clip gain' }));
    const clamped = screen.getByRole('spinbutton', { name: 'Edit clip gain' });
    expect(clamped).toHaveAttribute('min', '-90');
    expect(clamped).toHaveAttribute('max', '24');
    fireEvent.change(clamped, { target: { value: '-100' } });
    fireEvent.blur(clamped);
    await waitFor(() => expect(api.updateAudioClip).toHaveBeenCalledTimes(3));

    // Assert
    expect(api.updateAudioClip).toHaveBeenNthCalledWith(1, 'clip:a', { gainDb: -6 });
    expect(api.updateAudioClip).toHaveBeenNthCalledWith(2, 'clip:a', { gainDb: 3.2 });
    expect(api.updateAudioClip).toHaveBeenNthCalledWith(3, 'clip:a', { gainDb: -90 });
    expect(api.updateAudioClip).toHaveBeenCalledTimes(3);
    expect(screen.getByRole('button', { name: 'Edit clip gain' })).toHaveTextContent('+0.0 dB');
  });

  it('changes Raw/Processed source only on the selected Clip', async () => {
    const initial = recordingSession();
    const canonical = structuredClone(initial);
    canonical.arrangement.audioClips[0].takeVariant = 'processed';
    const api = createFakeNativeApi({
      bootstrapState: { canonical: canonicalState(initial) },
      responses: {
        setAudioClipTakeVariant: {
          canonical: canonicalState(canonical),
          projection: { state: 'notRequired' as const },
        },
      },
    });
    function Harness() {
      const [session, setSession] = useState(initial);
      const [store] = useState(() =>
        createTestSessionStore(api, {
          applyCanonical: (canonical) => setSession(canonical.session),
        }),
      );
      return (
        <>
          <TestSessionProvider api={api} store={store}>
            <AudioClipInspector
              session={session}
              selectedClipIds={['clip:a']}
              setSelectedClipIds={() => undefined}
            />
          </TestSessionProvider>
          <output data-testid="variants">
            {session.arrangement.audioClips.map((clip) => clip.takeVariant).join(',')}
          </output>
        </>
      );
    }
    renderWithSession(api, <Harness />);

    fireEvent.click(
      within(screen.getByRole('group', { name: 'Clip recording source' })).getByRole('button', {
        name: 'Processed',
      }),
    );

    await waitFor(() => expect(screen.getByTestId('variants')).toHaveTextContent('processed,raw'));
  });

  it('auditions a Take against its variant and stops when Native playback ends', async () => {
    const session = recordingSession();
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.setResponse('startTakeComparison', () => {
      api.audio = { ...api.audio, previewing: true };
      return api.audio;
    });
    api.setResponse('switchTakeComparisonVariant', () => api.audio);
    const selection: EditorSelection = { kind: 'clips', clipIds: ['clip:a'] };
    const store = createSessionStore(api);
    store.getState().host.adoptConnection(api.hostConnectionState);
    store.getState().project.acceptBootstrap(api.bootstrapState);
    const stop = bindSessionRuntime(store, api);
    render(
      <TestSessionProvider api={api} store={store}>
        <TakeInspector
          session={session}
          selection={selection}
          recordingActive={false}
          recordingCommandPending={false}
          onRecordAnotherTake={() => undefined}
        />
      </TestSessionProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
    await waitFor(() => expect(api.calls).toContain('startTakeComparison'));

    fireEvent.click(screen.getByRole('button', { name: 'Processed' }));
    await waitFor(() => expect(api.calls).toContain('switchTakeComparisonVariant'));
    expect(api.calls).not.toContain('setAudioClipTakeVariant');

    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    await waitFor(() => expect(api.calls).toContain('stopTakeComparison'));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Preview' })).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument());

    act(() => {
      api.emitAudioStatus({ ...api.audio, previewing: false });
    });

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Preview' })).toBeInTheDocument(),
    );
    stop();
  });

  it('switches between recording groups and records another take', async () => {
    const session = recordingSession();
    const firstTake = session.arrangement.takes[0];
    session.arrangement.takes.push({
      ...firstTake,
      id: 'take:2',
      sessionId: 'recording:2',
      passId: 'pass:2',
    });
    session.arrangement.recordingSessions.push({
      id: 'recording:2',
      startTick: 960,
      passIds: ['pass:2'],
      trackSlots: [
        {
          trackId: 'track:audio',
          activeTakeId: 'take:2',
          timelineClipId: 'clip:b',
        },
      ],
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const onRecordAnotherTake = vi.fn();
    renderWithSession(
      api,
      <TakeInspector
        session={session}
        selection={{ kind: 'track', trackId: 'track:audio' }}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={onRecordAnotherTake}
      />,
    );

    const groupSelector = screen.getByRole('combobox', { name: 'Recording group' });
    expect(groupSelector).toHaveValue('recording:2');
    expect(screen.getByText('CURRENT')).toBeInTheDocument();

    fireEvent.change(groupSelector, { target: { value: 'recording:1' } });
    await waitFor(() => expect(groupSelector).toHaveValue('recording:1'));

    fireEvent.click(screen.getByRole('button', { name: 'Record another take' }));
    expect(onRecordAnotherTake).toHaveBeenCalledWith('recording:1');
  });

  it('keeps MIDI Takes available without offering an audio preview', () => {
    const session = defaultSession();
    const midiTakeId = 'take:midi';
    const midiSessionId = 'recording:midi';
    const midiAssetId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41c');
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:midi-take',
      name: 'MIDI Take',
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
    session.arrangement.takes.push({
      id: midiTakeId,
      sessionId: midiSessionId,
      passId: 'pass:midi',
      trackId: 'track:midi-take',
      startTick: 0,
      durationTicks: 960,
      sourceStartSample: 0,
      sourceEndSample: 0,
      midiAssetId,
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:midi-take',
      name: 'MIDI Take',
      trackId: 'track:midi-take',
      startTick: 0,
      durationTicks: 960,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
      recordingTakeId: midiTakeId,
    });
    session.arrangement.recordingSessions.push({
      id: midiSessionId,
      startTick: 0,
      passIds: ['pass:midi'],
      trackSlots: [
        {
          trackId: 'track:midi-take',
          activeTakeId: 'take:other',
          timelineClipId: 'clip:midi-take',
        },
      ],
    });

    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    renderWithSession(
      api,
      <TakeInspector
        session={session}
        selection={{ kind: 'clips', clipIds: ['clip:midi-take'] }}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={() => undefined}
      />,
    );

    expect(screen.getByText('MIDI')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Preview' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Use Take 1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Place copy' })).toBeInTheDocument();
  });
});
