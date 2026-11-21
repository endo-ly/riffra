import { useEffect } from 'react';
import { useSession } from '@/session/session-context';
import { isEditableTypingTarget } from '@/shared/input';

/**
 * Shortcuts that apply inside the Workspace: playback with Space, and the
 * computer keyboard toggle that reaches the focused Instrument Track.
 */
export function useWorkspaceShortcuts(toggleComputerKeyboard: () => void) {
  const togglePlayback = useSession((state) => state.transport.togglePlayback);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        if (!event.repeat) toggleComputerKeyboard();
        return;
      }
      if (event.key === ' ' && !isEditableTypingTarget(event.target)) {
        event.preventDefault();
        void togglePlayback();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [toggleComputerKeyboard, togglePlayback]);
}
