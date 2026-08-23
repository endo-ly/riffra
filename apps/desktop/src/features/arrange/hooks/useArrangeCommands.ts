import { useCallback, useState } from 'react';
import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';

interface ArrangeCommandOptions {
  applyCanonicalState: (canonical: CanonicalState) => boolean;
}

/** Owns canonical Arrange commands and their pending/error message for the editor. */
export function useArrangeCommands({ applyCanonicalState }: ArrangeCommandOptions) {
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
        applyCanonicalState(next.canonical);
        if (next.projection.state === 'failed') setMessage(next.projection.message);
        return next.canonical.session;
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error);
        setMessage(detail);
        return null;
      }
    },
    [applyCanonicalState],
  );

  return {
    commit,
    message,
    setMessage,
  };
}
