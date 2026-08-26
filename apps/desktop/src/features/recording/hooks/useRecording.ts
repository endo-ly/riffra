import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CanonicalState, RecordingAsset } from '@/model/domain';
import { logNativeError } from '@/native/invoke';
import type { LibraryApi, RecordingApi } from '@/native/native-api';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface UseRecordingOptions {
  hostGeneration?: number;
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
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
    applyCanonicalState,
    onCommandFailure,
    onProjectionFailure,
    onFinalizationFailure,
  } = options;
  const hostGeneration = options.hostGeneration ?? 0;
  const [recordings, setRecordings] = useState<RecordingAsset[]>([]);
  const [recordingCommandPending, setRecordingCommandPending] = useState(false);
  const recordingCommandLock = useRef(false);
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;
  const { listRecordings, startArrangeRecording, recordAnotherTake, stopArrangeRecording } = api;

  useEffect(() => {
    currentHostGeneration.current = hostGeneration;
    recordingCommandLock.current = false;
    setRecordings([]);
    setRecordingCommandPending(false);
  }, [hostGeneration]);

  const reloadRecordings = useCallback(async () => {
    const requestGeneration = hostGeneration;
    const next = await listRecordings();
    if (currentHostGeneration.current === requestGeneration) setRecordings(next);
    return next;
  }, [hostGeneration, listRecordings]);

  const refreshRecordings = useCallback(() => {
    void reloadRecordings().catch(logNativeError('listRecordings'));
  }, [reloadRecordings]);

  const runRecordingCommand = useCallback(
    async (command: RecordingCommand, errorLabel: string): Promise<boolean> => {
      const requestGeneration = hostGeneration;
      if (recordingCommandLock.current) return false;
      recordingCommandLock.current = true;
      setRecordingCommandPending(true);
      try {
        await command();
        if (currentHostGeneration.current !== requestGeneration) return false;
        return true;
      } catch (error) {
        if (currentHostGeneration.current !== requestGeneration) return false;
        logNativeError(errorLabel)(error);
        onCommandFailure(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        if (currentHostGeneration.current === requestGeneration) {
          recordingCommandLock.current = false;
          setRecordingCommandPending(false);
        }
      }
    },
    [hostGeneration, onCommandFailure],
  );

  const startRecordingNow = useCallback(
    async (recordingSessionId?: string) => {
      const succeeded = await runRecordingCommand(
        async () => {
          const nextAudio = await (recordingSessionId
            ? recordAnotherTake(recordingSessionId)
            : startArrangeRecording());
          if (currentHostGeneration.current === hostGeneration) setAudio(nextAudio);
        },
        recordingSessionId ? 'recordAnotherTake' : 'startRecording',
      );
      if (succeeded) refreshRecordings();
      return succeeded;
    },
    [
      hostGeneration,
      recordAnotherTake,
      refreshRecordings,
      runRecordingCommand,
      setAudio,
      startArrangeRecording,
    ],
  );

  const toggleRecording = useCallback(async () => {
    if (audio.recording.active) {
      const succeeded = await runRecordingCommand(async () => {
        const result = await stopArrangeRecording();
        if (currentHostGeneration.current !== hostGeneration) return;
        setAudio(result.audio);
        applyArrangementMutation(result, applyCanonicalState, onProjectionFailure);
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
    hostGeneration,
    refreshRecordings,
    runRecordingCommand,
    setAudio,
    applyCanonicalState,
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
