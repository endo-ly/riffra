import type { AudioStatus } from '@/model/domain';
import { errorText } from '@/shared/errors';
import { HostConnectionChangedError, dispatchControl } from '../invoke';

export async function audioCommandError(
  operation: string,
  error: unknown,
  safetyCritical = false,
): Promise<AudioStatus> {
  if (error instanceof HostConnectionChangedError) throw error;
  const status = await dispatchControl({ command: 'audio.status', params: {} });
  return {
    ...status,
    state: safetyCritical || status.state === 'offline' ? 'faulted' : status.state,
    message: `${operation} failed: ${errorText(error)}. ${safetyCritical ? 'Audio output could not be confirmed; keep emergency mute engaged.' : 'Audio state was not changed.'} Saved data is safe.`,
  };
}
