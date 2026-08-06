// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { defaultSession } from '@/lib/domain';
import type { PluginEntry } from '@/lib/domain';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';
import App from '@/App';

const examplePlugin: PluginEntry = {
  id: 'plug:example',
  name: 'Example Synth',
  vendor: 'Acme',
  version: null,
  format: 'VST3',
  path: 'C:\\VST3\\example.vst3',
  bundle: false,
  modifiedAtMs: null,
  scanState: 'validated',
};

afterEach(cleanup);

function renderApp(fake: FakeNativeApi) {
  const result = render(<App api={fake} />);
  return result;
}

async function waitForAppShell() {
  await waitFor(() => expect(screen.getByRole('main')).toBeInTheDocument());
}

describe('App driven by FakeNativeApi', () => {
  it('leaves startup audio initialization to the native runtime', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);

    await waitForAppShell();
    await waitFor(() => expect(screen.getByRole('button', { name: /^MUTE$/ })).toBeInTheDocument());
    expect(fake.calls).toContain('bootstrap');
    await waitFor(() => expect(fake.calls).toContain('getAudioStatus'));
    expect(fake.calls).not.toContain('restoreCurrentRack');
    expect(fake.calls).not.toContain('restoreSamplePads');
    expect(fake.calls).not.toContain('setEmergencyMute');
    expect(fake.bootstrapState.session.workspace).toBe('arrange');
    expect(
      within(screen.getByRole('navigation', { name: /Workspace/ })).getAllByRole('button'),
    ).toHaveLength(3);

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /^MUTE$/ }));
    await waitFor(() => expect(screen.getByRole('button', { name: /UNMUTE/ })).toBeInTheDocument());
    expect(fake.audio.state).toBe('muted');
  });

  it('keeps shell notices and project actions available without a home surface', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: {
        safeMode: true,
        recoveredFromGeneration: true,
        recoveryCandidates: [
          {
            fileName: 'generation-1.json',
            updatedAtMs: 1,
            sessionId: 'scratch-1',
            projectName: 'Recovered Project',
            note: 'Stable generation',
          },
        ],
      },
    });
    renderApp(fake);

    await waitForAppShell();
    expect(screen.getByText('SAFE MODE')).toBeInTheDocument();
    expect(screen.getByText('RECOVERY CHOICE')).toBeInTheDocument();

    const workspaceNav = screen.getByRole('navigation', { name: /Workspace/ });
    expect(within(workspaceNav).getAllByRole('button')).toHaveLength(3);
    expect(screen.queryByRole('button', { name: /Home/ })).not.toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Search or command/ }));
    expect(screen.getByText('Import Project')).toBeInTheDocument();
    expect(screen.getByText('Export Project')).toBeInTheDocument();
    expect(screen.getByText('Audio Settings')).toBeInTheDocument();
  });

  it('keeps the runtime muted after a sidecar restart following startup', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);

    await waitForAppShell();
    await waitFor(() => expect(fake.audio.state).toBe('ready'));
    fake.calls.splice(0);

    fake.emitRuntimeRestarted(2);

    await waitFor(() => expect(fake.calls).toContain('restoreSamplePads'));
    expect(fake.audio.state).toBe('muted');
    expect(fake.calls).not.toContain('setEmergencyMute');
  });

  it('restores the Play rack when entering Play instead of during startup', async () => {
    const fake = new FakeNativeApi();
    fake.setAudioState('muted');
    renderApp(fake);

    await waitForAppShell();
    expect(fake.calls).not.toContain('restoreCurrentRack');

    const user = userEvent.setup();
    await user.click(
      within(screen.getByRole('navigation', { name: /Workspace/ })).getByRole('button', {
        name: /Play/,
      }),
    );
    await waitFor(() => expect(fake.calls).toContain('restoreCurrentRack'));
    expect(fake.calls).not.toContain('setEmergencyMute');
    expect(fake.audio.state).toBe('muted');
  });

  it('rehydrates the current Play runtime after a sidecar restart', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: { session: { ...defaultSession(), workspace: 'play' } },
    });
    renderApp(fake);

    await waitForAppShell();
    fake.calls.splice(0);

    fake.emitRuntimeRestarted(2);

    await waitFor(() => expect(fake.calls).toContain('restoreSamplePads'));
    await waitFor(() => expect(fake.calls).toContain('restoreCurrentRack'));
  });

  it('rehydrates the current Arrange runtime graph after a sidecar restart', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: { session: { ...defaultSession(), workspace: 'arrange' } },
    });
    renderApp(fake);

    await waitForAppShell();
    fake.calls.splice(0);

    fake.emitRuntimeRestarted(2);

    await waitFor(() => expect(fake.calls).toContain('restoreSamplePads'));
    await waitFor(() => expect(fake.calls).toContain('syncArrangementRuntime'));
  });

  it('does not continue automatic recovery when Sample Pad restoration rejects', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: { session: { ...defaultSession(), workspace: 'play' } },
    });
    let failRecovery = false;
    fake.restoreSamplePadsStrict = async () => {
      fake.calls.push('restoreSamplePads');
      if (failRecovery) throw new Error('Sample Pad restore failed.');
      return fake.audio;
    };
    renderApp(fake);

    await waitForAppShell();
    const rackRestoreCount = fake.calls.filter((call) => call === 'restoreCurrentRack').length;
    fake.calls.splice(0);
    failRecovery = true;

    fake.emitRuntimeRestarted(2);

    await waitFor(() => expect(fake.calls).toContain('restoreSamplePads'));
    expect(fake.calls.filter((call) => call === 'restoreCurrentRack')).toHaveLength(0);
    expect(rackRestoreCount).toBe(0);
  });

  it('retries the latest runtime generation after a restart during recovery', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: { session: { ...defaultSession(), workspace: 'arrange' } },
    });
    let holdNextRecovery = false;
    let releaseFirstRecovery: () => void = () => undefined;
    let firstRecoveryGate: Promise<void> | null = null;
    const defaultRestoreSamplePads = fake.restoreSamplePadsStrict;
    fake.restoreSamplePadsStrict = async () => {
      if (holdNextRecovery) {
        holdNextRecovery = false;
        fake.calls.push('restoreSamplePads');
        await firstRecoveryGate;
        return fake.audio;
      }
      return defaultRestoreSamplePads();
    };
    renderApp(fake);

    await waitForAppShell();
    fake.calls.splice(0);
    firstRecoveryGate = new Promise<void>((resolve) => {
      releaseFirstRecovery = resolve;
    });
    holdNextRecovery = true;

    fake.emitRuntimeRestarted(2);
    await waitFor(() => expect(fake.calls).toContain('restoreSamplePads'));
    fake.emitRuntimeRestarted(3);
    releaseFirstRecovery();

    await waitFor(() =>
      expect(fake.calls.filter((call) => call === 'restoreSamplePads')).toHaveLength(2),
    );
    await waitFor(() => expect(fake.calls).toContain('syncArrangementRuntime'));
  });

  it('does not let a cancelled Play failure overwrite a newer playing state', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: { session: { ...defaultSession(), workspace: 'arrange' } },
    });
    const playRejectors: ((reason?: unknown) => void)[] = [];
    fake.playTimeline = () => {
      fake.calls.push('playTimeline');
      return new Promise<void>((_resolve, reject) => {
        playRejectors.push(reject);
      });
    };
    renderApp(fake);

    await waitForAppShell();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(playRejectors).toHaveLength(1));

    fake.emitTransportStatus({ state: 'playing' });
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Stop playback' })).toBeInTheDocument(),
    );

    await user.click(screen.getByRole('button', { name: 'Stop playback' }));
    await waitFor(() =>
      expect(fake.calls.filter((call) => call === 'stopTimeline')).toHaveLength(1),
    );

    await user.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(playRejectors).toHaveLength(2));
    fake.emitTransportStatus({ state: 'playing' });
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Stop playback' })).toBeInTheDocument(),
    );

    playRejectors[0](new Error('Cancelled old Play request.'));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Stop playback' })).toBeInTheDocument(),
    );
  });

  it('sends a new Play while an earlier Stop command is still pending', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: { session: { ...defaultSession(), workspace: 'arrange' } },
    });
    const playSequences: number[] = [];
    const stopSequences: number[] = [];
    let releaseStop: () => void = () => undefined;
    const stopGate = new Promise<void>((resolve) => {
      releaseStop = resolve;
    });
    const defaultPlayTimeline = fake.playTimeline;
    fake.playTimeline = async (sequence) => {
      playSequences.push(sequence);
      await defaultPlayTimeline(sequence);
    };
    fake.stopTimeline = async (sequence) => {
      stopSequences.push(sequence);
      fake.emitTransportStatus({ state: 'stopped' });
      await stopGate;
    };
    renderApp(fake);

    await waitForAppShell();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Stop playback' })).toBeInTheDocument(),
    );

    await user.click(screen.getByRole('button', { name: 'Stop playback' }));
    await waitFor(() => expect(stopSequences).toHaveLength(1));
    await user.click(screen.getByRole('button', { name: 'Play' }));

    await waitFor(() => expect(playSequences).toHaveLength(2));
    expect(playSequences[1]).toBeGreaterThan(stopSequences[0]);
    releaseStop();
  });

  it('sends a new Play while an earlier Go to Start command is still pending', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: { session: { ...defaultSession(), workspace: 'arrange' } },
    });
    const playSequences: number[] = [];
    const goToStartSequences: number[] = [];
    let releaseGoToStart: () => void = () => undefined;
    const goToStartGate = new Promise<void>((resolve) => {
      releaseGoToStart = resolve;
    });
    const defaultPlayTimeline = fake.playTimeline;
    fake.playTimeline = async (sequence) => {
      playSequences.push(sequence);
      await defaultPlayTimeline(sequence);
    };
    fake.goToStartTimeline = async (sequence) => {
      goToStartSequences.push(sequence);
      fake.emitTransportStatus({ state: 'stopped' });
      await goToStartGate;
    };
    renderApp(fake);

    await waitForAppShell();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Stop playback' })).toBeInTheDocument(),
    );

    await user.click(screen.getByRole('button', { name: 'Stop and go to start' }));
    await waitFor(() => expect(goToStartSequences).toHaveLength(1));
    await user.click(screen.getByRole('button', { name: 'Play' }));

    await waitFor(() => expect(playSequences).toHaveLength(2));
    expect(playSequences[1]).toBeGreaterThan(goToStartSequences[0]);
    expect(screen.getByRole('button', { name: 'Stop playback' })).toBeInTheDocument();
    releaseGoToStart();
  });

  it('re-engages emergency mute when the audio driver changes', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole('button', { name: /^MUTE$/ })).toBeInTheDocument());
    await waitFor(() => expect(fake.calls).toContain('probeAudioDevices'));

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Open Audio Settings/ })).toBeInTheDocument(),
    );
    await user.click(screen.getByRole('button', { name: /Open Audio Settings/ }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Sample rate' }), '96000');
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(fake.calls).toContain('setAudioDriver'));
    await waitFor(() => expect(screen.getByRole('button', { name: /UNMUTE/ })).toBeInTheDocument());
    expect(screen.getByText(/EMERGENCY MUTE ENGAGED/)).toBeInTheDocument();
  });

  it('keeps the emergency mute control reachable from every workspace', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();

    const user = userEvent.setup();
    const workspaceNav = screen.getByRole('navigation', { name: /Workspace/ });

    for (const label of ['Play', 'Arrange', 'Design']) {
      await user.click(within(workspaceNav).getByRole('button', { name: new RegExp(label) }));
      expect(screen.getByRole('button', { name: /^(MUTE|UNMUTE)$/ })).toBeInTheDocument();
    }
  });

  it('does not wedge later navigation after clicking the active workspace', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();

    const user = userEvent.setup();
    const workspaceNav = screen.getByRole('navigation', { name: /Workspace/ });
    await user.click(within(workspaceNav).getByRole('button', { name: /Arrange/ }));
    await user.click(within(workspaceNav).getByRole('button', { name: /Play/ }));

    await waitFor(() => expect(fake.calls).toContain('switchWorkspace'));
    await waitFor(() => expect(fake.bootstrapState.session.workspace).toBe('play'));
  });

  it('previews master gain during a gesture and persists it once at the end', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();

    const master = screen.getByRole('slider', { name: /Master volume/ });
    fireEvent.pointerDown(master);
    fireEvent.change(master, { target: { value: '-12' } });
    await waitFor(() => expect(fake.calls).toContain('previewMasterGainDb'));
    fireEvent.pointerUp(master, { target: { value: '-12' } });

    await waitFor(() => expect(screen.getByText('-12.0 dB')).toBeInTheDocument());
    await waitFor(() => {
      expect(fake.calls.filter((call) => call === 'setMasterGainDb')).toHaveLength(1);
    });
  });

  it('shows the feedback cause in the mute banner when feedback is suspected', async () => {
    const fake = new FakeNativeApi({
      audio: fakeAudioStatus({ state: 'muted', feedbackSuspected: true }),
    });
    renderApp(fake);
    await waitForAppShell();
    await waitFor(() => expect(fake.calls).toContain('getAudioStatus'));

    await waitFor(() =>
      expect(screen.getByText(/acoustic feedback suspected/i)).toBeInTheDocument(),
    );
  });

  it('keeps output safe when the device is faulted and recovers into emergency mute', async () => {
    const fake = new FakeNativeApi({
      audio: fakeAudioStatus({ state: 'faulted', message: 'Device disconnected.' }),
    });
    renderApp(fake);
    await waitForAppShell();

    expect(fake.audio.state).toBe('faulted');
    const user = userEvent.setup();

    await waitFor(() => expect(fake.calls).toContain('probeAudioDevices'));
    await user.click(screen.getByRole('button', { name: /Open Audio Settings/ }));
    await user.click(screen.getByRole('button', { name: /Recover Audio/ }));
    await waitFor(() => expect(fake.audio.state).toBe('muted'));
    expect(fake.calls).toContain('recoverAudioDevice');
  });

  it('persists a recording take through the injected api and surfaces it in the Inbox', async () => {
    const fake = new FakeNativeApi({ recordingSamples: 48_000 });
    renderApp(fake);
    await waitForAppShell();

    const user = userEvent.setup();
    const recordButton = screen.getByRole('button', { name: /Start recording/ });
    await user.click(recordButton);

    await waitFor(() => expect(fake.audio.recording.active).toBe(true));
    expect(fake.calls).toContain('startRecording');

    await user.click(screen.getByRole('button', { name: /Stop recording/ }));
    await waitFor(() => expect(fake.audio.recording.active).toBe(false));
    expect(fake.calls).toContain('stopRecording');
    expect(fake.recordings[0].state).toBe('completed');
    expect(fake.recordings[0].samplesWritten).toBe(48_000);

    await user.click(screen.getByRole('button', { name: /Recordings/ }));
    await waitFor(() => expect(screen.getByText(fake.recordings[0].name)).toBeInTheDocument());
  });

  it('does not lose the Scratch Session when the workspace switches', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();

    const workspaceNav = screen.getByRole('navigation', { name: /Workspace/ });
    const user = userEvent.setup();
    await user.click(within(workspaceNav).getByRole('button', { name: /Play/ }));

    // Capture snapshot A — this mutates session state from the Play workspace.
    await user.click(screen.getByRole('button', { name: '＋' }));
    await waitFor(() => expect(fake.calls).toContain('captureSnapshot'));

    await user.click(within(workspaceNav).getByRole('button', { name: /Arrange/ }));
    await user.click(within(workspaceNav).getByRole('button', { name: /Play/ }));

    // The captured snapshot must survive workspace switches and reach persistence.
    await waitFor(() => {
      expect(
        fake.savedSessions.some((session) =>
          session.snapshots.some((snapshot) => snapshot.id === 'snapshot:A'),
        ),
      ).toBe(true);
    });
  });

  it('previews a sample pad through React props, not DOM listeners', async () => {
    const fake = new FakeNativeApi({ recordingSamples: 48_000 });
    renderApp(fake);
    await waitForAppShell();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Start recording/ }));
    await waitFor(() => expect(fake.audio.recording.active).toBe(true));
    await user.click(screen.getByRole('button', { name: /Stop recording/ }));
    await waitFor(() => expect(fake.recordings.length).toBeGreaterThan(0));

    const workspaceNav = screen.getByRole('navigation', { name: /Workspace/ });
    await user.click(within(workspaceNav).getByRole('button', { name: /Design/ }));

    await user.click(screen.getByRole('button', { name: /Map to Pad/ }));
    const previewCallsBefore = fake.calls.filter((call) => call === 'previewAsset').length;

    await user.click(screen.getByRole('button', { name: /Preview Fake Take 1/ }));
    await waitFor(() => {
      expect(fake.calls.filter((call) => call === 'previewAsset').length).toBeGreaterThan(
        previewCallsBefore,
      );
    });
  });

  it('loads a VST3 into the rack through the injected api and projects it into the Scratch Session', async () => {
    const plugins: PluginEntry[] = [
      {
        id: 'plug:example',
        name: 'Example Synth',
        vendor: 'Acme',
        version: null,
        format: 'VST3',
        path: 'C:\\VST3\\example.vst3',
        bundle: false,
        modifiedAtMs: null,
        scanState: 'validated',
      },
      {
        id: 'plug:other',
        name: 'Other Synth',
        vendor: 'Acme',
        version: null,
        format: 'VST3',
        path: 'C:\\VST3\\other.vst3',
        bundle: false,
        modifiedAtMs: null,
        scanState: 'validated',
      },
    ];
    const fake = new FakeNativeApi({ plugins });
    renderApp(fake);
    await waitForAppShell();

    await waitFor(() => expect(fake.calls).toContain('scanVst3Folder'));
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));

    await waitFor(() => expect(fake.calls).toContain('loadPluginIntoRack'));
    await waitFor(() => {
      const saved = fake.savedSessions[fake.savedSessions.length - 1];
      const loaded = saved.rack.devices.find((device) => device.kind === 'plugin');
      expect(loaded).toBeDefined();
      expect(loaded?.path).toBe(plugins[0].path);
      expect(loaded?.name).toBe(plugins[0].name);
      expect(loaded?.bypassed).toBe(false);
      expect(loaded?.gainDb).toBe(0);
      expect(saved.rack.devices.filter((device) => device.kind === 'plugin')).toHaveLength(1);
      expect(saved.rack.devices.filter((device) => device.kind !== 'plugin')).toHaveLength(3);
    });

    // Loading a second plugin replaces the first and never stacks in the rack.
    await user.click(screen.getByRole('button', { name: /Other Synth/ }));
    await waitFor(() => {
      const saved = fake.savedSessions[fake.savedSessions.length - 1];
      const rackPlugins = saved.rack.devices.filter((device) => device.kind === 'plugin');
      expect(rackPlugins).toHaveLength(1);
      expect(rackPlugins[0].path).toBe(plugins[1].path);
      expect(rackPlugins[0].name).toBe(plugins[1].name);
      expect(saved.rack.devices.filter((device) => device.kind !== 'plugin')).toHaveLength(3);
    });
  });

  it('keeps the Scratch Session rack unchanged when a plugin load faults', async () => {
    const fake = new FakeNativeApi({ plugins: [examplePlugin], pluginLoadFaulted: true });
    renderApp(fake);
    await waitForAppShell();
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Example Synth/ })).toBeInTheDocument(),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));

    await waitFor(() => expect(fake.audio.state).toBe('faulted'));
    await waitFor(() => {
      expect(
        fake.bootstrapState.session.rack.devices.some((device) => device.kind === 'plugin'),
      ).toBe(false);
    });
  });

  it('toggles plugin bypass through the Play workspace and reflects it in the rack', async () => {
    const fake = new FakeNativeApi({ plugins: [examplePlugin] });
    renderApp(fake);
    await waitForAppShell();
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Example Synth/ })).toBeInTheDocument(),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));
    await waitFor(() => expect(fake.calls).toContain('loadPluginIntoRack'));

    const workspaceNav = screen.getByRole('navigation', { name: /Workspace/ });
    await user.click(within(workspaceNav).getByRole('button', { name: /Play/ }));

    await user.click(screen.getByRole('button', { name: /Bypass/ }));
    await waitFor(() => {
      const saved = fake.savedSessions[fake.savedSessions.length - 1];
      expect(saved.rack.devices.find((device) => device.kind === 'plugin')?.bypassed).toBe(true);
    });

    await user.click(screen.getByRole('button', { name: /Enable/ }));
    await waitFor(() => {
      const saved = fake.savedSessions[fake.savedSessions.length - 1];
      expect(saved.rack.devices.find((device) => device.kind === 'plugin')?.bypassed).toBe(false);
    });
  });

  it('opens the loaded plugin editor from the Play rack', async () => {
    const fake = new FakeNativeApi({ plugins: [examplePlugin] });
    renderApp(fake);
    await waitForAppShell();
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Example Synth/ })).toBeInTheDocument(),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));
    await waitFor(() => expect(fake.calls).toContain('loadPluginIntoRack'));
    await user.click(
      within(screen.getByRole('navigation', { name: /Workspace/ })).getByRole('button', {
        name: /Play/,
      }),
    );
    await user.click(screen.getByRole('button', { name: /Open Example Synth editor/ }));

    await waitFor(() => expect(fake.calls).toContain('openPluginEditor'));
  });

  it('applies an audio driver selection without changing the Scratch Session', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();
    await waitFor(() => expect(fake.calls).toContain('probeAudioDevices'));
    const savesBeforeSelection = fake.savedSessions.length;

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Open Audio Settings/ }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Sample rate' }), '96000');
    await user.click(screen.getByRole('button', { name: 'Apply' }));

    await waitFor(() => expect(fake.calls).toContain('setAudioDriver'));
    await waitFor(() => expect(fake.audio.sampleRate).toBe(96_000));
    expect(fake.audio.driver).toBe('Fake Driver');
    expect(fake.calls).not.toContain('switchWorkspace');
    expect(fake.bootstrapState.session.workspace).toBe('arrange');
    expect(fake.savedSessions).toHaveLength(savesBeforeSelection);
  });

  it('restores plugin parameters into the rack through the injected api', async () => {
    const bootSession = {
      ...defaultSession(),
      workspace: 'play' as const,
      rack: {
        ...defaultSession().rack,
        devices: [
          ...defaultSession().rack.devices,
          {
            id: 'plugin:example',
            name: 'Example Synth',
            kind: 'plugin' as const,
            path: examplePlugin.path,
            bypassed: false,
            gainDb: 0,
            parameterValues: [0.3, 0.7],
            disabledPlaceholder: false,
          },
        ],
      },
    };
    const fake = new FakeNativeApi({
      plugins: [examplePlugin],
      pluginParameters: [
        { index: 0, name: 'Cutoff', value: 0, defaultValue: 0, automatable: true },
        { index: 1, name: 'Resonance', value: 0, defaultValue: 0, automatable: true },
      ],
      bootstrapState: { session: bootSession },
    });
    renderApp(fake);
    await waitForAppShell();

    await waitFor(() => {
      const loaded = fake.bootstrapState.session.rack.devices.find(
        (device) => device.kind === 'plugin',
      );
      expect(loaded).toBeDefined();
      expect(loaded?.parameterValues).toEqual([0.3, 0.7]);
    });
  });
});
