// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { InspectorPanel } from './InspectorPanel';
import { defaultSession, toAssetId, type BootstrapState } from '@/model/domain';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';

afterEach(cleanup);

function renderPanel(
  selection: Parameters<typeof InspectorPanel>[0]['arrangeSelection'],
  initialSession = defaultSession(),
) {
  const session = initialSession;
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
  const boot: BootstrapState = {
    session,
    viewState: { workspace: 'arrange', designContext: { activeTool: 'sample' } },
    pluginCatalog: [],
    runtimeStarted: true,
    runtimeStartupFinished: true,
    recoveredFromGeneration: false,
    safeMode: false,
    nativeAvailable: true,
    recoveryCandidates: [],
    dataRoot: 'C:\\Riffra',
    vst3Root: 'C:\\VST3',
  };
  render(
    <InspectorPanel
      audio={fakeAudioStatus()}
      boot={boot}
      focusMode={false}
      setFocusMode={() => undefined}
      session={session}
      viewState={boot.viewState}
      setSession={() => undefined}
      arrangeSelection={selection}
      setArrangeSelection={() => undefined}
      missingDependencies={[]}
      plugins={[]}
      onDisableMissingPlugin={async () => undefined}
      onReplaceMissingPlugin={async () => undefined}
      onRescanMissingPlugins={async () => undefined}
      api={api}
    />,
  );
}

describe('InspectorPanel', () => {
  it('shows the selected Track title without a close button', () => {
    renderPanel({ kind: 'track', trackId: 'track:audio' });

    expect(screen.getAllByText('TRACK')[0]).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Close Inspector/ })).not.toBeInTheDocument();
  });

  it('shows the empty Arrange title without a clip selection', () => {
    renderPanel({ kind: 'none' });

    expect(screen.getByText('INSPECTOR')).toBeInTheDocument();
  });

  it('limits mixed clip selections to shared clip actions', () => {
    const session = defaultSession();
    session.arrangement.audioClips.push({
      id: 'clip:audio',
      name: 'Audio Clip',
      trackId: 'track:audio',
      assetId: toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a'),
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

    renderPanel({ kind: 'clips', clipIds: ['clip:audio', 'clip:midi'] }, session);

    expect(screen.getByRole('button', { name: 'Duplicate' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Clip name')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('MIDI clip name')).not.toBeInTheDocument();
  });

  it('does not offer Crossfade when a mixed selection contains two Audio Clips', () => {
    const session = defaultSession();
    const audioClip = {
      id: 'clip:audio-a',
      name: 'Audio Clip A',
      trackId: 'track:audio',
      assetId: toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a'),
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
      takeVariant: 'raw' as const,
    };
    session.arrangement.audioClips.push(audioClip, {
      ...audioClip,
      id: 'clip:audio-b',
      name: 'Audio Clip B',
      startTick: 1_000,
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

    renderPanel({ kind: 'clips', clipIds: ['clip:audio-a', 'clip:audio-b', 'clip:midi'] }, session);

    expect(screen.queryByRole('button', { name: 'Crossfade' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Duplicate' })).toBeInTheDocument();
  });
});
