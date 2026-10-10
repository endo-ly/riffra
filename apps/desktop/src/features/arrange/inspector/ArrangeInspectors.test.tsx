// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ArrangeClipInspector } from './ArrangeClipInspector';
import { MidiClipInspector } from './MidiClipInspector';
import { TakeInspector } from './TakeInspector';
import { TrackInspector } from './TrackInspector';
import type { ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import type { CreativeSession } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/native/contracts';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';

afterEach(cleanup);

function recordingSession(): CreativeSession {
  const session = defaultSession();
  const rawId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a');
  const processedId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41b');
  session.arrangement.tracks.push({
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

describe('Arrange Inspectors', () => {
  it('does not show Audio Monitoring for an Instrument Track and surfaces operation errors', async () => {
    const session = defaultSession();
    const track = {
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.setTrackMidiInput = vi.fn().mockRejectedValue(new Error('MIDI route failed'));

    render(
      <TrackInspector
        track={track}
        session={session}
        applyCanonicalState={() => true}
        audio={fakeAudioStatus()}
        onOpenDevices={() => undefined}
        api={api}
      />,
    );

    expect(screen.queryByText('MONITORING')).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('MIDI channel'), { target: { value: '1' } });
    expect(await screen.findByRole('status')).toHaveTextContent('MIDI route failed');
  });

  it('edits MIDI Clip timing in bars and beats and shows the current value after editing', () => {
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
      loopEnabled: false,
    });
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.updateMidiClip = vi.fn().mockResolvedValue(null);
    render(
      <MidiClipInspector
        session={session}
        applyCanonicalState={() => true}
        selectedClipIds={['midi:1']}
        setSelectedClipIds={() => undefined}
        api={api}
      />,
    );
    const start = screen.getByLabelText('Start');
    const length = screen.getByLabelText('Length');

    // Act
    fireEvent.change(start, { target: { value: '3.2' } });
    fireEvent.keyDown(start, { key: 'Enter' });
    fireEvent.blur(start);
    fireEvent.change(length, { target: { value: '9' } });
    fireEvent.keyDown(length, { key: 'Escape' });
    fireEvent.blur(length);

    // Assert
    expect(start).toHaveValue('1.1.000');
    expect(length).toHaveValue('1.0.000');
    expect(api.updateMidiClip).toHaveBeenCalledTimes(1);
    expect(api.updateMidiClip).toHaveBeenCalledWith('midi:1', { startTick: 2 * 3_840 + 960 });
  });

  it('shows an Audio Clip length derived from its audio and moves it by bars', () => {
    // Arrange
    const session = recordingSession();
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.updateAudioClip = vi.fn().mockResolvedValue(null);
    render(
      <ArrangeClipInspector
        session={session}
        applyCanonicalState={() => true}
        selectedClipIds={['clip:a']}
        setSelectedClipIds={() => undefined}
        api={api}
      />,
    );
    const start = screen.getByLabelText('Start');

    // Act
    fireEvent.change(start, { target: { value: '2' } });
    fireEvent.blur(start);

    // Assert
    expect(screen.getByText('0.0.040')).toBeInTheDocument();
    expect(api.updateAudioClip).toHaveBeenCalledWith('clip:a', { startTick: 3_840 });
  });

  it('commits Clip gain from the keyboard and typed values, and discards a cancelled edit', () => {
    // Arrange
    const session = recordingSession();
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    api.updateAudioClip = vi.fn().mockResolvedValue(null);
    render(
      <ArrangeClipInspector
        session={session}
        applyCanonicalState={() => true}
        selectedClipIds={['clip:a']}
        setSelectedClipIds={() => undefined}
        api={api}
      />,
    );
    const slider = screen.getByLabelText('Clip gain');

    // Act
    fireEvent.change(slider, { target: { value: '-6' } });
    fireEvent.keyUp(slider, { key: 'ArrowLeft' });
    fireEvent.click(screen.getByRole('button', { name: 'Edit clip gain' }));
    const typed = screen.getByRole('spinbutton', { name: 'Edit clip gain' });
    fireEvent.change(typed, { target: { value: '3.2' } });
    fireEvent.blur(typed);
    fireEvent.click(screen.getByRole('button', { name: 'Edit clip gain' }));
    const cancelled = screen.getByRole('spinbutton', { name: 'Edit clip gain' });
    fireEvent.change(cancelled, { target: { value: '9' } });
    fireEvent.keyDown(cancelled, { key: 'Escape' });
    fireEvent.blur(cancelled);
    for (const value of ['30', '-70']) {
      fireEvent.click(screen.getByRole('button', { name: 'Edit clip gain' }));
      const input = screen.getByRole('spinbutton', { name: 'Edit clip gain' });
      expect(input).toHaveAttribute('min', '-60');
      expect(input).toHaveAttribute('max', '24');
      fireEvent.change(input, { target: { value } });
      fireEvent.blur(input);
    }

    // Assert
    expect(api.updateAudioClip).toHaveBeenNthCalledWith(1, 'clip:a', { gainDb: -6 });
    expect(api.updateAudioClip).toHaveBeenNthCalledWith(2, 'clip:a', { gainDb: 3.2 });
    expect(api.updateAudioClip).toHaveBeenNthCalledWith(3, 'clip:a', { gainDb: 24 });
    expect(api.updateAudioClip).toHaveBeenNthCalledWith(4, 'clip:a', { gainDb: -60 });
    expect(api.updateAudioClip).toHaveBeenCalledTimes(4);
    expect(screen.getByRole('button', { name: 'Edit clip gain' })).toHaveTextContent('+0.0 dB');
  });

  it('changes Raw/Processed source only on the selected Clip', async () => {
    const initial = recordingSession();
    const canonical = structuredClone(initial);
    canonical.arrangement.audioClips[0].takeVariant = 'processed';
    const api = new FakeNativeApi({
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
      return (
        <>
          <ArrangeClipInspector
            session={session}
            applyCanonicalState={(canonical) => {
              setSession(canonical.session);
              return true;
            }}
            selectedClipIds={['clip:a']}
            setSelectedClipIds={() => undefined}
            api={api}
          />
          <output data-testid="variants">
            {session.arrangement.audioClips.map((clip) => clip.takeVariant).join(',')}
          </output>
        </>
      );
    }
    render(<Harness />);

    fireEvent.click(
      within(screen.getByRole('group', { name: 'Clip recording source' })).getByRole('button', {
        name: 'Processed',
      }),
    );

    await waitFor(() => expect(screen.getByTestId('variants')).toHaveTextContent('processed,raw'));
  });

  it('keeps A/B audition independent from the Clip variant', async () => {
    const session = recordingSession();
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const selection: ArrangeSelection = { kind: 'clips', clipIds: ['clip:a'] };
    render(
      <TakeInspector
        session={session}
        selection={selection}
        applyCanonicalState={() => true}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={() => undefined}
        api={api}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
    await waitFor(() => expect(api.calls).toContain('startTakeComparison'));
    fireEvent.click(screen.getByRole('button', { name: 'Processed' }));
    await waitFor(() => expect(api.calls).toContain('switchTakeComparisonVariant'));
    expect(api.calls).not.toContain('setAudioClipTakeVariant');
  });

  it('shows the current Take explicitly and provides a stop action for audition', async () => {
    const session = recordingSession();
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    render(
      <TakeInspector
        session={session}
        selection={{ kind: 'track', trackId: 'track:audio' }}
        applyCanonicalState={() => true}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={() => undefined}
        api={api}
      />,
    );

    expect(screen.getByText('CURRENT')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Take 1 is current' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Preview' })).toBeInTheDocument(),
    );
    expect(api.calls).toContain('stopTakeComparison');
  });

  it('clears the audition when Native reports that preview playback ended', async () => {
    const session = recordingSession();
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    render(
      <TakeInspector
        session={session}
        selection={{ kind: 'track', trackId: 'track:audio' }}
        applyCanonicalState={() => true}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={() => undefined}
        api={api}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument());

    act(() => {
      api.emitAudioStatus({ ...api.audio, previewing: false });
    });

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Preview' })).toBeInTheDocument(),
    );
  });

  it('routes Record another take to the selected recording group', () => {
    const session = recordingSession();
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const onRecordAnotherTake = vi.fn();
    render(
      <TakeInspector
        session={session}
        selection={{ kind: 'track', trackId: 'track:audio' }}
        applyCanonicalState={() => true}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={onRecordAnotherTake}
        api={api}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Record another take' }));

    expect(onRecordAnotherTake).toHaveBeenCalledWith('recording:1');
  });

  it('lets a Track selection switch between recording groups', async () => {
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
    const api = new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    render(
      <TakeInspector
        session={session}
        selection={{ kind: 'track', trackId: 'track:audio' }}
        applyCanonicalState={() => true}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={() => undefined}
        api={api}
      />,
    );

    const groupSelector = screen.getByRole('combobox', { name: 'Recording group' });
    expect(groupSelector).toHaveValue('recording:2');
    fireEvent.change(groupSelector, { target: { value: 'recording:1' } });

    await waitFor(() => expect(groupSelector).toHaveValue('recording:1'));
    expect(screen.getByText('CURRENT')).toBeInTheDocument();
  });

  it('keeps MIDI Takes available without offering an audio preview', () => {
    const session = defaultSession();
    const midiTakeId = 'take:midi';
    const midiSessionId = 'recording:midi';
    const midiAssetId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41c');
    session.arrangement.tracks.push({
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

    render(
      <TakeInspector
        session={session}
        selection={{ kind: 'clips', clipIds: ['clip:midi-take'] }}
        applyCanonicalState={() => true}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={() => undefined}
        api={new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } })}
      />,
    );

    expect(screen.getByText('MIDI')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Preview' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Use Take 1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Place copy' })).toBeInTheDocument();
  });
});
