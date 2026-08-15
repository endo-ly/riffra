// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ArrangeClipInspector } from './ArrangeClipInspector';
import { TakeInspector } from './TakeInspector';
import { TrackInspector } from './TrackInspector';
import type { ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import type { CreativeSession } from '@/model/domain';
import { defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/native/contracts';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';

afterEach(cleanup);

function recordingSession(): CreativeSession {
  const session = defaultSession();
  const rawId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a');
  const processedId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41b');
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
      rack: { devices: [], macros: [] },
    };
    session.arrangement.tracks.push(track);
    const api = new FakeNativeApi({ bootstrapState: { session } });
    api.setTrackMidiInput = vi.fn().mockRejectedValue(new Error('MIDI route failed'));

    render(
      <TrackInspector
        track={track}
        session={session}
        setSession={() => undefined}
        audio={fakeAudioStatus()}
        missingDeviceIds={[]}
        plugins={[]}
        onDisableMissingPlugin={async () => undefined}
        onReplaceMissingPlugin={async () => undefined}
        onRescanMissingPlugins={async () => undefined}
        api={api}
      />,
    );

    expect(screen.queryByText('MONITORING')).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('MIDI channel'), { target: { value: '1' } });
    expect(await screen.findByRole('status')).toHaveTextContent('MIDI route failed');
  });

  it('changes Raw/Processed source only on the selected Clip', async () => {
    const initial = recordingSession();
    const canonical = structuredClone(initial);
    canonical.arrangement.audioClips[0].takeVariant = 'processed';
    const api = new FakeNativeApi({
      bootstrapState: { session: initial },
      responses: { setAudioClipTakeVariant: canonical },
    });
    function Harness() {
      const [session, setSession] = useState(initial);
      return (
        <>
          <ArrangeClipInspector
            session={session}
            setSession={setSession}
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const selection: ArrangeSelection = { kind: 'clips', clipIds: ['clip:a'] };
    render(
      <TakeInspector
        session={session}
        selection={selection}
        setSession={() => undefined}
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(
      <TakeInspector
        session={session}
        selection={{ kind: 'track', trackId: 'track:audio' }}
        setSession={() => undefined}
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

  it('routes Record another take to the selected recording group', () => {
    const session = recordingSession();
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const onRecordAnotherTake = vi.fn();
    render(
      <TakeInspector
        session={session}
        selection={{ kind: 'track', trackId: 'track:audio' }}
        setSession={() => undefined}
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
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(
      <TakeInspector
        session={session}
        selection={{ kind: 'track', trackId: 'track:audio' }}
        setSession={() => undefined}
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
      rack: { devices: [], macros: [] },
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
        setSession={() => undefined}
        recordingActive={false}
        recordingCommandPending={false}
        onRecordAnotherTake={() => undefined}
        api={new FakeNativeApi({ bootstrapState: { session } })}
      />,
    );

    expect(screen.getByText('MIDI')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Preview' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Use Take 1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Place copy' })).toBeInTheDocument();
  });
});
