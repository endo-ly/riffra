/**
 * Recording toggle state machine. Recording has five mutually exclusive outcomes
 * (ignore, cancel countdown, stop, start with count-in, start now) and the
 * branching is easy to regress. Extracted from App so the decision is unit-
 * testable without a native runtime or React.
 */

export type RecordingToggleDecision =
  | { kind: "ignore" }
  | { kind: "cancelCountdown" }
  | { kind: "stop" }
  | { kind: "startCountdown"; beats: number }
  | { kind: "startNow" };

export interface RecordingToggleInput {
  /** True while a start/stop command is mid-flight; further toggles are ignored. */
  commandPending: boolean;
  /** Remaining count-in beats, or null when no countdown is armed. */
  countdown: number | null;
  /** Whether the audio engine currently reports an active recording. */
  recordingActive: boolean;
  /** Configured count-in beats for the Scratch Session (0 disables count-in). */
  countInBeats: number;
}

/**
 * Decides what a record-button press should do given the current recording
 * state. The order matters: a pending command swallows the press, an armed
 * countdown is cancelled, an active recording is stopped, and only then is a
 * new recording considered (with optional count-in).
 */
export function decideRecordingToggle(input: RecordingToggleInput): RecordingToggleDecision {
  if (input.commandPending) return { kind: "ignore" };
  if (input.countdown !== null) return { kind: "cancelCountdown" };
  if (input.recordingActive) return { kind: "stop" };
  if (input.countInBeats > 0) return { kind: "startCountdown", beats: input.countInBeats };
  return { kind: "startNow" };
}
