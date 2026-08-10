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
    expect(fake.calls.filter((call) => call === 'bootstrap')).toHaveLength(1);
    await waitFor(() => expect(fake.calls).toContain('getAudioStatus'));
    expect(fake.calls).not.toContain('restoreSamplePads');
    expect(fake.calls).not.toContain('setEmergencyMute');
    expect(fake.bootstrapState.session.workspace).toBe('arrange');
    expect(
      within(screen.getByRole('navigation', { name: /Workspace/ })).getAllByRole('button'),
    ).toHaveLength(2);

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /^MUTE$/ }));
    await waitFor(() => expect(screen.getByRole('button', { name: /UNMUTE/ })).toBeInTheDocument());
    expect(fake.audio.state).toBe('muted');
  });

  it('uses the cached catalog and waits for Session audio graph restoration before scanning', async () => {
    // Arrange
    const fake = new FakeNativeApi({
      audio: fakeAudioStatus(),
      plugins: [examplePlugin],
      bootstrapState: {
        pluginCatalog: [examplePlugin],
        runtimeStarted: false,
        runtimeStartupFinished: false,
      },
    });

    // Act
    renderApp(fake);

    // Assert
    await waitForAppShell();
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Example Synth/ })).toBeInTheDocument(),
    );
    await new Promise((resolve) => window.setTimeout(resolve, 200));
    expect(fake.calls).not.toContain('startScanJob');

    fake.emitRuntimeStartupFinished();

    await waitFor(() => expect(fake.calls).toContain('startScanJob'));
  });

  it('retries runtime restoration once after the catalog scan repairs startup state', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: {
        runtimeStarted: false,
        runtimeStartupFinished: false,
      },
    });
    renderApp(fake);

    await waitForAppShell();
    await new Promise((resolve) => window.setTimeout(resolve, 200));
    expect(fake.calls).not.toContain('startScanJob');

    fake.emitRuntimeStartupFinished();

    await waitFor(() => expect(fake.calls).toContain('startScanJob'));
    await waitFor(() => expect(fake.calls).toContain('retryStartupRuntime'));
    expect(fake.calls.filter((call) => call === 'retryStartupRuntime')).toHaveLength(1);
    expect(fake.calls).not.toContain('recoverAudioDevice');
    expect(fake.calls.indexOf('startScanJob')).toBeLessThan(
      fake.calls.indexOf('retryStartupRuntime'),
    );
    expect(fake.bootstrapState.runtimeStarted).toBe(true);
  });

  it('does not retry runtime restoration when the catalog scan fails', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: {
        runtimeStarted: false,
        runtimeStartupFinished: false,
      },
    });
    fake.startScanJob = async () => {
      fake.calls.push('startScanJob');
      return {
        kind: 'scan',
        id: 'fake-scan-failure',
        state: 'failed',
        progress: 1,
        message: 'Fake scan failed.',
        result: null,
      };
    };
    renderApp(fake);

    await waitForAppShell();
    fake.emitRuntimeStartupFinished(false);

    await waitFor(() => expect(fake.calls).toContain('startScanJob'));
    await new Promise((resolve) => window.setTimeout(resolve, 100));
    expect(fake.calls).not.toContain('recoverAudioDevice');
    expect(fake.calls).not.toContain('retryStartupRuntime');
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
    expect(within(workspaceNav).getAllByRole('button')).toHaveLength(2);
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

  it('does not restore the rack runtime after a sidecar restart in Design', async () => {
    const fake = new FakeNativeApi({
      bootstrapState: { session: { ...defaultSession(), workspace: 'design' } },
    });
    renderApp(fake);

    await waitForAppShell();
    fake.calls.splice(0);

    fake.emitRuntimeRestarted(2);

    await waitFor(() => expect(fake.calls).toContain('restoreSamplePads'));
    expect(fake.calls).not.toContain('syncArrangementRuntime');
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
      bootstrapState: { session: { ...defaultSession(), workspace: 'arrange' } },
    });
    let failRecovery = false;
    fake.restoreSamplePadsStrict = async () => {
      fake.calls.push('restoreSamplePads');
      if (failRecovery) throw new Error('Sample Pad restore failed.');
      return fake.audio;
    };
    renderApp(fake);

    await waitForAppShell();
    fake.calls.splice(0);
    failRecovery = true;

    fake.emitRuntimeRestarted(2);

    await waitFor(() => expect(fake.calls).toContain('restoreSamplePads'));
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

  it('does not let a cancelled playback failure overwrite a newer playing state', async () => {
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

    for (const label of ['Arrange', 'Design']) {
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
    await user.click(within(workspaceNav).getByRole('button', { name: /Design/ }));

    await waitFor(() => expect(fake.calls).toContain('switchWorkspace'));
    await waitFor(() => expect(fake.bootstrapState.session.workspace).toBe('design'));
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
    await user.click(within(workspaceNav).getByRole('button', { name: /Design/ }));
    await waitFor(() => expect(fake.bootstrapState.session.workspace).toBe('design'));
    await user.click(within(workspaceNav).getByRole('button', { name: /Arrange/ }));
    await waitFor(() => expect(fake.bootstrapState.session.workspace).toBe('arrange'));
    expect(fake.bootstrapState.session.arrangement.tracks).toHaveLength(0);
    expect(fake.bootstrapState.session.rack.devices).toEqual(defaultSession().rack.devices);
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

  it('adds a scanned plugin to the focused Instrument Track', async () => {
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
    await user.click(screen.getByRole('button', { name: /Add Instrument Track/ }));
    await waitFor(() => expect(screen.getByText('Instrument 1')).toBeInTheDocument());
    await user.click(screen.getByText('Instrument 1'));
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));

    await waitFor(() => expect(fake.calls).toContain('setTrackInstrument'));
    await waitFor(() => {
      const saved = fake.savedSessions[fake.savedSessions.length - 1];
      const loaded = saved.arrangement.tracks[0]?.instrument;
      expect(loaded).toBeDefined();
      expect(loaded?.path).toBe(plugins[0].path);
      expect(loaded?.name).toBe(plugins[0].name);
    });

    await user.click(screen.getByRole('button', { name: /Other Synth/ }));
    await waitFor(() => {
      const saved = fake.savedSessions[fake.savedSessions.length - 1];
      expect(saved.arrangement.tracks[0]?.instrument?.path).toBe(plugins[1].path);
    });
  });

  it('requires a focused Track before adding a plugin', async () => {
    const fake = new FakeNativeApi({ plugins: [examplePlugin] });
    renderApp(fake);
    await waitForAppShell();
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Example Synth/ })).toBeInTheDocument(),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));
    expect(screen.getByText('Select a Track before adding a Plugin.')).toBeInTheDocument();
    expect(fake.calls).not.toContain('setTrackInstrument');
  });

  it('plays the focused Instrument Track from the Arrange lower panel', async () => {
    const fake = new FakeNativeApi({ plugins: [examplePlugin] });
    renderApp(fake);
    await waitForAppShell();
    await waitFor(() => expect(fake.calls).toContain('scanVst3Folder'));

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Add Instrument Track/ }));
    await waitFor(() => expect(screen.getByText('Instrument 1')).toBeInTheDocument());
    await user.click(screen.getByText('Instrument 1'));
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));
    await waitFor(() => expect(fake.calls).toContain('setTrackInstrument'));

    const savedSessionCount = fake.savedSessions.length;
    expect(fake.bootstrapState.session.arrangement.tracks[0]?.armed).toBe(false);
    await user.click(screen.getByLabelText('Instrument 1 track menu'));
    await user.click(screen.getByRole('button', { name: 'Open Play Surface' }));
    expect(screen.getByText('Live input only')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Computer Keyboard: Off' })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Computer Keyboard: Off' }));
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });
    await waitFor(() => {
      expect(fake.calls.filter((call) => call === 'sendMidiToTrack')).toHaveLength(2);
    });
    expect(fake.savedSessions).toHaveLength(savedSessionCount);

    const sentBeforeLibraryInput = fake.calls.filter((call) => call === 'sendMidiToTrack').length;
    const librarySearch = screen.getByLabelText('Library search');
    fireEvent.keyDown(librarySearch, { key: 'd' });
    fireEvent.keyUp(librarySearch, { key: 'd' });
    expect(fake.calls.filter((call) => call === 'sendMidiToTrack')).toHaveLength(
      sentBeforeLibraryInput,
    );

    const lowerPanelResize = screen.getByRole('button', { name: 'Resize lower panel' });
    fireEvent.pointerDown(lowerPanelResize, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(window, { clientY: 520, pointerId: 1 });
    fireEvent.pointerUp(window, { pointerId: 1 });
    fireEvent.keyDown(window, { key: 's' });
    fireEvent.keyUp(window, { key: 's' });
    await waitFor(() => {
      expect(fake.calls.filter((call) => call === 'sendMidiToTrack')).toHaveLength(4);
    });
    const collapsedResize = screen.getByRole('button', { name: 'Resize lower panel' });
    fireEvent.pointerDown(collapsedResize, { clientY: 520, pointerId: 2 });
    fireEvent.pointerMove(window, { clientY: 300, pointerId: 2 });
    fireEvent.pointerUp(window, { pointerId: 2 });
    expect(screen.queryByRole('button', { name: 'Stop Notes' })).not.toBeInTheDocument();
    expect(fake.savedSessions).toHaveLength(savedSessionCount);
  });

  it('adds an effect to a selected Audio Track from the Library', async () => {
    // Arrange
    const fake = new FakeNativeApi({ plugins: [examplePlugin] });
    renderApp(fake);
    await waitForAppShell();
    await waitFor(() => expect(fake.calls).toContain('scanVst3Folder'));
    const user = userEvent.setup();

    // Act
    await user.click(screen.getByRole('button', { name: /Add Audio Track/ }));
    await waitFor(() => expect(screen.getByText('Audio 1')).toBeInTheDocument());
    await user.click(screen.getByText('Audio 1'));
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));

    // Assert
    await waitFor(() => expect(fake.calls).toContain('addTrackEffect'));
    expect(fake.bootstrapState.session.arrangement.tracks[0]?.rack.devices).toHaveLength(1);
  });

  it('reopens collapsed side panels with their keyboard expansion direction', async () => {
    // Arrange
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();
    const libraryHandle = screen.getByRole('separator', {
      name: 'Resize or collapse library panel',
    });
    const inspectorHandle = screen.getByRole('separator', {
      name: 'Resize or collapse inspector panel',
    });

    // Act
    fireEvent.pointerDown(libraryHandle, { button: 0, clientX: 220, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 0, pointerId: 1 });
    fireEvent.pointerUp(window, { pointerId: 1 });
    fireEvent.keyDown(libraryHandle, { key: 'ArrowRight' });

    fireEvent.pointerDown(inspectorHandle, { button: 0, clientX: 280, pointerId: 2 });
    fireEvent.pointerMove(window, { clientX: 600, pointerId: 2 });
    fireEvent.pointerUp(window, { pointerId: 2 });
    fireEvent.keyDown(inspectorHandle, { key: 'ArrowLeft' });

    // Assert
    expect(libraryHandle).toHaveAttribute('aria-valuenow', '176');
    expect(inspectorHandle).toHaveAttribute('aria-valuenow', '220');
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
});
