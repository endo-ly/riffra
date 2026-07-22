import { useEffect, useRef, useState } from 'react';
import type { AudioAnalysis, AudioClip } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

export function useWaveformAnalyses(api: NativeApi, clips: AudioClip[]) {
  const [analyses, setAnalyses] = useState<Record<string, AudioAnalysis | null>>({});
  const requestedRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    let active = true;
    const present = new Set(clips.map((clip) => clip.assetId));

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
        .analyzeAsset(assetId)
        .then((analysis) => {
          if (active) setAnalyses((current) => ({ ...current, [assetId]: analysis }));
        })
        .catch(() => {
          if (active) setAnalyses((current) => ({ ...current, [assetId]: null }));
        });
    }
    return () => {
      active = false;
    };
  }, [api, clips]);

  return analyses;
}
