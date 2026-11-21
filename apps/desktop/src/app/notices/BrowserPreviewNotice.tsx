import { useSession } from '@/session/session-context';
import { Notice } from '@/shared/ui/message/Notice';

export function BrowserPreviewNotice() {
  const nativeAvailable = useSession((state) => state.project.boot?.nativeAvailable ?? true);
  if (nativeAvailable) return null;
  return (
    <Notice
      tone="info"
      title="Browser preview"
      message="Native audio, VST3, MIDI and recording are unavailable. Open the desktop app to use them."
    />
  );
}
