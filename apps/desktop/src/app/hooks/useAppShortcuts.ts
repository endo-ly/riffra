import { useEffect } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import { isEditableTypingTarget } from '@/shared/input';
import { useSessionStore } from '@/session/session-context';
import { selectHostReady, selectWorkspaceWaiting } from '@/session/selectors';

export function useAppShortcuts(setCommandOpen: Dispatch<SetStateAction<boolean>>) {
  const store = useSessionStore();
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const state = store.getState();
      const { project, audio } = state;
      if (!selectHostReady(state)) return;
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'm') {
        event.preventDefault();
        void audio.toggleMute();
        return;
      }
      if (event.ctrlKey && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setCommandOpen((open) => !open);
        return;
      }
      if (selectWorkspaceWaiting(state)) return;
      const typing = isEditableTypingTarget(event.target);
      if (event.ctrlKey && !event.shiftKey && !typing && event.key.toLowerCase() === 'b') {
        event.preventDefault();
        state.workspace.toggleBrowser();
        return;
      }
      if (event.ctrlKey && !event.shiftKey && !typing && event.key.toLowerCase() === 'i') {
        event.preventDefault();
        state.workspace.toggleInspector();
        return;
      }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === 'z') {
        event.preventDefault();
        void (event.shiftKey ? project.redo() : project.undo());
        return;
      }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === 'y') {
        event.preventDefault();
        void project.redo();
        return;
      }
      if (
        !event.defaultPrevented &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey &&
        !event.shiftKey &&
        !event.repeat &&
        !typing &&
        event.key.toLowerCase() === 'v'
      ) {
        state.workspace.toggleViewMode();
        return;
      }
      if (
        event.key === ' ' &&
        state.workspace.viewMode === 'showcase' &&
        !event.defaultPrevented &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey &&
        !event.repeat &&
        !typing
      ) {
        event.preventDefault();
        void state.transport.togglePlayback();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [store, setCommandOpen]);
}
