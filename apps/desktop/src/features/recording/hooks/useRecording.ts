import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession, RecordingAsset } from '@/model/domain';
import { logNativeError } from '@/native/invoke';
import type { LibraryApi, RecordingApi } from '@/native/native-api';

interface UseRecordingOptions {
  audio: AudioStatus;
  session: CreativeSession | null;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  setSession: (session: CreativeSession) => void;
}

type RecordingFeatureApi = RecordingApi & Pick<LibraryApi, 'listRecordings'>;

/** Owns recording command serialization and the Inbox projection of new takes. */
export function useRecording(api: RecordingFeatureApi, options: UseRecordingOptions) {
  const { audio, session, setAudio, setSession } = options;
  const [recordings, setRecordings] = useState<RecordingAsset[]>([]);
  const [recordingCommandPending, setRecordingCommandPending] = useState(false);
  const [recordingRequestPending, setRecordingRequestPending] = useState(false);
  const recordingCommandLock = useRef(false);
  const recordingAttemptedInReadyPeriod = useRef(false);
  const { listRecordings, startArrangeRecording, recordAnotherTake, stopArrangeRecording } = api;
  const hasArmedTrack = session?.arrangement.tracks.some((track) => track.armed) ?? false;

  const reloadRecordings = useCallback(async () => {
    const next = await listRecordings();
    setRecordings(next);
    return next;
  }, [listRecordings]);

  const startRecordingNow = useCallback(
    async (recordingSessionId?: string) => {
      if (recordingCommandLock.current) return;
      recordingCommandLock.current = true;
      setRecordingCommandPending(true);
      try {
        setAudio(
          await (recordingSessionId
            ? recordAnotherTake(recordingSessionId)
            : startArrangeRecording()),
        );
        await reloadRecordings();
      } finally {
        recordingCommandLock.current = false;
        setRecordingCommandPending(false);
      }
    },
    [recordAnotherTake, reloadRecordings, setAudio, startArrangeRecording],
  );

  useEffect(() => {
    if (!recordingRequestPending) return;
    if (audio.recording.active) {
      setRecordingRequestPending(false);
      recordingAttemptedInReadyPeriod.current = false;
      return;
    }
    if (audio.state !== 'ready') {
      recordingAttemptedInReadyPeriod.current = false;
      return;
    }
    if (recordingCommandPending) return;
    if (!hasArmedTrack || recordingAttemptedInReadyPeriod.current) return;
    recordingAttemptedInReadyPeriod.current = true;
    void startRecordingNow()
      .then(() => setRecordingRequestPending(false))
      .catch(logNativeError('startRecording'));
  }, [
    audio.recording.active,
    audio.state,
    hasArmedTrack,
    recordingRequestPending,
    recordingCommandPending,
    startRecordingNow,
  ]);

  const toggleRecording = useCallback(async () => {
    if (recordingCommandLock.current) return;
    if (audio.recording.active) {
      setRecordingRequestPending(false);
      recordingAttemptedInReadyPeriod.current = false;
      recordingCommandLock.current = true;
      setRecordingCommandPending(true);
      try {
        const result = await stopArrangeRecording();
        setAudio(result.audio);
        setSession(result.session);
        await reloadRecordings();
      } finally {
        recordingCommandLock.current = false;
        setRecordingCommandPending(false);
      }
      return;
    }

    if (recordingRequestPending) {
      setRecordingRequestPending(false);
      recordingAttemptedInReadyPeriod.current = false;
      return;
    }

    recordingAttemptedInReadyPeriod.current = false;
    setRecordingRequestPending(true);
  }, [
    audio.recording.active,
    recordingRequestPending,
    reloadRecordings,
    setAudio,
    setSession,
    stopArrangeRecording,
  ]);

  return {
    recordings,
    reloadRecordings,
    recordingCommandPending,
    recordingRequestPending,
    startRecordingNow,
    toggleRecording,
  };
}
