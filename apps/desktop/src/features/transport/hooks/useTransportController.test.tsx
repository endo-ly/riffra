// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useRef, useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { defaultViewState } from '@/app/view-state';
import { defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi } from '@/native/native-api-fake';
import { useTransportController } from './useTransportController';

afterEach(cleanup);

function Harness({
  api,
  startWorkspace = 'arrange',
}: {
  api: FakeNativeApi;
  startWorkspace?: 'arrange' | 'design';
}) {
  const [workspace, setWorkspace] = useState<'arrange' | 'design'>(startWorkspace);
  const [audio, setAudio] = useState(api.audio);
  const sessionRef = useRef(defaultSession());
  const transport = useTransportController({
    api,
    sessionRef,
    playbackMode: workspace === 'arrange' ? 'timeline' : 'preview',
    setAudio,
  });
  return (
    <>
      <button onClick={() => void transport.playTransport()}>Play</button>
      <button onClick={() => void transport.stopTransport()}>Stop</button>
      <button onClick={() => void transport.goToStart()}>Go to Start</button>
      <button
        onClick={() => setWorkspace((current) => (current === 'arrange' ? 'design' : 'arrange'))}
      >
        Switch workspace
      </button>
      <button
        onClick={() => {
          const session = sessionRef.current;
          if (session) {
            sessionRef.current = {
              ...session,
              arrangement: { ...session.arrangement, revision: session.arrangement.revision + 1 },
            };
          }
        }}
      >
        Mutate session
      </button>
      <output>{audio.message}</output>
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

  it('stops the timeline after switching from Arrange to Design', async () => {
    const api = new FakeNativeApi();
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('playTimeline'));
    api.emitTransportStatus({ state: 'playing' });
    fireEvent.click(screen.getByRole('button', { name: 'Switch workspace' }));
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));

    await waitFor(() => expect(api.calls).toContain('stopTimeline'));
    expect(api.calls).not.toContain('stopSamplePreview');
  });

  it('stops the preview after switching from Design to Arrange', async () => {
    const api = new FakeNativeApi({
      bootstrapState: { viewState: { ...defaultViewState(), workspace: 'design' } },
    });
    render(<Harness api={api} startWorkspace="design" />);
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('previewAsset'));
    fireEvent.click(screen.getByRole('button', { name: 'Switch workspace' }));
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));

    await waitFor(() => expect(api.calls).toContain('stopSamplePreview'));
    expect(api.calls).not.toContain('stopTimeline');
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

  it('does not reuse a render result after the arrangement revision changes', async () => {
    const api = new FakeNativeApi({
      bootstrapState: { viewState: { ...defaultViewState(), workspace: 'design' } },
    });
    render(<Harness api={api} startWorkspace="design" />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('renderTimeline'));
    expect(api.calls).toContain('previewAsset');

    fireEvent.click(screen.getByRole('button', { name: 'Mutate session' }));
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() =>
      expect(api.calls.filter((call) => call === 'renderTimeline')).toHaveLength(2),
    );
    expect(api.calls).toContain('previewAsset');
  });

  it('reuses a render result while the arrangement revision is unchanged', async () => {
    const api = new FakeNativeApi({
      bootstrapState: { viewState: { ...defaultViewState(), workspace: 'design' } },
    });
    render(<Harness api={api} startWorkspace="design" />);

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => expect(api.calls).toContain('previewAsset'));
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() =>
      expect(api.calls.filter((call) => call === 'previewAsset')).toHaveLength(2),
    );
    expect(api.calls.filter((call) => call === 'renderTimeline')).toHaveLength(1);
  });
});
