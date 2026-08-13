// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useRef, useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { defaultViewState } from '@/app/view-state';
import { defaultSession } from '@/native/browser-defaults';
import type { RenderResult } from '@/model/domain';
import { FakeNativeApi } from '@/native/native-api-fake';
import { useTransportController } from './useTransportController';

afterEach(cleanup);

function Harness({ api }: { api: FakeNativeApi }) {
  const [workspace, setWorkspace] = useState<'arrange' | 'design'>('arrange');
  const [audio, setAudio] = useState(api.audio);
  const [renderResult, setRenderResult] = useState<RenderResult | null>(null);
  const sessionRef = useRef(defaultSession());
  const transport = useTransportController({
    api,
    sessionRef,
    playbackMode: workspace === 'arrange' ? 'timeline' : 'preview',
    renderResult,
    setRenderResult,
    setAudio,
  });
  return (
    <>
      <button onClick={() => void transport.playTransport()}>Play</button>
      <button onClick={() => void transport.stopTransport()}>Stop</button>
      <button
        onClick={() => setWorkspace((current) => (current === 'arrange' ? 'design' : 'arrange'))}
      >
        Switch workspace
      </button>
      <output>{audio.message}</output>
      <output>{transport.transportPlaying ? 'transport-playing' : ''}</output>
    </>
  );
}

describe('useTransportController', () => {
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
    function DesignHarness() {
      const [workspace, setWorkspace] = useState<'arrange' | 'design'>('design');
      const [, setAudio] = useState(api.audio);
      const [renderResult, setRenderResult] = useState<RenderResult | null>(null);
      const sessionRef = useRef(defaultSession());
      const transport = useTransportController({
        api,
        sessionRef,
        playbackMode: workspace === 'arrange' ? 'timeline' : 'preview',
        renderResult,
        setRenderResult,
        setAudio,
      });
      return (
        <>
          <button onClick={() => void transport.playTransport()}>Play</button>
          <button onClick={() => void transport.stopTransport()}>Stop</button>
          <button onClick={() => setWorkspace('arrange')}>Switch workspace</button>
        </>
      );
    }

    render(<DesignHarness />);
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
});
