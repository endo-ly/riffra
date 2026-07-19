import type { AudioStatus } from '@/lib/domain';

/**
 * Audio safety state transitions. These encode the product invariants that keep
 * output safe: a command that faults the engine must not be treated as success,
 * and the runtime remains the single source of truth for emergency mute.
 */

/**
 * Returns true when an audio command's returned status represents a usable
 * (non-faulted, non-offline) engine.
 */
export function audioCommandSucceeded(audio: Pick<AudioStatus, 'state'>): boolean {
  return audio.state !== 'faulted' && audio.state !== 'offline';
}

/** Returns whether the Audio Runtime is currently forcing output silent. */
export function isOutputMuted(audio: Pick<AudioStatus, 'state'>): boolean {
  return audio.state === 'muted';
}
