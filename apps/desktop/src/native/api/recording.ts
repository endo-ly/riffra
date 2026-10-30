import type { AudioStatus, RecordingStopResult } from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface RecordingApi {
  startArrangeRecording(): Promise<AudioStatus>;
  recordAnotherTake(recordingSessionId: string): Promise<AudioStatus>;
  stopArrangeRecording(): Promise<RecordingStopResult>;
}

async function startArrangeRecording(): Promise<AudioStatus> {
  return dispatchControl({ command: 'record.start', params: { recordingSessionId: null } });
}

function recordAnotherTake(recordingSessionId: string): Promise<AudioStatus> {
  return dispatchControl({ command: 'record.start', params: { recordingSessionId } });
}

async function stopArrangeRecording(): Promise<RecordingStopResult> {
  return dispatchControl({ command: 'record.stop', params: {} });
}

export const recordingApi: RecordingApi = {
  startArrangeRecording,
  recordAnotherTake,
  stopArrangeRecording,
};
