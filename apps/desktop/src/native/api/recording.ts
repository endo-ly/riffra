import type { AudioStatus, RecordingStopResult } from '@/model/domain';
import { dispatchControl } from '../invoke';
import { audioCommandError } from './audio-error';

export async function startArrangeRecording(): Promise<AudioStatus> {
  return dispatchControl({ command: 'record.start', params: { recordingSessionId: null } });
}

export async function recordAnotherTake(recordingSessionId: string): Promise<AudioStatus> {
  try {
    return await dispatchControl({ command: 'record.start', params: { recordingSessionId } });
  } catch (error) {
    return await audioCommandError('Start another take', error);
  }
}

export async function stopArrangeRecording(): Promise<RecordingStopResult> {
  return dispatchControl({ command: 'record.stop', params: {} });
}
