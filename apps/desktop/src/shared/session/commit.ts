import type { ArrangementMutationResult, CanonicalState } from '@/model/generated';
import { HostConnectionChangedError } from '@/native/invoke';
import { errorText } from '@/shared/errors';
import { toast } from '@/shared/toasts';
import type { CanonicalAcceptance } from './slices/project';

type SessionMutation = Pick<ArrangementMutationResult, 'canonical' | 'projection'>;
type ApplyCanonicalState = (canonical: CanonicalState) => CanonicalAcceptance;

export function applyMutation(
  result: SessionMutation,
  applyCanonicalState: ApplyCanonicalState,
  onError: (message: string) => void,
): boolean {
  if (applyCanonicalState(result.canonical) === 'rejected') return false;
  if (result.projection.state === 'failed') onError(result.projection.message);
  return true;
}

export type SessionCommit = <T extends SessionMutation>(
  operation: Promise<T>,
  options?: { onError?: (message: string) => void },
) => Promise<T | null>;

export function createSessionCommit(applyCanonicalState: ApplyCanonicalState): SessionCommit {
  return async (operation, options) => {
    const onError = options?.onError ?? ((message: string) => toast(message, { kind: 'error' }));
    let result;
    try {
      result = await operation;
    } catch (error) {
      if (!(error instanceof HostConnectionChangedError)) onError(errorText(error));
      return null;
    }
    return applyMutation(result, applyCanonicalState, onError) ? result : null;
  };
}
