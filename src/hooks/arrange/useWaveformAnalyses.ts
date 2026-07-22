import { useEffect, useState } from 'react';
import type { AudioAnalysis, AudioClip } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

export function useWaveformAnalyses(api: NativeApi, clips: AudioClip[]) {
  const [analyses, setAnalyses] = useState<Record<string, AudioAnalysis | null>>({});

  useEffect(() => {
    let active = true;
    for (const assetId of new Set(clips.map((clip) => clip.assetId))) {
      if (Object.hasOwn(analyses, assetId)) continue;
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
  }, [analyses, api, clips]);

  return analyses;
}
