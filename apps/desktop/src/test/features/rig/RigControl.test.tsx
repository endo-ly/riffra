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

const bassRig = rig({ id: 'rig:2', name: 'Bass Rig', effects: [] });

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
  {
    activeRigId = null,
    lastPlayedRigId = null,
    rigs = [rig()],
    session = defaultSession(),
    plugins = [],
  } = {} as {
    activeRigId?: string | null;
    lastPlayedRigId?: string | null;
    rigs?: Rig[];
    session?: ReturnType<typeof defaultSession>;
    plugins?: PluginEntry[];
  },
) {
  const rigState = { rigs, activeRigId, lastPlayedRigId };
  const api = createFakeNativeApi({
    bootstrapState: { canonical: canonicalState(session) },
    rigState,
  });
  const store = createTestSessionStore(api, { session });
  store.getState().rig.accept(rigState);
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

function openPanel(barLabel: string) {
  fireEvent.click(screen.getByRole('button', { name: barLabel }));
}

function openRigList() {
  fireEvent.click(screen.getByRole('button', { name: 'Switch rig' }));
}

function rigOption(name: string) {
  return within(screen.getByRole('listbox', { name: 'Rigs' })).getByRole('option', {
    name: new RegExp(`^${name}`),
  });
}

function startCount(calls: string[]) {
  return calls.filter((call) => call === 'startRig').length;
}

describe('RigControl', () => {
  it('creates a rig from the empty panel and plays it', async () => {
    // Arrange
    const { api } = renderRig({ rigs: [] });

    // Act
    openPanel('Rig');
    fireEvent.click(screen.getByRole('button', { name: /New Rig/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Play Rig 1' }));

    // Assert
    await waitFor(() => expect(api.calls).toContain('startRig'));
    expect(await screen.findByRole('button', { name: 'Stop rig Rig 1' })).toBeInTheDocument();
    expect(screen.getByText('Playing')).toBeInTheDocument();
  });

  it('plays the rig played last from the bar and stops it there', async () => {
    // Arrange
    const { api } = renderRig({ rigs: [rig(), bassRig], lastPlayedRigId: 'rig:2' });

    // Act
    fireEvent.click(screen.getByRole('button', { name: 'Play rig Bass Rig' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Stop rig Bass Rig' }));

    // Assert
    await waitFor(() => expect(api.calls).toContain('stopRig'));
    expect(await screen.findByRole('button', { name: 'Play rig Bass Rig' })).toBeInTheDocument();
  });

  it('switches the playing rig but only shows a rig chosen while stopped', async () => {
    // Arrange
    const { api } = renderRig({ rigs: [rig(), bassRig], activeRigId: 'rig:1' });
    openPanel('Rig: Live Rig');

    // Act
    openRigList();
    fireEvent.click(rigOption('Bass Rig'));
    fireEvent.click(await screen.findByRole('button', { name: 'Stop Bass Rig' }));
    await screen.findByRole('button', { name: 'Play Bass Rig' });
    const startsWhilePlaying = startCount(api.calls);
    openRigList();
    fireEvent.click(rigOption('Live Rig'));

    // Assert
    expect(startsWhilePlaying).toBe(1);
    expect(await screen.findByRole('button', { name: 'Play Live Rig' })).toBeInTheDocument();
    expect(startCount(api.calls)).toBe(1);
  });

  it('renames a rig by double-click and renames or deletes another from its row menu', async () => {
    // Arrange
    renderRig({ rigs: [rig(), bassRig] });
    openPanel('Rig: Live Rig');

    // Act
    fireEvent.doubleClick(screen.getByRole('button', { name: 'Switch rig' }));
    const name = screen.getByLabelText('Rig name');
    fireEvent.change(name, { target: { value: 'Clean Rig' } });
    fireEvent.blur(name);
    await screen.findByRole('button', { name: 'Rig: Clean Rig' });
    openRigList();
    fireEvent.contextMenu(rigOption('Bass Rig'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }));
    fireEvent.change(screen.getByLabelText('Rig name'), { target: { value: 'Synth Rig' } });
    fireEvent.keyDown(screen.getByLabelText('Rig name'), { key: 'Enter' });
    await screen.findByRole('button', { name: 'Rig: Synth Rig' });
    openRigList();
    fireEvent.contextMenu(rigOption('Synth Rig'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    fireEvent.click(
      within(screen.getByRole('dialog', { name: 'Delete rig?' })).getByRole('button', {
        name: 'Delete',
      }),
    );

    // Assert
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Switch rig' })).toHaveTextContent('Clean Rig'),
    );
    openRigList();
    const list = screen.getByRole('listbox', { name: 'Rigs' });
    expect(
      within(list)
        .getAllByRole('option')
        .map((option) => option.textContent),
    ).toEqual([expect.stringMatching(/^Clean Rig/)]);
  });

  it('warns about a shared input and edits the playing rig', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks = [monitoringTrack()];
    const { api } = renderRig({
      activeRigId: 'rig:1',
      session,
      plugins: [plugin('Reverb')],
    });
    openPanel('Rig: Live Rig');

    // Act & Assert
    expect(screen.getByText(/In 1 is also monitored by track "Gtr"\./)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Bypass Delay' }));
    expect(await screen.findByRole('button', { name: 'Bypass Delay' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    fireEvent.click(screen.getByRole('button', { name: 'Add Effect' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Reverb — Riffra' }));
    expect(await screen.findByText('Reverb')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Move Reverb up' }));
    await waitFor(() =>
      expect(
        within(screen.getByRole('list', { name: 'Signal path' }))
          .getAllByRole('button', { name: /^Bypass/ })
          .map((button) => button.getAttribute('aria-label')),
      ).toEqual(['Bypass Reverb', 'Bypass Delay']),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Open Reverb editor' }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove Reverb' }));
    await waitFor(() => expect(screen.queryByText('Reverb')).not.toBeInTheDocument());
    expect(api.calls).toContain('openRigEditor');

    fireEvent.click(screen.getByRole('button', { name: 'Rig actions' }));
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.getByRole('dialog', { name: 'Rigs' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Rig actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Duplicate' }));
    expect(await screen.findByRole('button', { name: 'Switch rig' })).toHaveTextContent(
      'Live Rig copy',
    );
  });

  it('closes the panel on a press outside it', () => {
    // Arrange
    renderRig();
    openPanel('Rig: Live Rig');

    // Act
    fireEvent.pointerDown(document.body);

    // Assert
    expect(screen.queryByRole('dialog', { name: 'Rigs' })).toBeNull();
  });
});
