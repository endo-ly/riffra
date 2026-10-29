import { errorText } from '@/shared/errors';
import { useCallback, useState } from 'react';
import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';
import { HostConnectionChangedError } from '@/native/invoke';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
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
        applyArrangementMutation(next, applyCanonicalState, setMessage);
        return next.canonical.session;
      } catch (error) {
        if (error instanceof HostConnectionChangedError) return null;
        const detail = errorText(error);
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
