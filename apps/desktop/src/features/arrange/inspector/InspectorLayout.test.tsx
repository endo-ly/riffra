// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { TrackInspector } from '@/features/arrange/inspector/TrackInspector';
import { ArrangeClipInspector } from '@/features/arrange/inspector/ArrangeClipInspector';
import { MidiClipInspector } from '@/features/arrange/inspector/MidiClipInspector';
import type { CreativeSession } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/native/contracts';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';

afterEach(cleanup);

function sessionWithContent(): CreativeSession {
  const session = defaultSession();
  session.arrangement.tracks.push(
    {
      id: 'track:audio',
      name: 'Audio',
      kind: 'audio',
      gainDb: -3.5,
      pan: 0.25,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'auto',
      midiInput: {},
      rack: { devices: [], macros: [] },
    },
    {
      id: 'track:instrument',
      name: 'Keys',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: { deviceId: 'k', channel: 1 },
      rack: { devices: [], macros: [] },
    },
  );
  session.arrangement.audioClips.push({
    id: 'clip:audio',
    name: 'Audio Clip',
    trackId: 'track:audio',
    assetId: toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a'),
    startTick: 480,
    sourceRange: { start: 0, end: 48_000 },
    sourceSampleRate: 48_000,
    timelineDuration: { frames: 48_000, sampleRate: 48_000 },
    gainDb: -2,
    pan: -0.5,
    fadeIn: { frames: 480, sampleRate: 48_000 },
    fadeOut: { frames: 0, sampleRate: 48_000 },
    fadeShape: 'equalPower',
    loopEnabled: false,
    muted: false,
    takeVariant: 'raw',
  });
  session.arrangement.midiClips.push({
    id: 'clip:midi',
    name: 'MIDI Clip',
    trackId: 'track:instrument',
    startTick: 0,
    durationTicks: 960,
    notes: [],
    events: [],
    muted: false,
    loopEnabled: false,
  });
  return session;
}

const inspectorProps = (session: CreativeSession) => ({
  session,
  applyCanonicalState: () => true,
  api: new FakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } }),
});

describe('compact inspector layout', () => {
  it('renders Track MIX as a compact cluster without a section header', () => {
    const session = sessionWithContent();
    render(
      <TrackInspector
        {...inspectorProps(session)}
        track={session.arrangement.tracks[0]}
        audio={fakeAudioStatus()}
        missingDeviceIds={[]}
        plugins={[]}
        onDisableMissingPlugin={async () => undefined}
        onReplaceMissingPlugin={async () => undefined}
        onRescanMissingPlugins={async () => undefined}
      />,
    );
    expect(screen.getByLabelText('Track mix')).toBeInTheDocument();
    expect(screen.getByLabelText('Track gain')).toBeInTheDocument();
    expect(screen.getByLabelText('Track pan')).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Monitoring' })).toBeInTheDocument();
    expect(screen.queryByText('MIX')).not.toBeInTheDocument();
    expect(screen.queryByText('MONITORING')).not.toBeInTheDocument();
  });

  it('renders Clip timing as a field pair with consolidated header meta', () => {
    const session = sessionWithContent();
    render(
      <ArrangeClipInspector
        {...inspectorProps(session)}
        selectedClipIds={['clip:audio']}
        setSelectedClipIds={() => undefined}
      />,
    );
    expect(screen.getByLabelText('Clip mix')).toBeInTheDocument();
    expect(screen.getByLabelText('Clip gain')).toBeInTheDocument();
    expect(screen.getByText('TIMING')).toBeInTheDocument();
    expect(screen.getByText(/s · /)).toBeInTheDocument();
    expect(screen.queryByText('CLIP MIX')).not.toBeInTheDocument();
  });

  it('renders MIDI clip timing as a field pair', () => {
    const session = sessionWithContent();
    render(
      <MidiClipInspector
        {...inspectorProps(session)}
        selectedClipIds={['clip:midi']}
        setSelectedClipIds={() => undefined}
      />,
    );
    expect(screen.getByText('TIMING')).toBeInTheDocument();
    const start = screen.getByDisplayValue('0');
    const length = screen.getByDisplayValue('960');
    expect(start.closest('div')).toBe(length.closest('div'));
  });
});
