import { playbackProblem } from '@/app/runtime/runtime-projection';
import { useSession } from '@/session/session-context';
import { selectHostReady } from '@/session/selectors';
import { Notice } from '@/shared/ui/message/Notice';

function plural(count: number, noun: string) {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

export function PlaybackNotice() {
  const runtime = useSession((state) => state.runtime);
  const hostConnected = useSession(selectHostReady);
  const problem = playbackProblem(runtime);
  if (!problem) return null;
  if (problem.kind === 'failed') {
    return (
      <Notice
        tone="danger"
        title="Playback is out of sync"
        message={problem.message}
        actions={[
          {
            label: 'Retry',
            onClick: () => void runtime.retryProjection(),
            disabled: !hostConnected,
          },
        ]}
      />
    );
  }
  const skipped = [
    problem.missingSources ? plural(problem.missingSources, 'missing source') : null,
    problem.missingDevices ? plural(problem.missingDevices, 'missing device') : null,
  ]
    .filter(Boolean)
    .join(' and ');
  return (
    <Notice
      tone="warning"
      title="Playback skips missing items"
      message={`Playback continues without ${skipped}.`}
    />
  );
}
