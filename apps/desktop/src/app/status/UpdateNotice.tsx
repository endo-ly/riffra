import type { useAppUpdater } from '@/app/hooks/useAppUpdater';
import { Notice } from '@/shared/ui/message/Notice';

export function UpdateNotice({ appUpdater }: { appUpdater: ReturnType<typeof useAppUpdater> }) {
  const version = appUpdater.availableVersion;
  if (!version || appUpdater.postponed) return null;
  return (
    <Notice
      tone="info"
      title="Update available"
      message={`Riffra ${version} can be installed. The session stays untouched until you restart.`}
      actions={[
        {
          label: appUpdater.installing ? 'Installing…' : 'Update & restart',
          onClick: () => void appUpdater.install(),
          disabled: appUpdater.installing,
        },
        {
          label: 'Later',
          onClick: appUpdater.postpone,
          disabled: appUpdater.installing,
        },
      ]}
    />
  );
}
