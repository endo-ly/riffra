import type { AudioStatus, RecordingStopResult } from '@/model/domain';
import { dispatchControl } from '../invoke';
import { audioCommandError } from './audio-error';

export interface RecordingApi {
  startArrangeRecording(): Promise<AudioStatus>;
  recordAnotherTake(recordingSessionId: string): Promise<AudioStatus>;
  stopArrangeRecording(): Promise<RecordingStopResult>;
}

async function startArrangeRecording(): Promise<AudioStatus> {
  return dispatchControl({ command: 'record.start', params: { recordingSessionId: null } });
}

async function recordAnotherTake(recordingSessionId: string): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'record.start', params: { recordingSessionId } });
  } catch (error) {
    return await audioCommandError('Start another take', error);
  }
}

async function stopArrangeRecording(): Promise<RecordingStopResult> {
  return dispatchControl({ command: 'record.stop', params: {} });
}

export const recordingApi: RecordingApi = {
  startArrangeRecording,
  recordAnotherTake,
  stopArrangeRecording,
};
