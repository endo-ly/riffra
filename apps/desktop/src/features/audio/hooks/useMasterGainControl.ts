import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AudioStatus,
  CanonicalState,
  CreativeSession,
  SessionAudioPair,
} from '@/model/domain';
import { getHostGeneration, getProjectEpoch } from '@/native/invoke';
import type { AudioApi } from '@/native/native-api';

export type MasterGainControlApi = Pick<AudioApi, 'previewMasterGainDb' | 'setMasterGainDb'>;
interface PendingCommit {
  interactionId: number;
  value: number;
}

interface UseMasterGainControlOptions {
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  setAudio: (audio: AudioStatus) => void;
  api: MasterGainControlApi;
  disabled?: boolean;
}

/** Shares the generation-safe preview/commit lifecycle for every Master fader. */
export function useMasterGainControl({
  session,
  applyCanonicalState,
  setAudio,
  api,
  disabled = false,
}: UseMasterGainControlOptions) {
  const [draftDb, setDraftDb] = useState(session.settings.masterDb);
  const editing = useRef(false);
  const previewTimer = useRef<number | null>(null);
  const previewChain = useRef<Promise<void>>(Promise.resolve());
  const canonicalDb = useRef(session.settings.masterDb);
  const lastCommittedDb = useRef(session.settings.masterDb);
  const pendingCommit = useRef<PendingCommit | null>(null);
  const interactionId = useRef(0);
  const disposed = useRef(false);
  const sessionId = useRef(session.sessionId);

  useEffect(() => {
    if (sessionId.current !== session.sessionId) {
      sessionId.current = session.sessionId;
      editing.current = false;
      interactionId.current += 1;
      if (previewTimer.current !== null) {
        window.clearTimeout(previewTimer.current);
        previewTimer.current = null;
      }
      previewChain.current = Promise.resolve();
      pendingCommit.current = null;
    }
    canonicalDb.current = session.settings.masterDb;
    lastCommittedDb.current = session.settings.masterDb;
    if (!editing.current) setDraftDb(session.settings.masterDb);
  }, [session.sessionId, session.settings.masterDb]);

  useEffect(() => {
    disposed.current = false;
    return () => {
      disposed.current = true;
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
    };
  }, []);

  const preview = useCallback(
    (gainDb: number) => {
      if (disposed.current || disabled || !Number.isFinite(gainDb)) return;
      const generationAtSchedule = getHostGeneration();
      const projectEpochAtSchedule = getProjectEpoch();
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
      previewTimer.current = window.setTimeout(() => {
        previewTimer.current = null;
        if (
          disposed.current ||
          getHostGeneration() !== generationAtSchedule ||
          getProjectEpoch() !== projectEpochAtSchedule
        )
          return;
        previewChain.current = previewChain.current
          .catch(() => undefined)
          .then(() => {
            if (
              disposed.current ||
              getHostGeneration() !== generationAtSchedule ||
              getProjectEpoch() !== projectEpochAtSchedule
            )
              return;
            return api.previewMasterGainDb(gainDb);
          })
          .catch(() => undefined);
      }, 40);
    },
    [api, disabled],
  );

  const commit = useCallback(
    async (gainDb: number) => {
      editing.current = false;
      if (disposed.current || disabled || !Number.isFinite(gainDb)) return;
      const generationAtRequest = getHostGeneration();
      const projectEpochAtRequest = getProjectEpoch();
      const currentInteractionId = interactionId.current;
      if (pendingCommit.current?.interactionId === currentInteractionId) return;
      pendingCommit.current = { interactionId: currentInteractionId, value: gainDb };
      if (previewTimer.current !== null) {
        window.clearTimeout(previewTimer.current);
        previewTimer.current = null;
        previewChain.current = previewChain.current
          .catch(() => undefined)
          .then(() => {
            if (
              disposed.current ||
              getHostGeneration() !== generationAtRequest ||
              getProjectEpoch() !== projectEpochAtRequest
            )
              return;
            return api.previewMasterGainDb(gainDb);
          })
          .catch(() => undefined);
      }
      await previewChain.current.catch(() => undefined);
      if (
        disposed.current ||
        getHostGeneration() !== generationAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
      ) {
        if (
          pendingCommit.current?.interactionId === currentInteractionId &&
          pendingCommit.current.value === gainDb
        )
          pendingCommit.current = null;
        return;
      }
      try {
        if (gainDb === lastCommittedDb.current) return;
        const result: SessionAudioPair = await api.setMasterGainDb(gainDb);
        if (
          disposed.current ||
          getHostGeneration() !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return;
        lastCommittedDb.current = result.canonical.session.settings.masterDb;
        canonicalDb.current = result.canonical.session.settings.masterDb;
        applyCanonicalState(result.canonical);
        setAudio(result.audio);
        setDraftDb(result.canonical.session.settings.masterDb);
      } catch {
        if (
          disposed.current ||
          getHostGeneration() !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return;
        setDraftDb(canonicalDb.current);
        previewChain.current = previewChain.current
          .catch(() => undefined)
          .then(() => {
            if (
              disposed.current ||
              getHostGeneration() !== generationAtRequest ||
              getProjectEpoch() !== projectEpochAtRequest
            )
              return;
            return api.previewMasterGainDb(canonicalDb.current);
          })
          .catch(() => undefined);
      } finally {
        if (
          pendingCommit.current?.interactionId === currentInteractionId &&
          pendingCommit.current.value === gainDb
        )
          pendingCommit.current = null;
      }
    },
    [api, applyCanonicalState, disabled, setAudio],
  );

  const beginEditing = useCallback(() => {
    interactionId.current += 1;
    editing.current = true;
  }, []);

  return {
    draftDb,
    setDraftDb,
    beginEditing,
    preview,
    commit,
  };
}
