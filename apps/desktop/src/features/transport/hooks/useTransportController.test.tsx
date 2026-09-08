// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useRef } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi } from '@/native/native-api-fake';
import { useTransportController } from './useTransportController';

afterEach(cleanup);

function Harness({ api }: { api: FakeNativeApi }) {
  const sessionRef = useRef(defaultSession());
  const transport = useTransportController({ api, sessionRef });
  return (
    <>
      <button onClick={() => void transport.playTransport()}>Play</button>
      <button onClick={() => void transport.stopTransport()}>Stop</button>
      <button onClick={() => void transport.goToStart()}>Go to Start</button>
      <output>
        {transport.transportStarting
          ? 'transport-starting'
          : transport.transportPlaying
            ? 'transport-playing'
            : ''}
      </output>
    </>
  );
}

describe('useTransportController', () => {
  it('reports Starting until the native Playing status arrives', async () => {
    const api = new FakeNativeApi();
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('playTimeline'));

    act(() => api.emitTransportStatus({ state: 'starting' }));
    expect(screen.getByText('transport-starting')).toBeInTheDocument();

    act(() => api.emitTransportStatus({ state: 'playing' }));
    await waitFor(() => expect(screen.getByText('transport-playing')).toBeInTheDocument());
  });

  it('stops a timeline play request before the playing status arrives', async () => {
    const api = new FakeNativeApi();
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('playTimeline'));
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));

    await waitFor(() => expect(api.calls).toContain('stopTimeline'));
    expect(api.calls.filter((call) => call === 'playTimeline')).toHaveLength(1);
    expect(api.calls.filter((call) => call === 'stopTimeline')).toHaveLength(1);
  });

  it('moves a timeline play request to the start before the playing status arrives', async () => {
    const api = new FakeNativeApi();
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('playTimeline'));
    fireEvent.click(screen.getByRole('button', { name: 'Go to Start' }));

    await waitFor(() => expect(api.calls).toContain('goToStartTimeline'));
    expect(api.calls.filter((call) => call === 'playTimeline')).toHaveLength(1);
    expect(api.calls.filter((call) => call === 'goToStartTimeline')).toHaveLength(1);
  });

  it('starts a newer Play intent while Stop is still pending', async () => {
    const api = new FakeNativeApi();
    const stop: { resolve: () => void } = { resolve: () => undefined };
    api.setResponse(
      'stopTimeline',
      () =>
        new Promise<void>((resolve) => {
          stop.resolve = resolve;
        }),
    );
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() =>
      expect(api.calls.filter((call) => call === 'playTimeline')).toHaveLength(1),
    );
    api.emitTransportStatus({ state: 'playing' });
    await waitFor(() => expect(screen.getByText('transport-playing')).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    await waitFor(() => expect(api.calls).toContain('stopTimeline'));

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() =>
      expect(api.calls.filter((call) => call === 'playTimeline')).toHaveLength(2),
    );

    stop.resolve();
  });
});
