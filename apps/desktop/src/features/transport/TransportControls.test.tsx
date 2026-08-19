// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TransportControls } from './TransportControls';
import { defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';

afterEach(cleanup);

function Harness({
  api,
  onToggleRecording = () => undefined,
  transportPlaying = false,
  onPlay = () => undefined,
  onStop = () => undefined,
  onGoToStart = () => undefined,
}: {
  api: FakeNativeApi;
  onToggleRecording?: () => void;
  transportPlaying?: boolean;
  onPlay?: () => void;
  onStop?: () => void;
  onGoToStart?: () => void;
}) {
  const initial = defaultSession();
  const [session, setSession] = useState(initial);
  return (
    <TransportControls
      session={session}
      setSession={setSession}
      recordingActive={api.audio.recording.active}
      transportPlaying={transportPlaying}
      onPlay={onPlay}
      onStop={onStop}
      onGoToStart={onGoToStart}
      recordingCommandPending={false}
      onToggleRecording={onToggleRecording}
      api={api}
    />
  );
}

describe('TransportControls', () => {
  it('describes each transport control on hover', () => {
    const api = new FakeNativeApi({
      bootstrapState: { session: defaultSession() },
      audio: fakeAudioStatus(),
    });
    const onToggleRecording = vi.fn();
    render(<Harness api={api} onToggleRecording={onToggleRecording} />);

    expect(screen.getByRole('button', { name: 'Toggle loop' })).toHaveAttribute(
      'title',
      'Enable loop',
    );
    expect(screen.getByRole('button', { name: 'Play' })).toHaveAttribute('title', 'Play');
    expect(screen.getByRole('button', { name: 'Stop and go to start' })).toHaveAttribute(
      'title',
      'Stop and go to start',
    );
    expect(screen.getByRole('button', { name: 'Start recording' })).toHaveAttribute(
      'title',
      'Start recording',
    );
    const recordButton = screen.getByRole('button', { name: 'Start recording' });
    expect(recordButton).not.toBeDisabled();
    fireEvent.click(recordButton);
    expect(onToggleRecording).toHaveBeenCalledOnce();
    expect(screen.getByRole('button', { name: 'Toggle metronome' })).toHaveAttribute(
      'title',
      'Enable metronome',
    );
    expect(screen.getByRole('button', { name: 'Count-in: Off' })).toHaveAttribute(
      'title',
      'Count-in: Off',
    );
    expect(screen.getByLabelText('Project BPM')).toHaveAttribute('title', 'Project BPM');
    expect(screen.getByLabelText('Project time signature')).toHaveAttribute(
      'title',
      'Project time signature',
    );
  });

  it('delegates Play, Stop, and Go to start to the transport controller', () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    const onPlay = vi.fn();
    const onStop = vi.fn();
    const onGoToStart = vi.fn();
    const view = render(
      <Harness api={api} onPlay={onPlay} onStop={onStop} onGoToStart={onGoToStart} />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    fireEvent.click(screen.getByRole('button', { name: 'Stop and go to start' }));
    expect(onPlay).toHaveBeenCalledOnce();
    expect(onGoToStart).toHaveBeenCalledOnce();

    view.rerender(
      <Harness
        api={api}
        transportPlaying
        onPlay={onPlay}
        onStop={onStop}
        onGoToStart={onGoToStart}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Stop playback' }));
    expect(onStop).toHaveBeenCalledOnce();
  });

  it('delegates loop, metronome, and count-in edits to NativeApi', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    const mutation = () => ({
      session: defaultSession(),
      projection: { state: 'notRequired' as const },
    });
    const updateLoop = vi.fn(mutation);
    const updateSettings = vi.fn(mutation);
    api.setResponse('updateTimelineLoopRange', updateLoop);
    api.setResponse('updateSessionSettings', updateSettings);
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Toggle loop' }));
    fireEvent.click(screen.getByRole('button', { name: 'Toggle metronome' }));
    fireEvent.click(screen.getByRole('button', { name: 'Count-in: Off' }));

    await waitFor(() => {
      expect(updateLoop).toHaveBeenCalledWith(true, 0, 15_360);
      expect(updateSettings).toHaveBeenNthCalledWith(1, { metronomeEnabled: true });
      expect(updateSettings).toHaveBeenNthCalledWith(2, { countInBeats: 4 });
    });
  });

  it('commits BPM and meter changes from the Arrange transport', async () => {
    const api = new FakeNativeApi({
      bootstrapState: { session: defaultSession() },
      audio: fakeAudioStatus(),
    });
    render(<Harness api={api} />);

    const bpm = screen.getByLabelText('Project BPM');
    fireEvent.change(bpm, { target: { value: '135.5' } });
    fireEvent.blur(bpm);
    await waitFor(() => expect(api.calls).toContain('updateArrangementTimebase'));

    fireEvent.change(screen.getByLabelText('Project time signature'), {
      target: { value: '3/4' },
    });
    await waitFor(() => expect(screen.getByLabelText('Project time signature')).toHaveValue('3/4'));
    expect(api.calls.filter((call) => call === 'updateArrangementTimebase')).toHaveLength(2);
  });
});
