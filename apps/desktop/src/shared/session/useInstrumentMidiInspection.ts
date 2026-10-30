import { useEffect, useState } from 'react';
import type { InstrumentMidiInspection } from '@/model/generated';
import { errorText } from '@/shared/errors';
import { useSessionContext } from './session-context';

export function useInstrumentMidiInspection(trackId: string | null, revision: number) {
  const { api } = useSessionContext();
  const [inspection, setInspection] = useState<InstrumentMidiInspection | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    if (!trackId) {
      setInspection(null);
      setError(null);
      return;
    }
    void api
      .getInstrumentMidiSettings(trackId)
      .then((next) => {
        if (!cancelled) {
          setInspection(next);
          setError(null);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setInspection(null);
          setError(errorText(cause));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [api, trackId, revision]);
  return { inspection, error };
}
