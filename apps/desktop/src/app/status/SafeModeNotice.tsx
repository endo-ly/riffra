import { useSession } from '@/session/session-context';
import { Notice } from '@/shared/ui/message/Notice';

export function SafeModeNotice() {
  const safeMode = useSession((state) => state.project.boot?.safeMode ?? false);
  if (!safeMode) return null;
  return (
    <Notice
      tone="warning"
      title="Safe mode"
      message={
        <>
          VST3, MIDI input, driver changes, live preview and recording are isolated. Restart without{' '}
          <code>--safe-mode</code> to reconnect devices.
        </>
      }
    />
  );
}
