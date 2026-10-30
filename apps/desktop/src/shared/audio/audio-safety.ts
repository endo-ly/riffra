import type { AudioStatus } from '@/model/generated';

/**
 * Audio safety state transitions. The Runtime remains the source of truth for
 * engine health and emergency mute.
 */

/**
 * Returns true when the audio engine can continue running commands.
 */
export function isAudioEngineUsable(audio: Pick<AudioStatus, 'state'>): boolean {
  return audio.state !== 'faulted' && audio.state !== 'offline';
}

/** Returns whether the Audio Runtime is currently forcing output silent. */
export function isOutputMuted(audio: Pick<AudioStatus, 'state'>): boolean {
  return audio.state === 'muted';
}

/** Returns whether the UI should treat the output as unavailable to release. */
export function isEmergencyMuteActive(
  audio: Pick<AudioStatus, 'state' | 'feedbackSuspected'>,
): boolean {
  return isOutputMuted(audio) || audio.feedbackSuspected;
}
