import type { AudioStatus, AssetId, SessionAudioPair } from '@/model/domain';
import { invoke } from '../invoke';
import { audioCommandError } from './audio-error';

export async function startArrangeRecording(recordingSessionId?: string): Promise<AudioStatus> {
  return await invoke<AudioStatus>('start_arrange_recording', {
    recordingSessionId: recordingSessionId ?? null,
  });
}

export async function recordAnotherTake(recordingSessionId: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>('record_another_take', { recordingSessionId });
  } catch (error) {
    return await audioCommandError('Start another take', error);
  }
}

export async function stopArrangeRecording(): Promise<AudioStatus> {
  return await invoke<AudioStatus>('stop_arrange_recording');
}

export async function createSamplePad(assetId: AssetId, name: string): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('create_sample_pad', {
    assetId,
    name,
  });
}

export async function updateSamplePad(
  padId: string,
  patch: { startMs?: number; endMs?: number; gainDb?: number; loopEnabled?: boolean },
): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('update_sample_pad', {
    padId,
    patch,
  });
}

export async function removeSamplePad(padId: string): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('remove_sample_pad', {
    padId,
  });
}
