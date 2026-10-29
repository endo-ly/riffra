import { TestSessionProvider } from '@/test/session-provider';
// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TransportControls } from './TransportControls';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi, fakeAudioStatus } from '@/test/fake-native-api';

afterEach(cleanup);

function Harness({ api }: { api: FakeNativeApi }) {
  const initial = defaultSession();
  const [session, setSession] = useState(initial);
  return (
    <TestSessionProvider
      api={api}
      applyCanonicalState={(canonical) => setSession(canonical.session)}
    >
      <TransportControls
        session={session}
        recordingActive={api.audio.recording.active}
        positionTick={0}
        transportPlaying={false}
        transportStarting={false}
        transportPaused={false}
        onPlay={() => undefined}
        onPause={() => undefined}
        onStop={() => undefined}
        onGoToStart={() => undefined}
        recordingCommandPending={false}
        onToggleRecording={() => undefined}
      />
    </TestSessionProvider>
  );
}

describe('TransportControls', () => {
  it('commits BPM and meter changes from the Arrange transport', async () => {
    const api = new FakeNativeApi({
      bootstrapState: { canonical: canonicalState(defaultSession()) },
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

  it('chooses the metronome sound from the metronome context menu', async () => {
    // Arrange
    const api = new FakeNativeApi({
      bootstrapState: { canonical: canonicalState(defaultSession()) },
      audio: fakeAudioStatus(),
    });
    const update = vi.spyOn(api, 'updateSessionSettings');
    render(<Harness api={api} />);

    // Act
    fireEvent.contextMenu(screen.getByLabelText('Toggle metronome'));
    const current = screen.getByRole('menuitemradio', { name: 'Beep' });
    fireEvent.click(screen.getByRole('menuitemradio', { name: 'Wood' }));

    // Assert
    expect(current).toHaveAttribute('aria-checked', 'true');
    await waitFor(() => expect(update).toHaveBeenCalledWith({ metronomeSound: 'wood' }));
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });
});
