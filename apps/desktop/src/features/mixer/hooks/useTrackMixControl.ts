import { useCallback, useEffect, useRef, useState } from 'react';
import type { ArrangementMutationResult, CanonicalState, Track } from '@/model/domain';
import { getHostGeneration } from '@/native/invoke';
import type { ArrangeApi, AudioApi } from '@/native/native-api';

type TrackMixApi = Pick<ArrangeApi, 'updateTrack'> & Pick<AudioApi, 'previewTrackMix'>;
type MixParameter = 'gainDb' | 'pan';

interface UseTrackMixControlOptions {
  sessionId: string;
  track: Track;
  api: TrackMixApi;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  onError?: (message: string) => void;
  disabled?: boolean;
}

/** Coordinates one Track's transient preview and one-shot canonical commit. */
export function useTrackMixControl({
  sessionId,
  track,
  api,
  applyCanonicalState,
  onError,
  disabled = false,
}: UseTrackMixControlOptions) {
  const [gainDb, setGainDb] = useState(track.gainDb);
  const [pan, setPan] = useState(track.pan);
  const canonical = useRef({ gainDb: track.gainDb, pan: track.pan });
  const previewTimer = useRef<number | null>(null);
  const previewChain = useRef<Promise<void>>(Promise.resolve());
  const pendingCommit = useRef<Partial<Record<MixParameter, number>>>({});
  const disposed = useRef(false);
  const cancelled = useRef(false);
  const activeSessionId = useRef(sessionId);

  useEffect(() => {
    if (activeSessionId.current !== sessionId) {
      activeSessionId.current = sessionId;
      cancelled.current = true;
      if (previewTimer.current !== null) {
        window.clearTimeout(previewTimer.current);
        previewTimer.current = null;
      }
      previewChain.current = Promise.resolve();
      pendingCommit.current = {};
    }
    canonical.current = { gainDb: track.gainDb, pan: track.pan };
    setGainDb(track.gainDb);
    setPan(track.pan);
  }, [sessionId, track.id, track.gainDb, track.pan]);

  useEffect(
    () => () => {
      disposed.current = true;
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
    },
    [],
  );

  const schedulePreview = useCallback(
    (parameter: MixParameter, value: number) => {
      if (disabled || !Number.isFinite(value)) return;
      const generationAtSchedule = getHostGeneration();
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
      previewTimer.current = window.setTimeout(() => {
        previewTimer.current = null;
        if (disposed.current || getHostGeneration() !== generationAtSchedule) return;
        previewChain.current = previewChain.current
          .catch(() => undefined)
          .then(() => {
            if (disposed.current || getHostGeneration() !== generationAtSchedule) return;
            return api.previewTrackMix(track.id, { [parameter]: value });
          })
          .catch(() => undefined);
      }, 40);
    },
    [api, disabled, track.id],
  );

  const restoreCanonicalRuntime = useCallback(() => {
    const generation = getHostGeneration();
    const values = canonical.current;
    setGainDb(values.gainDb);
    setPan(values.pan);
    previewChain.current = previewChain.current
      .catch(() => undefined)
      .then(() => {
        if (disposed.current || getHostGeneration() !== generation) return;
        return api.previewTrackMix(track.id, values);
      })
      .catch(() => undefined);
  }, [api, track.id]);

  const commit = useCallback(
    async (parameter: MixParameter, value: number) => {
      if (cancelled.current) {
        cancelled.current = false;
        return;
      }
      if (disabled || !Number.isFinite(value)) {
        restoreCanonicalRuntime();
        return;
      }
      const generationAtRequest = getHostGeneration();
      const patch = { [parameter]: value } as { gainDb?: number; pan?: number };
      if (previewTimer.current !== null) {
        window.clearTimeout(previewTimer.current);
        previewTimer.current = null;
        previewChain.current = previewChain.current
          .catch(() => undefined)
          .then(() => {
            if (disposed.current || getHostGeneration() !== generationAtRequest) return;
            return api.previewTrackMix(track.id, patch);
          })
          .catch(() => undefined);
      }
      await previewChain.current.catch(() => undefined);
      if (disposed.current || getHostGeneration() !== generationAtRequest) return;
      if (value === canonical.current[parameter]) return;
      if (pendingCommit.current[parameter] === value) return;
      pendingCommit.current[parameter] = value;
      try {
        const result: ArrangementMutationResult = await api.updateTrack(track.id, patch);
        if (disposed.current || getHostGeneration() !== generationAtRequest) return;
        applyCanonicalState(result.canonical);
        const committed = result.canonical.session.arrangement.tracks.find(
          (candidate) => candidate.id === track.id,
        );
        if (committed) {
          canonical.current = { gainDb: committed.gainDb, pan: committed.pan };
          setGainDb(committed.gainDb);
          setPan(committed.pan);
        }
        if (result.projection.state === 'failed') onError?.(result.projection.message);
      } catch (error) {
        if (disposed.current || getHostGeneration() !== generationAtRequest) return;
        restoreCanonicalRuntime();
        onError?.(error instanceof Error ? error.message : String(error));
      } finally {
        if (pendingCommit.current[parameter] === value) delete pendingCommit.current[parameter];
      }
    },
    [api, applyCanonicalState, disabled, onError, restoreCanonicalRuntime, track.id],
  );

  const cancel = useCallback(() => {
    cancelled.current = true;
    if (previewTimer.current !== null) {
      window.clearTimeout(previewTimer.current);
      previewTimer.current = null;
    }
    restoreCanonicalRuntime();
  }, [restoreCanonicalRuntime]);

  const beginInteraction = useCallback(() => {
    cancelled.current = false;
  }, []);

  return {
    gainDb,
    pan,
    setGainDb,
    setPan,
    beginInteraction,
    schedulePreview,
    commit,
    cancel,
  };
}
