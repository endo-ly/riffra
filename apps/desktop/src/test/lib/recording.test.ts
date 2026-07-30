import { describe, expect, it } from 'vitest';
import { decideRecordingToggle } from '@/lib/recording';

describe('decideRecordingToggle', () => {
  const base = { commandPending: false, countdown: null, recordingActive: false, countInBeats: 0 };

  it('ignores presses while a command is in flight', () => {
    expect(decideRecordingToggle({ ...base, commandPending: true }).kind).toBe('ignore');
    expect(
      decideRecordingToggle({ ...base, commandPending: true, recordingActive: true }).kind,
    ).toBe('ignore');
  });

  it('cancels an armed countdown before anything else', () => {
    expect(decideRecordingToggle({ ...base, countdown: 4, recordingActive: false }).kind).toBe(
      'cancelCountdown',
    );
    expect(decideRecordingToggle({ ...base, countdown: 0, recordingActive: true }).kind).toBe(
      'cancelCountdown',
    );
  });

  it('stops an active recording before starting a new one', () => {
    expect(decideRecordingToggle({ ...base, recordingActive: true, countInBeats: 4 }).kind).toBe(
      'stop',
    );
  });

  it('arms a count-in when beats are configured', () => {
    const decision = decideRecordingToggle({ ...base, countInBeats: 4 });
    expect(decision).toEqual({ kind: 'startCountdown', beats: 4 });
  });

  it('starts immediately when no count-in is configured', () => {
    expect(decideRecordingToggle({ ...base, countInBeats: 0 }).kind).toBe('startNow');
  });
});
