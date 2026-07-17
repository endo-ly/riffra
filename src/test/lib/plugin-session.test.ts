import { describe, expect, it } from 'vitest';
import { shouldRestoreIndividualParameters } from '@/lib/plugin-session';

describe('plugin session persistence', () => {
  it('uses individual parameter replay only when no complete state blob exists', () => {
    expect(shouldRestoreIndividualParameters(null)).toBe(true);
    expect(shouldRestoreIndividualParameters('base64-state')).toBe(false);
  });
});
