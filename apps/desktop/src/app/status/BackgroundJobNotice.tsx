import type { BackgroundJobStatus } from '@/model/generated';
import { useSession } from '@/session/session-context';
import { selectHostReady } from '@/session/selectors';
import { Notice } from '@/shared/ui/message/Notice';

const JOB_TITLES: Record<BackgroundJobStatus['kind'], string> = {
  scan: 'Scanning plug-ins',
  render: 'Rendering',
  renderStems: 'Rendering stems',
  renderCapture: 'Rendering capture',
  analysisMeasure: 'Measuring',
  analysisLevels: 'Analyzing levels',
};

export function BackgroundJobNotice() {
  const job = useSession((state) => state.jobs.active);
  const cancel = useSession((state) => state.jobs.cancel);
  const hostConnected = useSession(selectHostReady);
  if (!job || !['queued', 'running', 'cancelling'].includes(job.state)) return null;
  return (
    <Notice
      tone="info"
      title={JOB_TITLES[job.kind]}
      message={job.message}
      progress={
        job.progress === null
          ? undefined
          : { ratio: job.progress, label: `${Math.round(job.progress * 100)}%` }
      }
      actions={[
        {
          label: job.state === 'cancelling' ? 'Cancelling…' : 'Cancel',
          onClick: () => void cancel(),
          disabled: !hostConnected || job.state === 'cancelling',
        },
      ]}
    />
  );
}
