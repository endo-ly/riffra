import type { AudioStatus, Session, Workspace } from '@/lib/domain';
import clsx from 'clsx';
import { workspaces } from '@/constants';
import { Icon } from '../shared/ui';
import styles from './GlobalBar.module.css';

interface GlobalBarProps {
  session: Session;
  audio: AudioStatus;
  isMuted: boolean;
  undoStack: unknown[];
  redoStack: unknown[];
  onUndo: () => void;
  onRedo: () => void;
  onSwitchWorkspace: (workspace: Workspace) => void;
  onRenameSession: () => void;
  onToggleMute: () => void;
  onOpenCommand: () => void;
}

export function GlobalBar(props: GlobalBarProps) {
  const {
    session,
    audio,
    isMuted,
    undoStack,
    redoStack,
    onUndo,
    onRedo,
    onSwitchWorkspace,
    onRenameSession,
    onToggleMute,
    onOpenCommand,
  } = props;
  return (
    <header className="global-bar">
      <div className={styles.brand}>
        <span className="logo-mark">R</span>
        <strong>RIFFRA</strong>
      </div>
      <button
        className={styles.sessionTitle}
        onClick={onRenameSession}
        title="Rename Scratch Session"
      >
        <span className="save-light" />
        {session.projectName ?? 'Untitled Scratch'}
        <small>Auto-saved</small>
        <Icon name="chevron" />
      </button>
      <div className={styles.historyControls}>
        <button
          aria-label="Undo"
          title="Undo (Ctrl+Z)"
          disabled={undoStack.length === 0}
          onClick={onUndo}
        >
          ↶
        </button>
        <button
          aria-label="Redo"
          title="Redo (Ctrl+Y)"
          disabled={redoStack.length === 0}
          onClick={onRedo}
        >
          ↷
        </button>
      </div>
      <nav className="workspace-tabs" aria-label="Workspace">
        {workspaces.map((item) => (
          <button
            key={item.id}
            className={session.workspace === item.id ? 'active' : ''}
            onClick={() => onSwitchWorkspace(item.id)}
          >
            {item.label}
            <kbd>{item.key}</kbd>
          </button>
        ))}
      </nav>
      <button className="command-trigger" onClick={onOpenCommand}>
        <Icon name="search" />
        Search or command<kbd>Ctrl K</kbd>
      </button>
      <button className={`engine-pill ${audio.state}`}>
        <span />
        {audio.state === 'ready' ? audio.driver : audio.state}
        <small>{audio.roundTripMs ? `${audio.roundTripMs} ms` : 'Audio'}</small>
      </button>
      <button
        className={clsx(styles.emergencyButton, isMuted && styles.active)}
        onClick={() => void onToggleMute()}
      >
        <Icon name="stop" />
        {isMuted ? 'UNMUTE' : 'MUTE'}
      </button>
    </header>
  );
}
