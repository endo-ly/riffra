import { TestSessionProvider } from '@/test/support/session-provider';
// @vitest-environment jsdom

import type { AudioClip, MidiClip } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/model/asset-id';
import { createFakeNativeApi, fakeAudioStatus } from '@/test/support/fake-native-api';
import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { PropertiesPanelView } from '@/features/inspector/PropertiesPanel';

afterEach(cleanup);

function renderPanel(
  selection: Parameters<typeof PropertiesPanelView>[0]['editorSelection'],
  initialSession = defaultSession(),
  onOpenDevices = () => undefined,
) {
  const session = initialSession;
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
  const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
  render(
    <TestSessionProvider api={api}>
      <PropertiesPanelView
        audio={fakeAudioStatus()}
        recordingCommandPending={false}
        session={session}
        editorSelection={selection}
        setEditorSelection={() => undefined}
        onOpenDevices={onOpenDevices}
        onRecordAnotherTake={() => undefined}
      />
    </TestSessionProvider>,
  );
}

function audioClip(id: string, name: string, startTick: number): AudioClip {
  return {
    id,
    name,
    trackId: 'track:audio',
    assetId: toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a'),
    startTick,
    sourceRange: { start: 0, end: 1_000 },
    sourceSampleRate: 48_000,
    timelineDuration: { frames: 1_000, sampleRate: 48_000 },
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

function midiClip(): MidiClip {
  return {
    instrumentControlEvents: [],
    id: 'clip:midi',
    name: 'MIDI Clip',
    trackId: 'track:instrument',
    startTick: 0,
    durationTicks: 960,
    notes: [],
    events: [],
    muted: false,
    loopEnabled: false,
  };
}

describe('PropertiesPanel', () => {
  it('limits mixed clip selections to shared clip actions', () => {
    const session = defaultSession();
    session.arrangement.audioClips.push(audioClip('clip:audio', 'Audio Clip', 0));
    session.arrangement.midiClips.push(midiClip());

    renderPanel({ kind: 'clips', clipIds: ['clip:audio', 'clip:midi'] }, session);

    expect(screen.getByRole('button', { name: 'Duplicate' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Clip name')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('MIDI clip name')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Crossfade' })).not.toBeInTheDocument();
  });

  it('does not offer Crossfade when a mixed selection contains two Audio Clips', () => {
    const session = defaultSession();
    session.arrangement.audioClips.push(
      audioClip('clip:audio-a', 'Audio Clip A', 0),
      audioClip('clip:audio-b', 'Audio Clip B', 1_000),
    );
    session.arrangement.midiClips.push(midiClip());

    renderPanel({ kind: 'clips', clipIds: ['clip:audio-a', 'clip:audio-b', 'clip:midi'] }, session);

    expect(screen.queryByRole('button', { name: 'Crossfade' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Duplicate' })).toBeInTheDocument();
  });
});
