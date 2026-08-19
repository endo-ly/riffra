import { useCallback, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession, RecordingAsset } from '@/model/domain';
import { logNativeError } from '@/native/invoke';
import type { LibraryApi, RecordingApi } from '@/native/native-api';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface UseRecordingOptions {
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  setSession: (session: CreativeSession) => void;
  onCommandFailure: (message: string) => void;
  onProjectionFailure: (message: string) => void;
  onFinalizationFailure: (message: string) => void;
}

type RecordingFeatureApi = RecordingApi & Pick<LibraryApi, 'listRecordings'>;
type RecordingCommand = () => Promise<void>;

/** Owns recording command serialization and the Inbox projection of new takes. */
export function useRecording(api: RecordingFeatureApi, options: UseRecordingOptions) {
  const {
    audio,
    setAudio,
    setSession,
    onCommandFailure,
    onProjectionFailure,
    onFinalizationFailure,
  } = options;
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
        onCommandFailure(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        recordingCommandLock.current = false;
        setRecordingCommandPending(false);
      }
    },
    [onCommandFailure],
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
        applyArrangementMutation(result, setSession, onProjectionFailure);
        if (result.finalization.state === 'recoveryRequired') {
          onFinalizationFailure(result.finalization.message);
        }
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
    onProjectionFailure,
    onFinalizationFailure,
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
