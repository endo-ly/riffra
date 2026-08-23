import type { ArrangementMutationResult, CanonicalState, CreativeSession } from '@/model/domain';

type SessionProjectionResult = Pick<
  ArrangementMutationResult,
  'canonical' | 'session' | 'projection'
>;

/** Applies the committed Session and reports a typed Runtime projection failure. */
export function applyArrangementMutation(
  result: SessionProjectionResult,
  setSession: (session: CreativeSession, canonical?: CanonicalState) => void,
  onProjectionFailure: (message: string) => void,
  applyCanonicalState?: (canonical: CanonicalState) => boolean,
): boolean {
  if (applyCanonicalState && 'canonical' in result) {
    applyCanonicalState(result.canonical);
  } else {
    setSession(result.session, 'canonical' in result ? result.canonical : undefined);
  }
  if (result.projection.state === 'failed') {
    onProjectionFailure(result.projection.message);
    return true;
  }
  return false;
}
