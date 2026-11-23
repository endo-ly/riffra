// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '@/app/App';
import { createSessionStore } from '@/session/store';
import { bindSessionRuntime } from '@/session/runtime';
import {
  createFakeNativeApi,
  type FakeNativeApi,
  fakeAudioStatus,
} from '@/test/support/fake-native-api';
import { defaultSession } from '@/native/preview/fixtures';
import { ToastStack } from '@/shared/ui/message/ToastStack';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
});

async function renderApp(api: FakeNativeApi) {
  render(<App api={api} />);
  await waitFor(() => expect(screen.getByRole('main')).toBeInTheDocument());
}

describe('App native boundary', () => {
  it('boots once and delegates emergency mute to the native host', async () => {
    const api = createFakeNativeApi();

    await renderApp(api);
    expect(
      screen.getByRole('button', { name: 'Play' }).closest('[data-global-control-bar]'),
    ).toBeInTheDocument();
    const emergencyMuteButton = screen.getByRole('button', { name: /^MUTE$/ });
    expect(emergencyMuteButton).toHaveAttribute('aria-pressed', 'false');
    await userEvent.click(emergencyMuteButton);

    await waitFor(() => expect(api.emergencyMuteRequests).toEqual([true]));
    expect(api.calls.filter((call) => call === 'bootstrap')).toHaveLength(1);
    expect(screen.getByRole('button', { name: /UNMUTE/ })).toHaveAttribute('aria-pressed', 'true');
  });

  it('pauses only the work area while instruments load', async () => {
    // Arrange
    const api = createFakeNativeApi();
    await renderApp(api);
    const workArea = document.querySelector('[data-app-body]');

    // Act
    act(() =>
      api.emitRuntimeProjectionStatus({
        ...api.runtimeProjection,
        state: 'preparing',
        loadProgress: {
          operationId: 1,
          stage: {
            phase: 'creating',
            ownerId: '',
            deviceId: 'instrument-1',
            pluginPath: 'C:\\Plugins\\Keys.vst3',
            completedDevices: 1,
            totalDevices: 4,
          },
          elapsedMs: 2000,
          phaseElapsedMs: 1000,
          stalled: false,
        },
      }),
    );

    // Assert
    expect(await screen.findByRole('status', { name: 'Loading instruments' })).toHaveTextContent(
      '1 / 4',
    );
    expect(workArea).toHaveAttribute('inert');
    expect(screen.getByRole('button', { name: /^MUTE$/ })).toBeEnabled();

    // Act
    act(() =>
      api.emitRuntimeProjectionStatus({
        ...api.runtimeProjection,
        state: 'active',
        loadProgress: undefined,
      }),
    );

    // Assert
    await waitFor(() => expect(workArea).not.toHaveAttribute('inert'));
    expect(screen.queryByRole('status', { name: 'Loading instruments' })).not.toBeInTheDocument();
  });

  it('shows a Bootstrap failure and returns to the app after Retry', async () => {
    const api = createFakeNativeApi({
      failures: { bootstrap: new Error('Host bootstrap failed') },
    });
    render(<App api={api} />);

    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('Host bootstrap failed'),
    );
    expect(screen.getByRole('button', { name: 'Retry' })).toBeEnabled();

    api.setFailure('bootstrap', null);
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }));

    await waitFor(() => expect(screen.getByRole('main')).toBeInTheDocument());
    expect(api.calls.filter((call) => call === 'bootstrap')).toHaveLength(2);
  });

  it('supports both left column resize handles from the keyboard', async () => {
    const api = createFakeNativeApi();
    await renderApp(api);

    const leftColumnHandle = screen.getByRole('separator', {
      name: 'Resize or collapse left column',
    });
    fireEvent.keyDown(leftColumnHandle, { key: 'ArrowRight' });
    expect(leftColumnHandle).toHaveAttribute('aria-valuenow', '288');

    const propertiesHandle = screen.getByRole('separator', {
      name: 'Resize Browser and Properties',
    });
    fireEvent.keyDown(propertiesHandle, { key: 'ArrowUp' });
    expect(propertiesHandle).toHaveAttribute('aria-valuenow', '328');
  });

  it('removes a collapsed left column from the interaction tree', async () => {
    const api = createFakeNativeApi();
    await renderApp(api);

    const leftColumn = screen.getByRole('complementary', { name: 'Left column' });
    const leftColumnHandle = screen.getByRole('separator', {
      name: 'Resize or collapse left column',
    });
    for (let step = 0; step < 29; step += 1) {
      fireEvent.keyDown(leftColumnHandle, { key: 'ArrowLeft' });
    }

    expect(leftColumnHandle).toHaveAttribute('aria-valuenow', '0');
    expect(leftColumn).toHaveAttribute('aria-hidden', 'true');
    expect(leftColumn).toHaveAttribute('inert');
  });

  it('shows a runtime restart notification without replaying session commands', async () => {
    const api = createFakeNativeApi();
    const stop = bindSessionRuntime(createSessionStore(api), api);
    render(<ToastStack />);
    await waitFor(() => expect(api.calls).toContain('onRuntimeRestarted'));
    const callsBeforeRestart = [...api.calls];

    api.emitRuntimeRestarted(2);

    await waitFor(() => expect(screen.getByText('Audio engine restarted')).toBeInTheDocument());
    expect(api.calls.filter((call) => call === 'retryRuntimeProjection')).toHaveLength(0);
    expect(api.calls.slice(0, callsBeforeRestart.length)).toEqual(callsBeforeRestart);
    stop();
  });

  it('waits for runtime startup before starting the VST scan', async () => {
    const api = createFakeNativeApi({
      bootstrapState: { runtimeStarted: false, runtimeStartupFinished: false },
    });

    await renderApp(api);
    expect(api.calls).not.toContain('startScanJob');

    api.emitRuntimeStartupFinished(true);

    await waitFor(() => expect(api.calls).toContain('startScanJob'));
  });

  it('retries runtime restoration once after a successful startup scan', async () => {
    const api = createFakeNativeApi({
      bootstrapState: { runtimeStarted: false, runtimeStartupFinished: false },
    });

    await renderApp(api);
    api.emitRuntimeStartupFinished(false);

    await waitFor(() => expect(api.calls).toContain('startScanJob'));
    await waitFor(() =>
      expect(api.calls.filter((call) => call === 'retryStartupRuntime')).toHaveLength(1),
    );

    api.emitRuntimeStartupFinished(false);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(api.calls.filter((call) => call === 'retryStartupRuntime')).toHaveLength(1);
  });

  it('ignores a cancelled transport failure after a newer status event', async () => {
    const api = createFakeNativeApi();
    let rejectPlay: (reason?: unknown) => void = () => undefined;
    api.setResponse(
      'playTimeline',
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectPlay = reject;
        }),
    );
    await renderApp(api);

    await userEvent.click(screen.getByRole('button', { name: 'Play' }));
    api.emitTransportStatus({ state: 'playing', appliedCommandSequence: 2 });
    rejectPlay(new Error('Cancelled previous transport request.'));

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Stop playback' })).toBeInTheDocument(),
    );
  });

  it('keeps the newest canonical state when events arrive out of order', async () => {
    const api = createFakeNativeApi();
    await renderApp(api);
    const newer = { ...defaultSession(), projectName: 'Newest' };
    newer.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:newest',
      name: 'Newest Track',
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
    const older = { ...defaultSession(), projectName: 'Older' };
    older.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:older',
      name: 'Older Track',
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

    api.emitCanonicalStateChanged({
      projectId: api.bootstrapState.projectState.activeProjectId,
      session: newer,
      sequence: 2,
      history: { canUndo: true, canRedo: false },
    });
    api.emitCanonicalStateChanged({
      projectId: api.bootstrapState.projectState.activeProjectId,
      session: older,
      sequence: 1,
      history: { canUndo: false, canRedo: false },
    });

    await waitFor(() => expect(screen.getByText('Newest Track')).toBeInTheDocument());
    expect(screen.queryByText('Older Track')).not.toBeInTheDocument();
  });

  it('surfaces the native feedback cause in the global safety control', async () => {
    const api = createFakeNativeApi({
      audio: fakeAudioStatus({
        state: 'muted',
        feedbackSuspected: true,
        message: 'Feedback suspected near the input monitor.',
      }),
    });

    await renderApp(api);
    await waitFor(() => expect(api.calls).toContain('onAudioStatus'));
    api.emitAudioStatus(api.audio);

    await waitFor(() =>
      expect(screen.getByText(/Feedback suspected near the input monitor/)).toBeInTheDocument(),
    );
    expect(screen.getByRole('button', { name: /UNMUTE/ })).toBeInTheDocument();
  });

  it('offers the checked application update again after Later and pauses editing while it installs', async () => {
    const api = createFakeNativeApi({ responses: { checkForAppUpdate: '9.9.9' } });

    await renderApp(api);

    expect(screen.getByText(/Riffra 9\.9\.9 can be installed/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Later' }));
    expect(screen.queryByText(/Riffra 9\.9\.9 can be installed/)).not.toBeInTheDocument();

    await userEvent.keyboard('{Control>}k{/Control}');
    await userEvent.click(await screen.findByText('Check for Updates'));
    expect(await screen.findByText(/Riffra 9\.9\.9 can be installed/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Update & restart' }));

    await waitFor(() => expect(api.calls).toContain('installAppUpdate'));
    expect(await screen.findByText('Installing update')).toBeInTheDocument();
  });

  it('stays silent when the startup check fails and reports a failed manual check', async () => {
    const api = createFakeNativeApi({ failures: { checkForAppUpdate: new Error('offline') } });
    const appUpdate = () => createSessionStore(api).getState().appUpdate;
    render(<ToastStack />);

    await act(() => appUpdate().checkOnStartup());
    expect(screen.queryByText('Update check failed')).not.toBeInTheDocument();

    await act(() => appUpdate().checkNow());

    expect(screen.getByText('Update check failed')).toBeInTheDocument();
  });
});
