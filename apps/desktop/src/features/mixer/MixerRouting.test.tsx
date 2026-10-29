import { TestSessionProvider } from '@/test/session-provider';
// @vitest-environment jsdom
import type { AutomationTarget, Track } from '@/model/domain';
import { defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi } from '@/native/native-api-fake';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';
import { MixerRouting } from './MixerRouting';

afterEach(cleanup);

const track: Track = {
  externalAudioSourceTrackIds: [],
  panLaw: 'equalPower' as const,
  id: 'track:1',
  name: 'Keys',
  kind: 'audio',
  gainDb: 0,
  pan: 0,
  muted: false,
  solo: false,
  armed: false,
  monitoring: 'off',
  midiInput: {},
  effects: [
    {
      id: 'effect:1',
      name: 'Compressor',
      bypassed: false,
      plugin: { path: 'comp.vst3', parameterValues: [], disabledPlaceholder: false },
    },
  ],
};

function sidechainSession() {
  const session = defaultSession();
  session.arrangement.tracks = [structuredClone(track)];
  session.arrangement.mixer.sidechainSends = [
    {
      id: 'sidechain:1',
      sourceTrackId: track.id,
      targetDeviceId: 'effect:1',
      levelDb: -6,
      position: 'postFader',
      enabled: true,
    },
  ];
  return session;
}

it('lists the sidechain sends of the track and opens their automation', () => {
  const session = sidechainSession();
  const api = new FakeNativeApi();
  const opened: AutomationTarget[] = [];
  render(
    <TestSessionProvider api={api}>
      <MixerRouting
        arrangement={session.arrangement}
        track={track}
        commit={() => undefined}
        onClose={() => undefined}
        onOpenAutomation={(target) => opened.push(target)}
      />
    </TestSessionProvider>,
  );
  expect(screen.getByText('Compressor')).toBeInTheDocument();
  expect(screen.getByLabelText('Compressor sidechain level')).toHaveValue('-6');
  fireEvent.click(screen.getByRole('button', { name: 'Automation' }));
  expect(opened).toEqual([{ kind: 'sidechainSendLevel', sidechainSendId: 'sidechain:1' }]);
});

it('hides the sidechain list for tracks without sidechain sends', () => {
  const session = sidechainSession();
  session.arrangement.mixer.sidechainSends = [];
  const api = new FakeNativeApi();
  render(
    <TestSessionProvider api={api}>
      <MixerRouting
        arrangement={session.arrangement}
        track={track}
        commit={() => undefined}
        onClose={() => undefined}
        onOpenAutomation={() => undefined}
      />
    </TestSessionProvider>,
  );
  expect(
    screen.getByText('Add sidechain sends from the target device in Devices.'),
  ).toBeInTheDocument();
});
