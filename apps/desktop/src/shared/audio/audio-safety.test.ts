import { describe, expect, it } from 'vitest';
import { makeAudioStatus } from '@/test/support/test-fixtures';
import { audioCommandSucceeded, isEmergencyMuteActive } from '@/shared/audio/audio-safety';

describe('audioCommandSucceeded', () => {
  it('treats a recoverable command error as a failed command', () => {
    expect(
      audioCommandSucceeded(
        makeAudioStatus({ state: 'ready', message: 'Preview failed: the preset was rejected.' }),
      ),
    ).toBe(false);
  });
});

describe('isEmergencyMuteActive', () => {
  it('keeps a pending feedback mute actionable as an unmute', () => {
    expect(
      isEmergencyMuteActive(makeAudioStatus({ state: 'ready', feedbackSuspected: true })),
    ).toBe(true);
    expect(isEmergencyMuteActive(makeAudioStatus({ state: 'ready' }))).toBe(false);
  });
});
