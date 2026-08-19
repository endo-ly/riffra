import { useCallback, useRef, useState } from 'react';
import type {
  ArrangementMutationResult,
  ArrangementProjectionOutcome,
  CreativeSession,
  RuntimeProjectionStatus,
} from '@/model/domain';
import type { ArrangeApi, TransportApi } from '@/native/native-api';

interface ArrangeCommandOptions {
  api: ArrangeApi & Pick<TransportApi, 'retryRuntimeProjection'>;
  setSession: (session: CreativeSession) => void;
}

function projectionIsActive(status: RuntimeProjectionStatus): boolean {
  return (
    status.state === 'active' &&
    status.targetProjectionSequence !== null &&
    status.targetProjectionSequence === status.activeProjectionSequence
  );
}

function applyProjectionOutcome(
  outcome: ArrangementProjectionOutcome,
  wasOutOfSync: boolean,
): { outOfSync: boolean; message: string | null } {
  if (outcome.state === 'failed') {
    return { outOfSync: true, message: outcome.message };
  }
  if (outcome.state === 'queued' && projectionIsActive(outcome.status)) {
    return { outOfSync: false, message: null };
  }
  // NotRequired means that this canonical edit has no runtime projection.
  // A queued request is accepted but is not proof of activation. Neither
  // outcome may erase an existing runtime failure.
  return { outOfSync: wasOutOfSync, message: null };
}

/** Owns canonical Arrange commands and their pending/error projection for the editor. */
export function useArrangeCommands({ api, setSession }: ArrangeCommandOptions) {
  const [message, setMessage] = useState('');
  const [runtimeOutOfSync, setRuntimeOutOfSync] = useState(false);
  const runtimeOutOfSyncRef = useRef(false);
  const [pendingCanonicalOperations, setPendingCanonicalOperations] = useState(0);

  const publishRuntimeSync = useCallback((outOfSync: boolean) => {
    runtimeOutOfSyncRef.current = outOfSync;
    setRuntimeOutOfSync(outOfSync);
  }, []);

  const commit = useCallback(
    async (operation: Promise<ArrangementMutationResult | null>) => {
      setMessage('');
      setPendingCanonicalOperations((count) => count + 1);
      try {
        const next = await operation;
        if (!next) {
          setMessage('The edit was not applied.');
          return null;
        }
        setSession(next.session);
        const outcome = applyProjectionOutcome(next.projection, runtimeOutOfSyncRef.current);
        publishRuntimeSync(outcome.outOfSync);
        setMessage(outcome.message ?? '');
        return next.session;
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error);
        setMessage(detail);
        return null;
      } finally {
        setPendingCanonicalOperations((count) => Math.max(0, count - 1));
      }
    },
    [publishRuntimeSync, setSession],
  );

  const retryRuntimeSync = useCallback(async () => {
    try {
      const status = await api.retryRuntimeProjection();
      if (projectionIsActive(status)) {
        publishRuntimeSync(false);
        setMessage('');
        return;
      }
      if (status.state === 'failed') publishRuntimeSync(true);
      setMessage(status.lastError ?? 'Playback runtime projection is still pending.');
    } catch (error) {
      publishRuntimeSync(true);
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }, [api, publishRuntimeSync]);

  return {
    commit,
    message,
    setMessage,
    runtimeOutOfSync,
    retryRuntimeSync,
    pendingCanonicalOperations,
  };
}
