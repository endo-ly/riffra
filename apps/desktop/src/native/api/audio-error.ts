import type { AudioStatus } from '@/model/domain';
import { HostConnectionChangedError, invokeHostOrFallback } from '../invoke';
import { offlineAudioStatus } from '@/shared/audio/audio-defaults';

function nativeErrorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  try {
    return JSON.stringify(error);
  } catch {
    return 'Unknown native error';
  }
}

export async function audioCommandError(
  operation: string,
  error: unknown,
  safetyCritical = false,
): Promise<AudioStatus> {
  if (error instanceof HostConnectionChangedError) throw error;
  const status = await invokeHostOrFallback<AudioStatus>(
    'get_audio_status',
    {},
    offlineAudioStatus(),
  );
  return {
    ...status,
    state: safetyCritical || status.state === 'offline' ? 'faulted' : status.state,
    message: `${operation} failed: ${nativeErrorText(error)}. ${safetyCritical ? 'Audio output could not be confirmed; keep emergency mute engaged.' : 'Audio state was not changed.'} Saved data is safe.`,
  };
}
