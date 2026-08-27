import type { AudioStatus, RecordingStopResult } from '@/model/domain';
import { invokeHost } from '../invoke';
import { audioCommandError } from './audio-error';

export async function startArrangeRecording(): Promise<AudioStatus> {
  return await invokeHost<AudioStatus>('start_arrange_recording');
}

export async function recordAnotherTake(recordingSessionId: string): Promise<AudioStatus> {
  try {
    return await invokeHost<AudioStatus>('record_another_take', { recordingSessionId });
  } catch (error) {
    return await audioCommandError('Start another take', error);
  }
}

export async function stopArrangeRecording(): Promise<RecordingStopResult> {
  return await invokeHost<RecordingStopResult>('stop_arrange_recording');
}
