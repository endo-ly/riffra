import { useCallback, useEffect, useRef, useState } from 'react';
import type { InstrumentLibraryItem } from '@/model/generated';
import { logNativeError } from '@/native/invoke';
import { errorText } from '@/shared/errors';
import { useSession, useSessionContext, useSessionStore } from '@/shared/session/session-context';

export function useInstrumentPreview() {
  const { api } = useSessionContext();
  const store = useSessionStore();
  const hostGeneration = useSession((state) => state.host.connection.generation);
  const safeMode = useSession((state) => state.project.boot?.safeMode ?? false);
  const [previewingId, setPreviewingId] = useState<string | null>(null);
  const [previewPendingId, setPreviewPendingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const nativePreviewing = useRef(false);
  const pending = useRef<string | null>(null);
  const request = useRef(0);
  const instrumentPreviewing = useSession((state) => state.audio.status.instrumentPreviewing);
  useEffect(() => {
    request.current += 1;
    pending.current = null;
    nativePreviewing.current = false;
    setPreviewingId(null);
    setPreviewPendingId(null);
    setError(null);
  }, [hostGeneration]);
  useEffect(() => {
    if (instrumentPreviewing) {
      nativePreviewing.current = true;
      return;
    }
    if (!nativePreviewing.current) return;
    nativePreviewing.current = false;
    if (pending.current === null) setPreviewingId(null);
  }, [instrumentPreviewing]);
  const preview = useCallback(
    async (item: InstrumentLibraryItem) => {
      if (safeMode || item.preview === null || pending.current !== null) return;
      const id = ++request.current;
      pending.current = item.id;
      setPreviewPendingId(item.id);
      const stopping = previewingId === item.id;
      try {
        const status = await (stopping
          ? api.stopInstrumentPreview()
          : api.previewInstrument(item.id));
        if (request.current !== id) return;
        nativePreviewing.current = status.instrumentPreviewing;
        setPreviewingId(status.instrumentPreviewing ? item.id : null);
        store.getState().audio.setStatus(status);
      } catch (cause) {
        if (request.current !== id) return;
        if (!stopping) {
          if (!nativePreviewing.current) setPreviewingId(null);
          setError(errorText(cause));
        }
        logNativeError(stopping ? 'stopInstrumentPreview' : 'previewInstrument')(cause);
      } finally {
        if (request.current === id) {
          pending.current = null;
          setPreviewPendingId(null);
        }
      }
    },
    [api, previewingId, safeMode, store],
  );
  return { previewingId, previewPendingId, preview, error };
}
