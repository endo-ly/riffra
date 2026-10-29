import { useSessionContext } from '@/shared/session/session-context';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { CreativeSession } from '@/model/generated';

interface PendingCommit {
  interactionId: number;
  value: number;
}

interface UseMasterGainControlOptions {
  session: CreativeSession;
  disabled?: boolean;
}

/** Shares the generation-safe preview/commit lifecycle for every Master fader. */
export function useMasterGainControl({ session, disabled = false }: UseMasterGainControlOptions) {
  const { api, commit: sessionCommit, getCanonicalState } = useSessionContext();
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
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
      previewTimer.current = window.setTimeout(() => {
        previewTimer.current = null;
        if (disposed.current) return;
        previewChain.current = previewChain.current
          .catch(() => undefined)
          .then(() => {
            if (disposed.current) return;
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
      const currentInteractionId = interactionId.current;
      if (pendingCommit.current?.interactionId === currentInteractionId) return;
      pendingCommit.current = { interactionId: currentInteractionId, value: gainDb };
      if (previewTimer.current !== null) {
        window.clearTimeout(previewTimer.current);
        previewTimer.current = null;
        previewChain.current = previewChain.current
          .catch(() => undefined)
          .then(() => {
            if (disposed.current) return;
            return api.previewMasterGainDb(gainDb);
          })
          .catch(() => undefined);
      }
      await previewChain.current.catch(() => undefined);
      if (disposed.current) {
        if (
          pendingCommit.current?.interactionId === currentInteractionId &&
          pendingCommit.current.value === gainDb
        )
          pendingCommit.current = null;
        return;
      }
      try {
        const before = getCanonicalState();
        if (before && before.session.sessionId !== sessionId.current) return;
        if (before?.session.sessionId === sessionId.current) {
          canonicalDb.current = before.session.settings.masterDb;
          lastCommittedDb.current = canonicalDb.current;
        }
        if (gainDb === lastCommittedDb.current) return;
        const requestSessionId = sessionId.current;
        const result = await sessionCommit(api.setMasterGainDb(gainDb));
        if (
          disposed.current ||
          sessionId.current !== requestSessionId ||
          interactionId.current !== currentInteractionId
        )
          return;
        const current = getCanonicalState();
        if (
          current &&
          current.session.sessionId === requestSessionId &&
          (!before || current.projectId === before.projectId) &&
          (!result || current.projectId === result.canonical.projectId)
        ) {
          lastCommittedDb.current = current.session.settings.masterDb;
          canonicalDb.current = current.session.settings.masterDb;
        } else return;
        if (!result || result.projection.state === 'failed') {
          setDraftDb(canonicalDb.current);
          previewChain.current = previewChain.current
            .catch(() => undefined)
            .then(() => {
              if (!disposed.current) return api.previewMasterGainDb(canonicalDb.current);
            })
            .catch(() => undefined);
          return;
        }
        setDraftDb(canonicalDb.current);
      } finally {
        if (
          pendingCommit.current?.interactionId === currentInteractionId &&
          pendingCommit.current.value === gainDb
        )
          pendingCommit.current = null;
      }
    },
    [api, sessionCommit, getCanonicalState, disabled],
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
