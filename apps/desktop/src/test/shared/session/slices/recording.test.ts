// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { useEffect, useState } from 'react';
import { useStore } from 'zustand';
import { createSessionStore } from '@/shared/session/store';
import { bindSessionRuntime } from '@/shared/session/runtime';
import { clearAllToasts, useToasts } from '@/shared/toasts';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AudioStatus, CreativeSession } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import {
  createFakeNativeApi,
  type FakeNativeApi,
  fakeAudioStatus,
} from '@/test/support/fake-native-api';

function sessionWithTrack(armed: boolean): CreativeSession {
  const session = defaultSession();
  session.arrangement.tracks = [
    {
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
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
      effects: [],
    },
  ];
  return session;
}

function useRecordingHarness(
  api: FakeNativeApi,
  initialSession: CreativeSession,
  initialAudio = fakeAudioStatus(),
) {
  const [store] = useState(() => {
    const next = createSessionStore(api);
    next.getState().host.adoptConnection(api.hostConnectionState);
    next.getState().project.acceptBootstrap({
      ...api.bootstrapState,
      canonical: { ...canonicalState(initialSession), sequence: -1 },
    });
    next.getState().audio.setStatus(initialAudio);
    return next;
  });
  useEffect(() => bindSessionRuntime(store, api), [store, api]);
  const state = useStore(store);
  const toasts = useToasts();
  const lastToast = toasts.at(-1)?.text ?? null;
  return {
    toggleRecording: state.recording.toggle,
    startRecordingNow: state.recording.start,
    recordingCommandPending: state.recording.commandPending || state.recording.stopPending,
    audio: state.audio.status,
    setAudio: state.audio.setStatus,
    session: state.project.boot?.canonical.session,
    setSession: (session: CreativeSession) => {
      const boot = store.getState().project.boot;
      if (boot)
        store
          .getState()
          .project.acceptBootstrap({ ...boot, canonical: { ...boot.canonical, session } });
    },
    commandError: lastToast?.startsWith('Recording failed: ')
      ? lastToast.slice('Recording failed: '.length)
      : null,
    error: lastToast === 'projection failed' ? lastToast : null,
    finalizationError: lastToast?.startsWith('Recording files were preserved in Inbox: ')
      ? lastToast.slice('Recording files were preserved in Inbox: '.length)
      : null,
  };
}

afterEach(clearAllToasts);

describe('recording', () => {
  it('covers start success, Inbox failure, pending cleanup, and no automatic retry', async () => {
    const api = createFakeNativeApi({ recordings: [] });
    const startedAudio = fakeAudioStatus();
    startedAudio.recording.active = true;
    api.setResponse('startArrangeRecording', startedAudio);
    api.setFailure('listRecordings', new Error('Inbox unavailable'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { result } = renderHook(() => useRecordingHarness(api, sessionWithTrack(true)));

    try {
      await act(async () => {
        await result.current.toggleRecording();
      });

      await waitFor(() =>
        expect(errorSpy).toHaveBeenCalledWith('[native] listRecordings failed:', expect.any(Error)),
      );
      expect(result.current.audio.recording.active).toBe(true);
      expect(result.current.commandError).toBeNull();
      expect(result.current.recordingCommandPending).toBe(false);

      api.setResponse('startArrangeRecording', undefined);
      api.setFailure('startArrangeRecording', new Error('audio unavailable'));
      act(() => {
        result.current.setAudio(fakeAudioStatus());
      });
      await act(async () => {
        await result.current.toggleRecording();
      });

      expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(2);
      expect(result.current.recordingCommandPending).toBe(false);
      expect(result.current.commandError).toBe('audio unavailable');

      act(() => {
        result.current.setSession(sessionWithTrack(true));
      });
      expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(2);
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('reports an unarmed recording command without starting recording or transport', async () => {
    const api = createFakeNativeApi({ recordings: [] });
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
    const api = createFakeNativeApi({ recordings: [] });
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

  it('handles Record Another Take failures inside the recording hook', async () => {
    const api = createFakeNativeApi({ recordings: [] });
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

  it('covers stop failure, finalization success, and Inbox refresh failure', async () => {
    const activeAudio = fakeAudioStatus();
    activeAudio.recording.active = true;
    const api = createFakeNativeApi({
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
      errorSpy.mockClear();

      api.setFailure('stopArrangeRecording', null);
      api.setFailure('listRecordings', new Error('Inbox unavailable'));
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
    const api = createFakeNativeApi({ recordings: [], audio: activeAudio });
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

  it('keeps recording commands blocked while the accepted stop is still pending', async () => {
    const activeAudio = fakeAudioStatus();
    activeAudio.recording.active = true;
    const api = createFakeNativeApi({ recordings: [], audio: activeAudio });
    api.setResponse('stopArrangeRecording', {
      canonical: canonicalState(defaultSession()),
      audio: activeAudio,
      projection: { state: 'notRequired' },
      finalization: { state: 'processing' },
    });
    const { result } = renderHook(() =>
      useRecordingHarness(api, sessionWithTrack(true), activeAudio),
    );

    await act(async () => {
      await result.current.toggleRecording();
    });

    expect(result.current.audio.recording.active).toBe(true);
    expect(result.current.recordingCommandPending).toBe(true);
    await act(async () => {
      await result.current.toggleRecording();
      await result.current.startRecordingNow();
    });
    expect(api.calls.filter((call) => call === 'stopArrangeRecording')).toHaveLength(1);
    expect(api.calls.filter((call) => call === 'startArrangeRecording')).toHaveLength(0);

    act(() => {
      const stoppedAudio = fakeAudioStatus();
      stoppedAudio.recording.processing = true;
      result.current.setAudio(stoppedAudio);
    });
    await waitFor(() => expect(result.current.recordingCommandPending).toBe(false));
  });

  it('keeps a stopped take visible when finalization requires Inbox recovery', async () => {
    const activeAudio = fakeAudioStatus();
    activeAudio.recording.active = true;
    const api = createFakeNativeApi({ recordings: [], audio: activeAudio });
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

  it('keeps recording controls blocked until native finalization completes', async () => {
    const processingAudio = fakeAudioStatus();
    processingAudio.recording.processing = true;
    const api = createFakeNativeApi({ recordings: [], audio: processingAudio });
    const { result } = renderHook(() =>
      useRecordingHarness(api, sessionWithTrack(true), processingAudio),
    );

    await act(async () => {
      await result.current.toggleRecording();
    });

    expect(api.calls).not.toContain('startArrangeRecording');
    expect(api.calls).toContain('onRecordingFinalized');

    act(() => {
      api.emitRecordingFinalized({
        directory: 'recordings/inbox/take-1',
        succeeded: false,
        message: 'processed output failed',
      });
    });

    await waitFor(() => expect(result.current.finalizationError).toBe('processed output failed'));
    expect(api.calls).toContain('listRecordings');
  });
});
