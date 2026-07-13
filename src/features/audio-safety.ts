import type { AudioStatus } from '../domain';

/**
 * Audio safety state transitions. These encode the product invariants that keep
 * output safe: a command that faults the engine must not be treated as success,
 * and emergency mute stays engaged whenever the engine cannot confirm safe
 * output. Extracted from App so the rules are unit-testable without React or a
 * native runtime.
 */

/**
 * Returns true when an audio command's returned status represents a usable
 * (non-faulted, non-offline) engine. Used to gate session mutations: a faulted
 * command must not persist routing or mute changes as if it succeeded.
 */
export function audioCommandSucceeded(audio: Pick<AudioStatus, 'state'>): boolean {
  return audio.state !== 'faulted' && audio.state !== 'offline';
}

/**
 * Resolves the emergencyMuted value that should persist in the Scratch Session
 * after a mute/unmute attempt. Unmute is refused while the engine is faulted or
 * offline so the user never gets a false "live" indication; mute always wins.
 */
export function resolveEmergencyMuteAfterCommand(
  currentEmergencyMuted: boolean,
  audio: Pick<AudioStatus, 'state'>,
  attemptedMute: boolean,
): boolean {
  if (!audioCommandSucceeded(audio)) {
    return currentEmergencyMuted || attemptedMute;
  }
  return attemptedMute;
}

/**
 * The effective muted flag shown to the user: emergency mute engaged from the
 * session OR the runtime reporting a muted state.
 */
export function isOutputMuted(
  sessionEmergencyMuted: boolean,
  audio: Pick<AudioStatus, 'state'>,
): boolean {
  return sessionEmergencyMuted || audio.state === 'muted';
}
