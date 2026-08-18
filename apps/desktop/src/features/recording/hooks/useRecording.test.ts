// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type { CreativeSession } from '@/model/domain';
import { defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';
import { useRecording } from './useRecording';

function sessionWithTrack(armed: boolean): CreativeSession {
  const session = defaultSession();
  session.arrangement.tracks = [
    {
      id: 'track:microphone',
      name: 'Microphone',
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed,
      monitoring: 'off',
      midiInput: {},
      rack: { devices: [], macros: [] },
    },
  ];
  return session;
}

function useRecordingHarness(
  api: FakeNativeApi,
  initialSession: CreativeSession,
  initialAudio = fakeAudioStatus(),
) {
  const [audio, setAudio] = useState(initialAudio);
  const [session, setSession] = useState(initialSession);
  const recording = useRecording(api, { audio, session, setAudio, setSession });
  return { ...recording, audio, setAudio, session, setSession };
}

describe('useRecording', () => {
  it('keeps a global recording request pending until a track is armed', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(false)));

    await act(async () => {
      await result.current.toggleRecording();
    });

    expect(result.current.recordingRequestPending).toBe(true);
    expect(api.calls).not.toContain('startArrangeRecording');

    act(() => {
      result.current.setSession(sessionWithTrack(true));
    });

    await waitFor(() => {
      expect(api.calls).toContain('startArrangeRecording');
      expect(result.current.recordingRequestPending).toBe(false);
    });
  });

  it('cancels a pending global recording request when Record is pressed again', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(false)));

    await act(async () => {
      await result.current.toggleRecording();
    });
    await act(async () => {
      await result.current.toggleRecording();
    });

    expect(result.current.recordingRequestPending).toBe(false);
    expect(api.calls).not.toContain('startArrangeRecording');
  });

  it('cancels a pending request even when an armed track is already present', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    const { result } = renderHook(() =>
      useRecordingHarness(api, sessionWithTrack(true), fakeAudioStatus({ state: 'starting' })),
    );

    await act(async () => {
      await result.current.toggleRecording();
    });
    expect(result.current.recordingRequestPending).toBe(true);
    expect(api.calls).not.toContain('startArrangeRecording');

    await act(async () => {
      await result.current.toggleRecording();
    });
    expect(result.current.recordingRequestPending).toBe(false);

    act(() => {
      result.current.setAudio(fakeAudioStatus());
    });
    expect(api.calls).not.toContain('startArrangeRecording');
  });

  it('holds a failed request and retries once after audio becomes ready again', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    api.setFailure('startArrangeRecording', new Error('audio unavailable'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() =>
      useRecordingHarness(api, sessionWithTrack(true), fakeAudioStatus()),
    );

    try {
      await act(async () => {
        await result.current.toggleRecording();
      });
      await waitFor(() =>
        expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(1),
      );
      expect(result.current.recordingRequestPending).toBe(true);
      expect(errorSpy).toHaveBeenCalledOnce();

      api.setFailure('startArrangeRecording', null);
      act(() => {
        result.current.setAudio(fakeAudioStatus({ state: 'faulted' }));
      });
      act(() => {
        result.current.setAudio(fakeAudioStatus({ state: 'ready' }));
      });

      await waitFor(() => {
        expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(2);
        expect(result.current.recordingRequestPending).toBe(false);
      });
    } finally {
      errorSpy.mockRestore();
    }
  });
});
