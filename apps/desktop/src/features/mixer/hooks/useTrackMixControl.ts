import { useCallback, useEffect, useRef, useState } from 'react';
import type { ArrangementMutationResult, CanonicalState, Track } from '@/model/domain';
import { getHostGeneration, getProjectEpoch } from '@/native/invoke';
import type { ArrangeApi, AudioApi } from '@/native/native-api';

type TrackMixApi = Pick<ArrangeApi, 'updateTrack'> & Pick<AudioApi, 'previewTrackMix'>;
type MixParameter = 'gainDb' | 'pan';
const mixParameters: readonly MixParameter[] = ['gainDb', 'pan'];

interface PendingCommit {
  interactionId: number;
  value: number;
}

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
  const previewTimers = useRef<Partial<Record<MixParameter, number>>>({});
  const previewVersions = useRef<Record<MixParameter, number>>({ gainDb: 0, pan: 0 });
  const previewChain = useRef<Promise<void>>(Promise.resolve());
  const pendingCommit = useRef<Partial<Record<MixParameter, PendingCommit>>>({});
  const editing = useRef<Partial<Record<MixParameter, boolean>>>({});
  const interactionId = useRef(0);
  const disposed = useRef(false);
  const cancelled = useRef<Partial<Record<MixParameter, boolean>>>({});
  const activeSessionId = useRef(sessionId);
  const activeTrackId = useRef(track.id);

  const clearPreviewTimer = useCallback((parameter: MixParameter) => {
    const timer = previewTimers.current[parameter];
    if (timer !== undefined) {
      window.clearTimeout(timer);
      delete previewTimers.current[parameter];
    }
  }, []);

  const invalidatePreview = useCallback((parameter: MixParameter) => {
    previewVersions.current[parameter] += 1;
    return previewVersions.current[parameter];
  }, []);

  const queuePreview = useCallback(
    (
      parameter: MixParameter,
      value: number,
      generation: number,
      projectEpoch: number,
      previewVersion: number,
    ) => {
      previewChain.current = previewChain.current
        .catch(() => undefined)
        .then(() => {
          if (
            disposed.current ||
            previewVersions.current[parameter] !== previewVersion ||
            getHostGeneration() !== generation ||
            getProjectEpoch() !== projectEpoch
          )
            return;
          return api.previewTrackMix(track.id, { [parameter]: value });
        })
        .catch(() => undefined);
    },
    [api, track.id],
  );

  const setDraft = useCallback((parameter: MixParameter, value: number) => {
    if (parameter === 'gainDb') setGainDb(value);
    else setPan(value);
  }, []);

  const restoreCanonicalRuntime = useCallback(
    (parameter: MixParameter) => {
      const value = canonical.current[parameter];
      setDraft(parameter, value);
      clearPreviewTimer(parameter);
      const generation = getHostGeneration();
      const projectEpoch = getProjectEpoch();
      const previewVersion = invalidatePreview(parameter);
      queuePreview(parameter, value, generation, projectEpoch, previewVersion);
    },
    [clearPreviewTimer, invalidatePreview, queuePreview, setDraft],
  );

  useEffect(() => {
    const sessionChanged = activeSessionId.current !== sessionId;
    const trackChanged = activeTrackId.current !== track.id;
    if (sessionChanged || trackChanged) {
      activeSessionId.current = sessionId;
      activeTrackId.current = track.id;
      cancelled.current = {};
      editing.current = {};
      interactionId.current += 1;
      mixParameters.forEach(clearPreviewTimer);
      mixParameters.forEach(invalidatePreview);
      previewChain.current = Promise.resolve();
      pendingCommit.current = {};
    }

    const nextValues = { gainDb: track.gainDb, pan: track.pan };
    mixParameters.forEach((parameter) => {
      const previousValue = canonical.current[parameter];
      const nextValue = nextValues[parameter];
      canonical.current[parameter] = nextValue;
      if (editing.current[parameter]) return;

      setDraft(parameter, nextValue);
      if (!sessionChanged && !trackChanged && previousValue !== nextValue) {
        clearPreviewTimer(parameter);
        const generation = getHostGeneration();
        const projectEpoch = getProjectEpoch();
        const previewVersion = invalidatePreview(parameter);
        queuePreview(parameter, nextValue, generation, projectEpoch, previewVersion);
      }
    });
  }, [
    clearPreviewTimer,
    invalidatePreview,
    queuePreview,
    sessionId,
    setDraft,
    track.gainDb,
    track.id,
    track.pan,
  ]);

  useEffect(() => {
    disposed.current = false;
    return () => {
      disposed.current = true;
      mixParameters.forEach(clearPreviewTimer);
    };
  }, [clearPreviewTimer]);

  const schedulePreview = useCallback(
    (parameter: MixParameter, value: number) => {
      if (disabled || !Number.isFinite(value)) return;
      const generationAtSchedule = getHostGeneration();
      const projectEpochAtSchedule = getProjectEpoch();
      clearPreviewTimer(parameter);
      const previewVersion = invalidatePreview(parameter);
      previewTimers.current[parameter] = window.setTimeout(() => {
        delete previewTimers.current[parameter];
        if (
          disposed.current ||
          previewVersions.current[parameter] !== previewVersion ||
          getHostGeneration() !== generationAtSchedule ||
          getProjectEpoch() !== projectEpochAtSchedule
        )
          return;
        queuePreview(
          parameter,
          value,
          generationAtSchedule,
          projectEpochAtSchedule,
          previewVersion,
        );
      }, 40);
    },
    [clearPreviewTimer, disabled, invalidatePreview, queuePreview],
  );

  const commit = useCallback(
    async (parameter: MixParameter, value: number) => {
      if (cancelled.current[parameter]) {
        delete cancelled.current[parameter];
        editing.current[parameter] = false;
        return;
      }
      editing.current[parameter] = false;
      if (disabled || !Number.isFinite(value)) {
        restoreCanonicalRuntime(parameter);
        return;
      }
      const generationAtRequest = getHostGeneration();
      const projectEpochAtRequest = getProjectEpoch();
      const currentInteractionId = interactionId.current;
      if (pendingCommit.current[parameter]?.interactionId === currentInteractionId) return;
      pendingCommit.current[parameter] = { interactionId: currentInteractionId, value };
      const patch = { [parameter]: value } as { gainDb?: number; pan?: number };
      try {
        clearPreviewTimer(parameter);
        const previewVersion = invalidatePreview(parameter);
        queuePreview(parameter, value, generationAtRequest, projectEpochAtRequest, previewVersion);
        await previewChain.current.catch(() => undefined);
        if (
          disposed.current ||
          getHostGeneration() !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return;
        if (value === canonical.current[parameter]) return;
        const result: ArrangementMutationResult = await api.updateTrack(track.id, patch);
        if (
          disposed.current ||
          getHostGeneration() !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return;
        if (!applyCanonicalState(result.canonical)) return;
        const committed = result.canonical.session.arrangement.tracks.find(
          (candidate) => candidate.id === track.id,
        );
        if (committed) {
          canonical.current = { gainDb: committed.gainDb, pan: committed.pan };
          if (!editing.current.gainDb) setGainDb(committed.gainDb);
          if (!editing.current.pan) setPan(committed.pan);
        }
        if (result.projection.state === 'failed') {
          restoreCanonicalRuntime(parameter);
          onError?.(result.projection.message);
        }
      } catch (error) {
        if (
          disposed.current ||
          getHostGeneration() !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return;
        restoreCanonicalRuntime(parameter);
        onError?.(error instanceof Error ? error.message : String(error));
      } finally {
        const pending = pendingCommit.current[parameter];
        if (pending?.interactionId === currentInteractionId && pending.value === value)
          delete pendingCommit.current[parameter];
      }
    },
    [
      api,
      applyCanonicalState,
      clearPreviewTimer,
      disabled,
      invalidatePreview,
      onError,
      queuePreview,
      restoreCanonicalRuntime,
      track.id,
    ],
  );

  const cancel = useCallback(
    (parameter: MixParameter) => {
      cancelled.current[parameter] = true;
      editing.current[parameter] = false;
      interactionId.current += 1;
      clearPreviewTimer(parameter);
      restoreCanonicalRuntime(parameter);
    },
    [clearPreviewTimer, restoreCanonicalRuntime],
  );

  const beginInteraction = useCallback(
    (parameter: MixParameter) => {
      interactionId.current += 1;
      cancelled.current[parameter] = false;
      editing.current[parameter] = true;
      invalidatePreview(parameter);
    },
    [invalidatePreview],
  );

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
