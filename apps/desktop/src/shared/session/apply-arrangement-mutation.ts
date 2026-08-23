import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';

type SessionProjectionResult = Pick<ArrangementMutationResult, 'canonical' | 'projection'>;

/** Applies the committed canonical state and reports a typed Runtime projection failure. */
export function applyArrangementMutation(
  result: SessionProjectionResult,
  applyCanonicalState: (canonical: CanonicalState) => boolean,
  onProjectionFailure: (message: string) => void,
): boolean {
  applyCanonicalState(result.canonical);
  if (result.projection.state === 'failed') {
    onProjectionFailure(result.projection.message);
    return true;
  }
  return false;
}
