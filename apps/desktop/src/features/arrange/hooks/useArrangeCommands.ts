import { useCallback, useState } from 'react';
import type { ArrangementMutationResult, CanonicalState, CreativeSession } from '@/model/domain';

interface ArrangeCommandOptions {
  setSession: (session: CreativeSession, canonical?: CanonicalState) => void;
}

/** Owns canonical Arrange commands and their pending/error message for the editor. */
export function useArrangeCommands({ setSession }: ArrangeCommandOptions) {
  const [message, setMessage] = useState('');

  const commit = useCallback(
    async (operation: Promise<ArrangementMutationResult | null>) => {
      setMessage('');
      try {
        const next = await operation;
        if (!next) {
          setMessage('The edit was not applied.');
          return null;
        }
        setSession(next.session, next.canonical);
        if (next.projection.state === 'failed') setMessage(next.projection.message);
        return next.session;
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error);
        setMessage(detail);
        return null;
      }
    },
    [setSession],
  );

  return {
    commit,
    message,
    setMessage,
  };
}
