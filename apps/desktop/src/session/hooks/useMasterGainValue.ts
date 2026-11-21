import type { CanonicalState, CreativeSession } from '@/model/generated';
import { useSessionContext } from '@/session/session-context';
import { useCallback } from 'react';
import { usePreviewedValue } from './usePreviewedValue';

export function useMasterGainValue(
  session: CreativeSession,
  options: { disabled?: boolean; onError?: (message: string) => void } = {},
) {
  const { api } = useSessionContext();
  const read = useCallback((canonical: CanonicalState) => canonical.session.settings.masterDb, []);
  return usePreviewedValue({
    value: session.settings.masterDb,
    sessionId: session.sessionId,
    target: 'master:gainDb',
    read,
    submit: (value) => api.setMasterGainDb(value),
    preview: (value) => api.previewMasterGainDb(value),
    disabled: options.disabled,
    onError: options.onError,
  });
}
