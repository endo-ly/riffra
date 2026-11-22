// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { PluginEntry, Rig, Track } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/preview/fixtures';
import { RigControl } from '@/features/rig/RigControl';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { createTestSessionStore, TestSessionProvider } from '@/test/support/session-provider';

afterEach(cleanup);

const effect = {
  id: 'effect:delay',
  name: 'Delay',
  bypassed: false,
  plugin: { path: 'Delay.vst3', parameterValues: [], disabledPlaceholder: false },
};

function rig(overrides: Partial<Rig> = {}): Rig {
  return {
    id: 'rig:1',
    name: 'Live Rig',
    audioInput: { channelIndex: 0 },
    effects: [effect],
    gainDb: -6,
    ...overrides,
  };
}

function monitoringTrack(name = 'Gtr'): Track {
  return {
    id: 'track:1',
    name,
    kind: 'audio',
    gainDb: 0,
    pan: 0,
    muted: false,
    solo: false,
    armed: false,
    monitoring: 'on',
    audioInput: { channelIndex: 0 },
    midiInput: {},
    effects: [],
    panLaw: 'equalPower',
    externalAudioSourceTrackIds: [],
  };
}

function renderRig(
  { activeRigId = null, rigs = [rig()], session = defaultSession(), plugins = [] } = {} as {
    activeRigId?: string | null;
    rigs?: Rig[];
    session?: ReturnType<typeof defaultSession>;
    plugins?: PluginEntry[];
  },
) {
  const api = createFakeNativeApi({
    bootstrapState: { canonical: canonicalState(session) },
    rigState: { rigs, activeRigId },
  });
  const store = createTestSessionStore(api, { session });
  store.getState().rig.accept({ rigs, activeRigId });
  api.onRigStateChanged((state) => store.getState().rig.accept(state));
  store.getState().catalog.acceptPlugins(plugins);
  render(
    <TestSessionProvider api={api} store={store}>
      <RigControl />
    </TestSessionProvider>,
  );
  return { api, store };
}

function plugin(name: string): PluginEntry {
  return {
    id: name.toLowerCase(),
    name,
    vendor: 'Riffra',
    version: '1.0',
    identifier: name.toLowerCase(),
    format: 'VST3',
    role: 'effect',
    path: `${name}.vst3`,
    bundle: true,
    modifiedAtMs: null,
    scanState: 'validated',
  };
}

describe('RigControl', () => {
  it('starts from a small stopped button and creates a rig from the panel', async () => {
    // Arrange
    const { api } = renderRig();

    // Act
    fireEvent.click(screen.getByRole('button', { name: 'Rig' }));
    fireEvent.change(screen.getByLabelText('New rig name'), { target: { value: 'Keys' } });
    fireEvent.click(screen.getByRole('button', { name: 'New rig' }));

    // Assert
    expect(screen.getByRole('dialog', { name: 'Rigs' })).toBeInTheDocument();
    await waitFor(() => expect(api.calls).toContain('createRig'));
    fireEvent.click(await screen.findByRole('button', { name: /^Keys/ }));
    await waitFor(() => expect(api.calls).toContain('startRig'));
    expect(screen.getByRole('button', { name: 'Rig: Keys' })).toBeInTheDocument();
  });

  it('shows the playing rig as a two-line control and stops it', async () => {
    // Arrange
    const { api } = renderRig({ activeRigId: 'rig:1' });

    // Act
    expect(screen.getByRole('button', { name: 'Rig: Live Rig' })).toBeInTheDocument();
    expect(screen.getByText('In 1')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Stop rig' }));

    // Assert
    await waitFor(() => expect(api.calls).toContain('stopRig'));
    expect(await screen.findByRole('button', { name: 'Rig' })).toBeInTheDocument();
  });

  it('warns about a shared input and edits the playing rig from the list', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks = [monitoringTrack()];
    const { api } = renderRig({
      activeRigId: 'rig:1',
      session,
      plugins: [plugin('Reverb')],
    });

    // Act
    fireEvent.click(screen.getByRole('button', { name: 'Open rig list' }));

    // Assert
    expect(screen.getByText('In 1 is also monitored by track "Gtr".')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Bypass Delay' }));
    await waitFor(() => expect(api.calls).toContain('setRigEffectBypassed'));
    expect(await screen.findByRole('button', { name: 'Bypass Delay' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    fireEvent.click(screen.getByRole('button', { name: 'Add Effect' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Reverb — Riffra' }));
    await waitFor(() => expect(api.calls).toContain('addRigEffect'));
    expect(await screen.findByText('Reverb')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(
      within(screen.getByRole('dialog', { name: 'Delete rig?' })).getByText('Cancel'),
    );
    expect(api.calls).not.toContain('deleteRig');
    expect(screen.getByRole('dialog', { name: 'Rigs' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(
      within(screen.getByRole('dialog', { name: 'Delete rig?' })).getByRole('button', {
        name: 'Delete',
      }),
    );
    await waitFor(() => expect(api.calls).toContain('deleteRig'));
  });
});
