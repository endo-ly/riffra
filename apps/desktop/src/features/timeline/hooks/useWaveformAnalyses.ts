import { useEffect, useRef, useState } from 'react';
import type { AudioAnalysis, AudioClip, AssetId } from '@/model/generated';
import { useSession, useSessionContext } from '@/shared/session/session-context';

export function useWaveformAnalyses(clips: AudioClip[]) {
  const { api } = useSessionContext();
  const hostGeneration = useSession((state) => state.host.connection.generation);
  const [analyses, setAnalyses] = useState<Record<string, AudioAnalysis | null>>({});
  const requestsRef = useRef(new Map<AssetId, Promise<AudioAnalysis | null>>());
  useEffect(() => {
    requestsRef.current = new Map();
    setAnalyses({});
    return () => {
      requestsRef.current.clear();
    };
  }, [api, hostGeneration]);

  useEffect(() => {
    const requests = requestsRef.current;
    const present = new Set(clips.map((clip) => clip.assetId));
    for (const id of requests.keys()) {
      if (!present.has(id)) requests.delete(id);
    }
    setAnalyses((current) => {
      const entries = Object.entries(current).filter(([id]) => present.has(id as AssetId));
      return entries.length === Object.keys(current).length ? current : Object.fromEntries(entries);
    });

    for (const assetId of present) {
      if (requests.has(assetId)) continue;
      const request = api.analyzeAsset(assetId).catch(() => null);
      requests.set(assetId, request);
      void request.then((analysis) => {
        if (requestsRef.current.get(assetId) !== request) return;
        setAnalyses((current) => ({ ...current, [assetId]: analysis }));
      });
    }
  }, [api, clips, hostGeneration]);

  return analyses;
}
