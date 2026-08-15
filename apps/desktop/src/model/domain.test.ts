import { describe, expect, it } from 'vitest';
import { defaultSession } from '@/native/browser-defaults';

describe('Scratch Session safety defaults', () => {
  it('starts at a conservative master level with a safety limiter', () => {
    const session = defaultSession();

    expect(session.projectName).toBeNull();
    expect(session.settings.masterDb).toBe(-18);
    expect(session.arrangement.tracks).toHaveLength(0);
  });
});
