import { useCallback, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession, RecordingAsset } from '@/model/domain';
import { logNativeError } from '@/native/invoke';
import type { LibraryApi, RecordingApi } from '@/native/native-api';

interface UseRecordingOptions {
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  setSession: (session: CreativeSession) => void;
}

type RecordingFeatureApi = RecordingApi & Pick<LibraryApi, 'listRecordings'>;
type RecordingCommand = () => Promise<void>;

/** Owns recording command serialization and the Inbox projection of new takes. */
export function useRecording(api: RecordingFeatureApi, options: UseRecordingOptions) {
  const { audio, setAudio, setSession } = options;
  const [recordings, setRecordings] = useState<RecordingAsset[]>([]);
  const [recordingCommandPending, setRecordingCommandPending] = useState(false);
  const recordingCommandLock = useRef(false);
  const { listRecordings, startArrangeRecording, recordAnotherTake, stopArrangeRecording } = api;

  const reloadRecordings = useCallback(async () => {
    const next = await listRecordings();
    setRecordings(next);
    return next;
  }, [listRecordings]);

  const refreshRecordings = useCallback(() => {
    void reloadRecordings().catch(logNativeError('listRecordings'));
  }, [reloadRecordings]);

  const runRecordingCommand = useCallback(
    async (command: RecordingCommand, errorLabel: string): Promise<boolean> => {
      if (recordingCommandLock.current) return false;
      recordingCommandLock.current = true;
      setRecordingCommandPending(true);
      try {
        await command();
        return true;
      } catch (error) {
        logNativeError(errorLabel)(error);
        return false;
      } finally {
        recordingCommandLock.current = false;
        setRecordingCommandPending(false);
      }
    },
    [],
  );

  const startRecordingNow = useCallback(
    async (recordingSessionId?: string) => {
      const succeeded = await runRecordingCommand(
        async () => {
          setAudio(
            await (recordingSessionId
              ? recordAnotherTake(recordingSessionId)
              : startArrangeRecording()),
          );
        },
        recordingSessionId ? 'recordAnotherTake' : 'startRecording',
      );
      if (succeeded) refreshRecordings();
      return succeeded;
    },
    [recordAnotherTake, refreshRecordings, runRecordingCommand, setAudio, startArrangeRecording],
  );

  const toggleRecording = useCallback(async () => {
    if (audio.recording.active) {
      const succeeded = await runRecordingCommand(async () => {
        const result = await stopArrangeRecording();
        setAudio(result.audio);
        setSession(result.session);
      }, 'stopRecording');
      if (succeeded) refreshRecordings();
      return;
    }

    await startRecordingNow();
  }, [
    audio.recording.active,
    refreshRecordings,
    runRecordingCommand,
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
