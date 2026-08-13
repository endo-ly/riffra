import { useCallback, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession, RecordingAsset } from '@/model/domain';
import type { LibraryApi, RecordingApi } from '@/native/native-api';

interface UseRecordingOptions {
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  setSession: (session: CreativeSession) => void;
}

type RecordingFeatureApi = RecordingApi & Pick<LibraryApi, 'listRecordings'>;

/** Owns recording command serialization and the Inbox projection of new takes. */
export function useRecording(api: RecordingFeatureApi, options: UseRecordingOptions) {
  const { audio, setAudio, setSession } = options;
  const [recordings, setRecordings] = useState<RecordingAsset[]>([]);
  const [recordingCommandPending, setRecordingCommandPending] = useState(false);
  const recordingCommandLock = useRef(false);
  const { listRecordings, startArrangeRecording, stopArrangeRecording } = api;

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
        setAudio(await startArrangeRecording(recordingSessionId));
        await reloadRecordings();
      } finally {
        recordingCommandLock.current = false;
        setRecordingCommandPending(false);
      }
    },
    [reloadRecordings, setAudio, startArrangeRecording],
  );

  const toggleRecording = useCallback(async () => {
    if (recordingCommandLock.current) return;
    if (!audio.recording.active) {
      await startRecordingNow();
      return;
    }
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
  }, [
    audio.recording.active,
    reloadRecordings,
    setAudio,
    setSession,
    startRecordingNow,
    stopArrangeRecording,
  ]);

  return {
    recordings,
    reloadRecordings,
    recordingCommandPending,
    startRecordingNow,
    toggleRecording,
  };
}
