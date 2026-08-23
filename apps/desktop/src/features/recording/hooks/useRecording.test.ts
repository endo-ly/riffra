// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type { AudioStatus, CreativeSession } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
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
  const [commandError, setCommandError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [finalizationError, setFinalizationError] = useState<string | null>(null);
  const recording = useRecording(api, {
    audio,
    setAudio,
    applyCanonicalState: (canonical) => {
      setSession(canonical.session);
      return true;
    },
    onCommandFailure: setCommandError,
    onProjectionFailure: setError,
    onFinalizationFailure: setFinalizationError,
  });
  return {
    ...recording,
    audio,
    setAudio,
    session,
    setSession,
    commandError,
    error,
    finalizationError,
  };
}

describe('useRecording', () => {
  it('starts a global recording command immediately without retaining a request', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    const startedAudio = fakeAudioStatus();
    startedAudio.recording.active = true;
    api.setResponse('startArrangeRecording', startedAudio);
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(true)));

    await act(async () => {
      await result.current.toggleRecording();
    });

    expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(1);
    expect(result.current.audio.recording.active).toBe(true);
    expect(result.current.commandError).toBeNull();
    expect(result.current.recordingCommandPending).toBe(false);
  });

  it('keeps a successful start separate from a failed Inbox refresh', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    api.setFailure('listRecordings', new Error('Inbox unavailable'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(true)));

    try {
      await act(async () => {
        await expect(result.current.startRecordingNow()).resolves.toBe(true);
      });

      await waitFor(() =>
        expect(errorSpy).toHaveBeenCalledWith('[native] listRecordings failed:', expect.any(Error)),
      );
      expect(errorSpy).not.toHaveBeenCalledWith(
        '[native] startRecording failed:',
        expect.any(Error),
      );
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('does not start again when the session changes after a failed command', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    api.setFailure('startArrangeRecording', new Error('track is not armed'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(false)));

    try {
      await act(async () => {
        await result.current.toggleRecording();
      });

      act(() => {
        result.current.setSession(sessionWithTrack(true));
      });

      expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(1);
      expect(result.current.recordingCommandPending).toBe(false);
      expect(result.current.commandError).toBe('track is not armed');
      expect(errorSpy).toHaveBeenCalledOnce();
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('reports an unarmed recording command without starting recording or transport', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    api.setFailure('startArrangeRecording', new Error('No tracks are armed for recording.'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(false)));

    try {
      await act(async () => {
        await expect(result.current.toggleRecording()).resolves.toBeUndefined();
      });

      expect(api.calls).toContain('startArrangeRecording');
      expect(api.calls).not.toContain('playTimeline');
      expect(result.current.audio.recording.active).toBe(false);
      expect(result.current.commandError).toBe('No tracks are armed for recording.');
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('does not treat an overlapping start command as successful', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    let resolveStart!: (audio: AudioStatus) => void;
    const startResponse = new Promise<AudioStatus>((resolve) => {
      resolveStart = resolve;
    });
    api.setResponse('startArrangeRecording', () => startResponse);
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(true)));

    let firstStart!: Promise<boolean>;
    act(() => {
      firstStart = result.current.startRecordingNow();
    });
    await waitFor(() => expect(result.current.recordingCommandPending).toBe(true));

    let secondStart!: boolean;
    await act(async () => {
      secondStart = await result.current.startRecordingNow();
    });

    expect(secondStart).toBe(false);
    expect(result.current.recordingCommandPending).toBe(true);

    resolveStart(fakeAudioStatus());
    await act(async () => {
      await expect(firstStart).resolves.toBe(true);
    });
    expect(result.current.recordingCommandPending).toBe(false);
  });

  it('handles a failed start without retaining pending state or retrying automatically', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    api.setFailure('startArrangeRecording', new Error('audio unavailable'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(true)));

    try {
      await act(async () => {
        await result.current.toggleRecording();
      });
      expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(1);
      expect(result.current.recordingCommandPending).toBe(false);
      expect(errorSpy).toHaveBeenCalledOnce();

      api.setFailure('startArrangeRecording', null);
      act(() => {
        result.current.setAudio(fakeAudioStatus({ state: 'ready' }));
      });
      expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(1);

      await act(async () => {
        await result.current.toggleRecording();
      });
      expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(2);
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('handles Record Another Take failures inside the recording hook', async () => {
    const api = new FakeNativeApi({ recordings: [] });
    api.setFailure('recordAnotherTake', new Error('recording session unavailable'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(true)));

    try {
      await act(async () => {
        await expect(result.current.startRecordingNow('recording:1')).resolves.toBe(false);
      });

      expect(result.current.recordingCommandPending).toBe(false);
      expect(errorSpy).toHaveBeenCalledOnce();
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('clears command pending when Stop recording fails', async () => {
    const activeAudio = fakeAudioStatus();
    activeAudio.recording.active = true;
    const api = new FakeNativeApi({
      recordings: [],
      audio: activeAudio,
    });
    api.setFailure('stopArrangeRecording', new Error('stop failed'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() =>
      useRecordingHarness(api, sessionWithTrack(true), api.audio),
    );

    try {
      await act(async () => {
        await result.current.toggleRecording();
      });

      expect(result.current.recordingCommandPending).toBe(false);
      expect(result.current.audio.recording.active).toBe(true);
      expect(errorSpy).toHaveBeenCalledOnce();

      api.setFailure('stopArrangeRecording', null);
      api.setResponse('stopArrangeRecording', {
        canonical: canonicalState(defaultSession()),
        audio: fakeAudioStatus(),
        projection: { state: 'notRequired' },
        finalization: { state: 'notRequired' },
      });
      await act(async () => {
        await result.current.toggleRecording();
      });
      expect(result.current.audio.recording.active).toBe(false);
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('keeps a successful stop separate from a failed Inbox refresh', async () => {
    const activeAudio = fakeAudioStatus();
    activeAudio.recording.active = true;
    const api = new FakeNativeApi({
      recordings: [],
      audio: activeAudio,
    });
    api.setFailure('listRecordings', new Error('Inbox unavailable'));
    api.setResponse('stopArrangeRecording', {
      canonical: canonicalState(defaultSession()),
      audio: fakeAudioStatus(),
      projection: { state: 'notRequired' },
      finalization: { state: 'notRequired' },
    });
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() =>
      useRecordingHarness(api, sessionWithTrack(true), api.audio),
    );

    try {
      await act(async () => {
        await result.current.toggleRecording();
      });

      expect(result.current.audio.recording.active).toBe(false);
      await waitFor(() =>
        expect(errorSpy).toHaveBeenCalledWith('[native] listRecordings failed:', expect.any(Error)),
      );
      expect(errorSpy).not.toHaveBeenCalledWith(
        '[native] stopRecording failed:',
        expect.any(Error),
      );
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('publishes the committed session before exposing projection failure', async () => {
    const activeAudio = fakeAudioStatus();
    activeAudio.recording.active = true;
    const api = new FakeNativeApi({ recordings: [], audio: activeAudio });
    const committedSession = sessionWithTrack(false);
    api.setResponse('stopArrangeRecording', {
      canonical: canonicalState(committedSession),
      audio: fakeAudioStatus(),
      projection: {
        state: 'failed',
        message: 'projection failed',
      },
      finalization: { state: 'completed' },
    });
    const { result } = renderHook(() =>
      useRecordingHarness(api, sessionWithTrack(true), api.audio),
    );

    await act(async () => {
      await result.current.toggleRecording();
    });

    expect(result.current.session).toBe(committedSession);
    expect(result.current.error).toBe('projection failed');
  });

  it('keeps a stopped take visible when finalization requires Inbox recovery', async () => {
    const activeAudio = fakeAudioStatus();
    activeAudio.recording.active = true;
    const api = new FakeNativeApi({ recordings: [], audio: activeAudio });
    const stoppedSession = sessionWithTrack(false);
    api.setResponse('stopArrangeRecording', {
      canonical: canonicalState(stoppedSession),
      audio: fakeAudioStatus(),
      projection: { state: 'notRequired' },
      finalization: { state: 'recoveryRequired', message: 'manifest invalid' },
    });
    const { result } = renderHook(() =>
      useRecordingHarness(api, sessionWithTrack(true), api.audio),
    );

    await act(async () => {
      await result.current.toggleRecording();
    });

    expect(result.current.audio.recording.active).toBe(false);
    expect(result.current.session).toBe(stoppedSession);
    expect(result.current.finalizationError).toBe('manifest invalid');
  });
});
