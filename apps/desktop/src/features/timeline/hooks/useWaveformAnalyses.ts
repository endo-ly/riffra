import { useEffect, useRef, useState } from 'react';
import type { AudioAnalysis, AudioClip } from '@/model/generated';

import { useSessionContext } from '@/shared/session/session-context';

export function useWaveformAnalyses(clips: AudioClip[]) {
  const { api, hostGeneration } = useSessionContext();
  const [analyses, setAnalyses] = useState<Record<string, AudioAnalysis | null>>({});
  const requestedRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    requestedRef.current.clear();
    setAnalyses({});
  }, [hostGeneration]);

  useEffect(() => {
    let active = true;
    const present = new Set<string>(clips.map((clip) => clip.assetId));

    // Drop cache entries for assetIds that no longer have any clip referencing
    // them. Keeps the record bounded when the user removes many clips.
    if (present.size) {
      setAnalyses((current) => {
        let pruned = false;
        const next = { ...current };
        for (const id of Object.keys(next)) {
          if (!present.has(id) && next[id] !== null) {
            delete next[id];
            requestedRef.current.delete(id);
            pruned = true;
          }
        }
        return pruned ? next : current;
      });
    }

    for (const assetId of present) {
      if (requestedRef.current.has(assetId)) continue;
      requestedRef.current.add(assetId);
      void api
        .analyzeAsset(assetId as AudioClip['assetId'])
        .then((analysis) => {
          if (active) {
            setAnalyses((current) => ({ ...current, [assetId]: analysis }));
          }
        })
        .catch(() => {
          if (active) {
            setAnalyses((current) => ({ ...current, [assetId]: null }));
          }
        });
    }
    return () => {
      active = false;
    };
  }, [api, clips, hostGeneration]);

  return analyses;
}
