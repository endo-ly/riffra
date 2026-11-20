import type { AudioStatus, RecordingAsset } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';
import { notify } from '@/shared/notify';
import type { SessionCommit } from '../commit';

export interface RecordingSlice {
  recordings: RecordingAsset[];
  commandPending: boolean;
  stopPending: boolean;
  reload(): Promise<RecordingAsset[]>;
  start(recordingSessionId?: string): Promise<boolean>;
  toggle(): Promise<void>;
  acceptAudio(status: AudioStatus): void;
  reset(): void;
}

export function createRecordingSlice(
  api: Pick<
    NativeApi,
    'listRecordings' | 'startArrangeRecording' | 'recordAnotherTake' | 'stopArrangeRecording'
  >,
  get: () => RecordingSlice,
  update: (patch: Partial<RecordingSlice>) => void,
  getAudio: () => AudioStatus,
  setAudio: (status: AudioStatus) => void,
  commit: SessionCommit,
): RecordingSlice {
  let locked = false;
  let generation = 0;
  const finalizationFailure = (message: string) =>
    notify.error('Recording could not be finalized', {
      message: 'The recorded files were kept in the Inbox.',
      cause: message,
    });
  const refresh = () => {
    void get().reload().catch(logNativeError('listRecordings'));
  };
  const run = async (command: () => Promise<void>) => {
    if (locked) return false;
    locked = true;
    const current = generation;
    update({ commandPending: true });
    try {
      await command();
      return current === generation;
    } catch (error) {
      if (current === generation) notify.error('Recording failed', { cause: error });
      return false;
    } finally {
      if (current === generation) {
        locked = false;
        update({ commandPending: false });
      }
    }
  };
  return {
    recordings: [],
    commandPending: false,
    stopPending: false,
    reset() {
      generation += 1;
      locked = false;
      update({ recordings: [], commandPending: false, stopPending: false });
    },
    acceptAudio(status) {
      if ((!status.recording.active || status.recording.processing) && get().stopPending)
        update({ stopPending: false });
    },
    async reload() {
      const current = generation;
      const recordings = await api.listRecordings();
      if (current === generation) update({ recordings });
      return recordings;
    },
    async start(recordingSessionId) {
      if (get().stopPending) return false;
      const current = generation;
      const succeeded = await run(async () => {
        const audio = await (recordingSessionId
          ? api.recordAnotherTake(recordingSessionId)
          : api.startArrangeRecording());
        if (current === generation) setAudio(audio);
      });
      if (succeeded) refresh();
      return succeeded;
    },
    async toggle() {
      const audio = getAudio();
      if (audio.recording.processing || get().stopPending) return;
      if (!audio.recording.active) {
        await get().start();
        return;
      }
      const current = generation;
      const succeeded = await run(async () => {
        const result = await api.stopArrangeRecording();
        if (current !== generation) return;
        setAudio(result.audio);
        if (result.audio.recording.active) update({ stopPending: true });
        await commit(Promise.resolve(result));
        if (result.finalization.state === 'recoveryRequired')
          finalizationFailure(result.finalization.message);
      });
      if (succeeded) refresh();
    },
  };
}
