// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import App from '@/app/App';
import { useRuntimeRestartNotification } from '@/app/runtime/useRuntimeRestartNotification';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';
import { ToastStack } from '@/shared/ui/ToastStack';

afterEach(cleanup);

async function renderApp(api: FakeNativeApi) {
  render(<App api={api} />);
  await waitFor(() => expect(screen.getByRole('main')).toBeInTheDocument());
}

describe('App native boundary', () => {
  it('boots once and delegates emergency mute to the native host', async () => {
    const api = new FakeNativeApi();

    await renderApp(api);
    await userEvent.click(screen.getByRole('button', { name: /^MUTE$/ }));

    await waitFor(() => expect(api.emergencyMuteRequests).toEqual([true]));
    expect(api.calls.filter((call) => call === 'bootstrap')).toHaveLength(1);
    expect(screen.getByRole('button', { name: /UNMUTE/ })).toBeInTheDocument();
  });

  it('shows a runtime restart notification without replaying session commands', async () => {
    const api = new FakeNativeApi();
    function RuntimeNotification() {
      useRuntimeRestartNotification({ api });
      return <ToastStack />;
    }
    render(<RuntimeNotification />);
    await waitFor(() => expect(api.calls).toContain('onRuntimeRestarted'));
    const callsBeforeRestart = [...api.calls];

    api.emitRuntimeRestarted(2);

    await waitFor(() => expect(screen.getByText(/Audio Runtime restarted/)).toBeInTheDocument());
    expect(api.calls.filter((call) => call === 'retryRuntimeProjection')).toHaveLength(0);
    expect(api.calls.slice(0, callsBeforeRestart.length)).toEqual(callsBeforeRestart);
  });

  it('waits for runtime startup before starting the VST scan', async () => {
    const api = new FakeNativeApi({
      bootstrapState: { runtimeStarted: false, runtimeStartupFinished: false },
    });

    await renderApp(api);
    expect(api.calls).not.toContain('startScanJob');

    api.emitRuntimeStartupFinished(true);

    await waitFor(() => expect(api.calls).toContain('startScanJob'));
  });

  it('retries runtime restoration once after a successful startup scan', async () => {
    const api = new FakeNativeApi({
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
    const api = new FakeNativeApi();
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
    api.emitTransportStatus({ state: 'playing', sequence: 2 });
    rejectPlay(new Error('Cancelled previous transport request.'));

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Stop playback' })).toBeInTheDocument(),
    );
  });

  it('surfaces the native feedback cause in the global safety control', async () => {
    const api = new FakeNativeApi({
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
});
