// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { useState } from 'react';
import App from '@/app/App';
import { useRuntimeRestartNotification } from '@/app/runtime/useRuntimeRestartNotification';
import { defaultViewState } from '@/model/domain';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';

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

  it('keeps the CreativeSession unchanged while navigating workspaces', async () => {
    const api = new FakeNativeApi();
    const initialSession = api.bootstrapState.session;

    await renderApp(api);
    const navigation = within(screen.getByRole('navigation', { name: 'Workspace' }));
    await userEvent.click(navigation.getByRole('button', { name: /Design/ }));

    await waitFor(() => expect(api.calls).toContain('switchWorkspace'));
    expect(api.bootstrapState.session).toBe(initialSession);
    expect(api.bootstrapState.viewState.workspace).toBe('design');
  });

  it('shows a runtime restart notification without replaying session commands', async () => {
    const api = new FakeNativeApi();
    function RuntimeNotification() {
      const [message, setMessage] = useState('');
      useRuntimeRestartNotification({ api, setScanMessage: setMessage });
      return <output>{message}</output>;
    }
    render(<RuntimeNotification />);
    await waitFor(() => expect(api.calls).toContain('onRuntimeRestarted'));
    const callsBeforeRestart = [...api.calls];

    api.emitRuntimeRestarted(2);

    await waitFor(() => expect(screen.getByText(/Audio Runtime restarted/)).toBeInTheDocument());
    expect(api.calls.filter((call) => call === 'retryRuntimeProjection')).toHaveLength(0);
    expect(api.calls.filter((call) => call === 'restoreSamplePads')).toHaveLength(0);
    expect(api.calls.slice(0, callsBeforeRestart.length)).toEqual(callsBeforeRestart);
  });

  it('ignores a cancelled transport failure after a newer status event', async () => {
    const api = new FakeNativeApi({
      bootstrapState: { viewState: { ...defaultViewState(), workspace: 'arrange' } },
    });
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
