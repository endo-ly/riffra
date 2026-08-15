// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
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
      <output>{transport.transportPlaying ? 'transport-playing' : ''}</output>
    </>
  );
}

describe('useTransportController', () => {
  it('stops a timeline play request before the playing status arrives', async () => {
    const api = new FakeNativeApi();
    let playSequence = 0;
    let stopSequence = 0;
    api.setResponse('playTimeline', (sequence: unknown) => {
      playSequence = Number(sequence);
    });
    api.setResponse('stopTimeline', (sequence: unknown) => {
      stopSequence = Number(sequence);
    });
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('playTimeline'));
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));

    await waitFor(() => expect(api.calls).toContain('stopTimeline'));
    expect(stopSequence).toBeGreaterThan(playSequence);
  });

  it('moves a timeline play request to the start before the playing status arrives', async () => {
    const api = new FakeNativeApi();
    let playSequence = 0;
    let startSequence = 0;
    api.setResponse('playTimeline', (sequence: unknown) => {
      playSequence = Number(sequence);
    });
    api.setResponse('goToStartTimeline', (sequence: unknown) => {
      startSequence = Number(sequence);
    });
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('playTimeline'));
    fireEvent.click(screen.getByRole('button', { name: 'Go to Start' }));

    await waitFor(() => expect(api.calls).toContain('goToStartTimeline'));
    expect(startSequence).toBeGreaterThan(playSequence);
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
