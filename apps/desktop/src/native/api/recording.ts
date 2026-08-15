import type { AudioStatus, SessionAudioPair } from '@/model/domain';
import { invoke } from '../invoke';
import { audioCommandError } from './audio-error';

export async function startArrangeRecording(): Promise<AudioStatus> {
  return await invoke<AudioStatus>('start_arrange_recording');
}

export async function recordAnotherTake(recordingSessionId: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('record_another_take', { recordingSessionId });
  } catch (error) {
    return await audioCommandError('Start another take', error);
  }
}

export async function stopArrangeRecording(): Promise<SessionAudioPair> {
  return await invoke<SessionAudioPair>('stop_arrange_recording');
}
