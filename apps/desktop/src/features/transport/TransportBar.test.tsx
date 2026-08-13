// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { TransportBar } from './TransportBar';
import { defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';

afterEach(cleanup);

function Harness({ api }: { api: FakeNativeApi }) {
  const initial = defaultSession();
  const [session, setSession] = useState(initial);
  return (
    <TransportBar
      session={session}
      workspace="arrange"
      setSession={setSession}
      audio={api.audio}
      setAudio={() => undefined}
      transportPlaying={false}
      onPlay={() => undefined}
      onStop={() => undefined}
      onGoToStart={() => undefined}
      recordingCommandPending={false}
      onToggleRecording={() => undefined}
      api={api}
    />
  );
}

describe('TransportBar', () => {
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
