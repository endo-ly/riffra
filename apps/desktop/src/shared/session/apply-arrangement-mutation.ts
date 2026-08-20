import type { ArrangementMutationResult, CreativeSession } from '@/model/domain';

type SessionProjectionResult = Pick<ArrangementMutationResult, 'session' | 'projection'>;

/** Applies the committed Session and reports a typed Runtime projection failure. */
export function applyArrangementMutation(
  result: SessionProjectionResult,
  setSession: (session: CreativeSession) => void,
  onProjectionFailure: (message: string) => void,
): boolean {
  setSession(result.session);
  if (result.projection.state === 'failed') {
    onProjectionFailure(result.projection.message);
    return true;
  }
  return false;
}
