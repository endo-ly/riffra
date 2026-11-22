import { useSession } from '@/session/session-context';
import { Notice } from '@/shared/ui/message/Notice';

export function UpdateNotice() {
  const appUpdate = useSession((state) => state.appUpdate);
  const version = appUpdate.availableVersion;
  if (!version || appUpdate.postponed) return null;
  return (
    <Notice
      tone="info"
      title="Update available"
      message={`Riffra ${version} can be installed. The session stays untouched until you restart.`}
      actions={[
        {
          label: appUpdate.installing ? 'Installing…' : 'Update & restart',
          onClick: () => void appUpdate.install(),
          disabled: appUpdate.installing,
        },
        {
          label: 'Later',
          onClick: appUpdate.postpone,
          disabled: appUpdate.installing,
        },
      ]}
    />
  );
}
