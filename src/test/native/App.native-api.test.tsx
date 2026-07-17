// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { defaultSession } from '@/lib/domain';
import type { BackgroundJobStatus, PluginEntry, RenderResult } from '@/lib/domain';
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
  it('boots muted and toggles emergency mute through the injected api', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);

    await waitForAppShell();
    expect(screen.getByRole('button', { name: /UNMUTE/ })).toBeInTheDocument();
    expect(fake.calls).toContain('bootstrap');
    expect(fake.calls).toContain('getAudioStatus');

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /UNMUTE/ }));

    await waitFor(() => expect(screen.getByRole('button', { name: /^MUTE$/ })).toBeInTheDocument());
    expect(fake.calls).toContain('setEmergencyMute');
    expect(fake.audio.state).toBe('ready');

    await user.click(screen.getByRole('button', { name: /^MUTE$/ }));
    await waitFor(() => expect(screen.getByRole('button', { name: /UNMUTE/ })).toBeInTheDocument());
    expect(fake.audio.state).toBe('muted');
  });

  it('re-engages emergency mute when the audio driver changes', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /UNMUTE/ }));
    await waitFor(() => expect(screen.getByRole('button', { name: /^MUTE$/ })).toBeInTheDocument());

    await user.click(screen.getByRole('button', { name: /96,000 Hz/ }));
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
      expect(screen.getByRole('button', { name: /UNMUTE/ })).toBeInTheDocument();
    }
  });

  it('cancels a render without promoting a result and allows a clean retry', async () => {
    const session = defaultSession();
    session.arrangement.audioClips = [
      {
        id: 'clip:render',
        assetId: 'asset:source',
        name: 'Render source',
        trackId: 'main',
        positionMs: 0,
        durationMs: 1_000,
        sourceStartMs: 0,
        sourceEndMs: 0,
        loopEnabled: false,
        gainDb: 0,
        fadeInMs: 0,
        fadeOutMs: 0,
        pan: 0,
        muted: false,
      },
    ];
    const fake = new FakeNativeApi({ bootstrapState: { session } });
    let status: BackgroundJobStatus = {
      id: 'job:render:cancel',
      kind: 'render',
      state: 'queued',
      progress: 0,
      message: 'Render queued.',
      result: null,
    };
    const completed: RenderResult = {
      assetId: 'asset:render-retry',
      path: 'fake://retry.wav',
      sampleRate: 48_000,
      frames: 48_000,
      durationMs: 1_000,
      clipCount: 1,
      rangeStartMs: 0,
      rangeEndMs: 1_000,
      normalized: false,
      trackId: null,
      state: 'completed',
      message: 'Retry completed.',
    };
    let starts = 0;
    fake.startRenderJob = vi.fn(async () => {
      starts += 1;
      return starts === 1
        ? status
        : {
            ...status,
            id: 'job:render:retry',
            state: 'completed',
            progress: 1,
            message: 'Retry completed.',
            result: completed,
          };
    });
    fake.getBackgroundJob = vi.fn(async () => status);
    fake.cancelBackgroundJob = vi.fn(async () => {
      status = {
        ...status,
        state: 'cancelled',
        message: 'Render cancelled; no partial result was promoted.',
        result: null,
      };
      return status;
    });
    renderApp(fake);
    await waitForAppShell();
    const user = userEvent.setup();
    const workspaceNav = screen.getByRole('navigation', { name: /Workspace/ });
    await user.click(within(workspaceNav).getByRole('button', { name: /Arrange/ }));
    const renderButton = screen.getByRole('button', { name: /Render WAV/ });
    expect(renderButton.closest('.workspace-scroll')).toBeInTheDocument();
    await user.click(renderButton);
    await waitFor(() => expect(screen.getByRole('button', { name: /Cancel/ })).toBeInTheDocument());

    await user.click(screen.getByRole('button', { name: /Cancel/ }));
    await waitFor(() =>
      expect(screen.getByText(/Render failed: Render cancelled/)).toBeInTheDocument(),
    );
    expect(screen.queryByText('fake://retry.wav')).not.toBeInTheDocument();

    await user.click(renderButton);
    await waitFor(() => expect(screen.getByText('fake://retry.wav')).toBeInTheDocument());
    expect(fake.startRenderJob).toHaveBeenCalledTimes(2);
  });

  it('shows the feedback cause in the mute banner when feedback is suspected', async () => {
    const fake = new FakeNativeApi({
      audio: fakeAudioStatus({ state: 'muted', feedbackSuspected: true }),
    });
    renderApp(fake);
    await waitForAppShell();

    expect(screen.getByText(/acoustic feedback suspected/i)).toBeInTheDocument();
  });

  it('keeps output safe when the device is faulted and recovers into emergency mute', async () => {
    const fake = new FakeNativeApi({
      audio: fakeAudioStatus({ state: 'faulted', message: 'Device disconnected.' }),
    });
    renderApp(fake);
    await waitForAppShell();

    expect(fake.audio.state).toBe('faulted');
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: /Recover device/ }));
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
    const sessionNote = screen.getByPlaceholderText(/意図、比較対象/);
    await user.type(sessionNote, 'remember this tone');
    expect((sessionNote as HTMLTextAreaElement).value).toContain('remember this tone');

    await user.click(within(workspaceNav).getByRole('button', { name: /Home/ }));
    await user.click(within(workspaceNav).getByRole('button', { name: /Play/ }));
    expect((screen.getByPlaceholderText(/意図、比較対象/) as HTMLTextAreaElement).value).toContain(
      'remember this tone',
    );

    await waitFor(() => {
      expect(
        fake.savedSessions.some((session) => session.settings.note.includes('remember this tone')),
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

    expect(fake.calls).toContain('scanVst3Folder');
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

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));

    await waitFor(() => expect(fake.audio.state).toBe('faulted'));
    await waitFor(() => {
      const saved = fake.savedSessions[fake.savedSessions.length - 1];
      expect(saved.rack.devices.some((device) => device.kind === 'plugin')).toBe(false);
    });
  });

  it('toggles plugin bypass through the Play workspace and reflects it in the rack', async () => {
    const fake = new FakeNativeApi({ plugins: [examplePlugin] });
    renderApp(fake);
    await waitForAppShell();

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

  it('applies an audio driver selection without changing the Scratch Session', async () => {
    const fake = new FakeNativeApi();
    renderApp(fake);
    await waitForAppShell();
    const savesBeforeSelection = fake.savedSessions.length;

    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /96,000 Hz/ }));

    await waitFor(() => expect(fake.calls).toContain('setAudioDriver'));
    await waitFor(() => expect(fake.audio.sampleRate).toBe(96_000));
    expect(fake.audio.driver).toBe('Fake Driver');
    expect(fake.savedSessions).toHaveLength(savesBeforeSelection);
  });

  it('restores plugin parameters into the rack through the injected api', async () => {
    const bootSession = {
      ...defaultSession(),
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
            stateData: null,
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
      const saved = fake.savedSessions[fake.savedSessions.length - 1];
      const loaded = saved.rack.devices.find((device) => device.kind === 'plugin');
      expect(loaded).toBeDefined();
      expect(loaded?.parameterValues).toEqual([0.3, 0.7]);
    });
  });
});
