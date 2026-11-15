import { useMemo } from 'react';
import { useSession, useSessionContext } from './session-context';
import { selectTrack } from './selectors';
import { useSessionRead } from './useSessionRead';

export function useInstrumentMidiInspection(trackId: string | null) {
  const { api } = useSessionContext();
  const instrument = useSession((state) => selectTrack(state, trackId)?.instrument);
  const clips = useSession((state) =>
    trackId ? state.readModel?.clipsByTrack.get(trackId)?.midi : undefined,
  );
  const instrumentId = instrument?.id;
  const settings = instrument?.midi;
  const request = useMemo(
    () =>
      trackId
        ? {
            key: `instrument-midi:${trackId}`,
            inputs: [instrumentId, settings, clips],
            load: () => api.getInstrumentMidiSettings(trackId),
          }
        : null,
    [api, trackId, instrumentId, settings, clips],
  );
  const { data: inspection, error } = useSessionRead(request);
  return { inspection, error };
}
