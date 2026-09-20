import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AudioStatus,
  CanonicalState,
  CreativeSession,
  SessionAudioPair,
} from '@/model/domain';
import { getHostGeneration } from '@/native/invoke';
import type { AudioApi } from '@/native/native-api';

export type MasterGainControlApi = Pick<AudioApi, 'previewMasterGainDb' | 'setMasterGainDb'>;

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
  const sessionId = useRef(session.sessionId);

  useEffect(() => {
    if (sessionId.current !== session.sessionId) {
      sessionId.current = session.sessionId;
      editing.current = false;
      if (previewTimer.current !== null) {
        window.clearTimeout(previewTimer.current);
        previewTimer.current = null;
      }
      previewChain.current = Promise.resolve();
    }
    canonicalDb.current = session.settings.masterDb;
    lastCommittedDb.current = session.settings.masterDb;
    if (!editing.current) setDraftDb(session.settings.masterDb);
  }, [session.sessionId, session.settings.masterDb]);

  useEffect(
    () => () => {
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
    },
    [],
  );

  const preview = useCallback(
    (gainDb: number) => {
      if (disabled || !Number.isFinite(gainDb)) return;
      const generationAtSchedule = getHostGeneration();
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
      previewTimer.current = window.setTimeout(() => {
        previewTimer.current = null;
        if (getHostGeneration() !== generationAtSchedule) return;
        previewChain.current = previewChain.current
          .catch(() => undefined)
          .then(() => {
            if (getHostGeneration() !== generationAtSchedule) return;
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
      const generationAtRequest = getHostGeneration();
      if (previewTimer.current !== null) {
        window.clearTimeout(previewTimer.current);
        previewTimer.current = null;
      }
      await previewChain.current.catch(() => undefined);
      if (getHostGeneration() !== generationAtRequest || !Number.isFinite(gainDb)) return;
      if (gainDb === lastCommittedDb.current) return;
      try {
        const result: SessionAudioPair = await api.setMasterGainDb(gainDb);
        if (getHostGeneration() !== generationAtRequest) return;
        lastCommittedDb.current = result.canonical.session.settings.masterDb;
        canonicalDb.current = result.canonical.session.settings.masterDb;
        applyCanonicalState(result.canonical);
        setAudio(result.audio);
        setDraftDb(result.canonical.session.settings.masterDb);
      } catch {
        if (getHostGeneration() !== generationAtRequest) return;
        setDraftDb(canonicalDb.current);
      }
    },
    [api, applyCanonicalState, setAudio],
  );

  const beginEditing = useCallback(() => {
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
