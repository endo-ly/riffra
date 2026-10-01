import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CanonicalState, RecordingAsset } from '@/model/domain';
import { logNativeError } from '@/native/invoke';
import type { LibraryApi, NativeEventApi, RecordingApi } from '@/native/native-api';
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
type RecordingFeatureApi = RecordingApi &
  Pick<LibraryApi, 'listRecordings'> &
  Pick<NativeEventApi, 'onRecordingFinalized'>;
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
  const [recordingStopPending, setRecordingStopPending] = useState(false);
  const recordingCommandLock = useRef(false);
  const {
    listRecordings,
    onRecordingFinalized,
    startArrangeRecording,
    recordAnotherTake,
    stopArrangeRecording,
  } = api;
  useEffect(() => {
    recordingCommandLock.current = false;
    setRecordings([]);
    setRecordingCommandPending(false);
    setRecordingStopPending(false);
  }, [hostGeneration]);
  useEffect(() => {
    if (!audio.recording.active || audio.recording.processing) setRecordingStopPending(false);
  }, [audio.recording.active, audio.recording.processing]);
  const reloadRecordings = useCallback(async () => {
    const next = await listRecordings();
    setRecordings(next);
    return next;
  }, [listRecordings]);
  const refreshRecordings = useCallback(() => {
    void reloadRecordings().catch(logNativeError('listRecordings'));
  }, [reloadRecordings]);
  useEffect(() => {
    return onRecordingFinalized((event) => {
      void reloadRecordings().catch(logNativeError('listRecordings'));
      if (!event.succeeded && event.message) onFinalizationFailure(event.message);
    });
  }, [hostGeneration, onFinalizationFailure, onRecordingFinalized, reloadRecordings]);
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
        {
          recordingCommandLock.current = false;
          setRecordingCommandPending(false);
        }
      }
    },
    [onCommandFailure],
  );
  const startRecordingNow = useCallback(
    async (recordingSessionId?: string) => {
      if (recordingStopPending) return false;
      const succeeded = await runRecordingCommand(
        async () => {
          const nextAudio = await (recordingSessionId
            ? recordAnotherTake(recordingSessionId)
            : startArrangeRecording());
          setAudio(nextAudio);
        },
        recordingSessionId ? 'recordAnotherTake' : 'startRecording',
      );
      if (succeeded) refreshRecordings();
      return succeeded;
    },
    [
      recordAnotherTake,
      refreshRecordings,
      runRecordingCommand,
      setAudio,
      startArrangeRecording,
      recordingStopPending,
    ],
  );
  const toggleRecording = useCallback(async () => {
    if (audio.recording.processing || recordingStopPending) return;
    if (audio.recording.active) {
      const succeeded = await runRecordingCommand(async () => {
        const result = await stopArrangeRecording();
        setAudio(result.audio);
        if (result.audio.recording.active) setRecordingStopPending(true);
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
    audio.recording.processing,
    recordingStopPending,
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
    recordingCommandPending: recordingCommandPending || recordingStopPending,
    startRecordingNow,
    toggleRecording,
  };
}
