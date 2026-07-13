import { describe, expect, it } from 'vitest';
import { makeAudioStatus } from '@/test/test-fixtures';
import {
  audioCommandSucceeded,
  isOutputMuted,
  resolveEmergencyMuteAfterCommand,
} from '@/features/audio-safety';

function audio(state: 'offline' | 'starting' | 'ready' | 'muted' | 'faulted') {
  return makeAudioStatus({ state });
}

describe('audioCommandSucceeded', () => {
  it('treats ready, muted, and starting as usable commands', () => {
    expect(audioCommandSucceeded(audio('ready'))).toBe(true);
    expect(audioCommandSucceeded(audio('muted'))).toBe(true);
    expect(audioCommandSucceeded(audio('starting'))).toBe(true);
  });

  it('treats faulted and offline as failed commands', () => {
    expect(audioCommandSucceeded(audio('faulted'))).toBe(false);
    expect(audioCommandSucceeded(audio('offline'))).toBe(false);
  });
});

describe('resolveEmergencyMuteAfterCommand', () => {
  it('persists the attempted mute when the engine is usable', () => {
    expect(resolveEmergencyMuteAfterCommand(true, audio('ready'), false)).toBe(false);
    expect(resolveEmergencyMuteAfterCommand(false, audio('muted'), true)).toBe(true);
  });

  it('refuses unmute while the engine is faulted so output never silently goes live', () => {
    expect(resolveEmergencyMuteAfterCommand(false, audio('faulted'), false)).toBe(false);
    expect(resolveEmergencyMuteAfterCommand(true, audio('offline'), false)).toBe(true);
    expect(resolveEmergencyMuteAfterCommand(false, audio('faulted'), true)).toBe(true);
  });
});

describe('isOutputMuted', () => {
  it('is muted when either the session flag or the runtime reports muted', () => {
    expect(isOutputMuted(true, audio('ready'))).toBe(true);
    expect(isOutputMuted(false, audio('muted'))).toBe(true);
    expect(isOutputMuted(false, audio('ready'))).toBe(false);
  });
});
