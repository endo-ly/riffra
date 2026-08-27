import { useCallback, useState } from 'react';
import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';
import { getHostGeneration, HostConnectionChangedError } from '@/native/invoke';

interface ArrangeCommandOptions {
  applyCanonicalState: (canonical: CanonicalState) => boolean;
}

/** Owns canonical Arrange commands and their pending/error message for the editor. */
export function useArrangeCommands({ applyCanonicalState }: ArrangeCommandOptions) {
  const [message, setMessage] = useState('');

  const commit = useCallback(
    async (operation: Promise<ArrangementMutationResult | null>) => {
      const requestGeneration = getHostGeneration();
      setMessage('');
      try {
        const next = await operation;
        if (getHostGeneration() !== requestGeneration) return null;
        if (!next) {
          setMessage('The edit was not applied.');
          return null;
        }
        applyCanonicalState(next.canonical);
        if (next.projection.state === 'failed') setMessage(next.projection.message);
        return next.canonical.session;
      } catch (error) {
        if (error instanceof HostConnectionChangedError) return null;
        if (getHostGeneration() !== requestGeneration) return null;
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
