import { useCallback, useState } from 'react';
import type { ArrangementMutationResult, CreativeSession } from '@/model/domain';
import type { ArrangeApi, TransportApi } from '@/native/native-api';

interface ArrangeCommandOptions {
  api: ArrangeApi & Pick<TransportApi, 'retryRuntimeProjection'>;
  setSession: (session: CreativeSession) => void;
}

/** Owns canonical Arrange commands and their pending/error projection for the editor. */
export function useArrangeCommands({ api, setSession }: ArrangeCommandOptions) {
  const [message, setMessage] = useState('');
  const [runtimeOutOfSync, setRuntimeOutOfSync] = useState(false);
  const [pendingCanonicalOperations, setPendingCanonicalOperations] = useState(0);

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
        if (next.projection.state === 'failed') {
          setRuntimeOutOfSync(true);
          setMessage(next.projection.message);
        } else {
          setRuntimeOutOfSync(false);
          setMessage('');
        }
        return next.session;
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error);
        setMessage(detail);
        return null;
      } finally {
        setPendingCanonicalOperations((count) => Math.max(0, count - 1));
      }
    },
    [setSession],
  );

  const retryRuntimeSync = useCallback(async () => {
    try {
      await api.retryRuntimeProjection();
      setRuntimeOutOfSync(false);
      setMessage('');
    } catch (error) {
      setRuntimeOutOfSync(true);
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }, [api]);

  return {
    commit,
    message,
    setMessage,
    runtimeOutOfSync,
    retryRuntimeSync,
    pendingCanonicalOperations,
  };
}
