import { describe, expect, it } from 'vitest';
import { makeAudioStatus } from '@/test/test-fixtures';
import { audioCommandSucceeded, isOutputMuted } from '@/lib/audio-safety';

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

describe('isOutputMuted', () => {
  it('reflects the runtime mute state', () => {
    expect(isOutputMuted(audio('muted'))).toBe(true);
    expect(isOutputMuted(audio('ready'))).toBe(false);
  });
});
