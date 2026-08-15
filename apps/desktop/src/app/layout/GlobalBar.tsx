import type { AudioStatus, CreativeSession, HistoryState } from '@/model/domain';
import { Icon } from '@/shared/ui/primitives';
import shellStyles from '../AppShell.module.css';
import styles from './GlobalBar.module.css';

interface GlobalBarProps {
  session: CreativeSession;
  audio: AudioStatus;
  isMuted: boolean;
  historyState: HistoryState;
  onUndo: () => void;
  onRedo: () => void;
  onRenameSession: () => void;
  onToggleMute: () => void;
  onOpenCommand: () => void;
  onOpenAudioSettings: () => void;
  audioSettingsOpen: boolean;
}

export function GlobalBar(props: GlobalBarProps) {
  const {
    session,
    audio,
    isMuted,
    historyState,
    onUndo,
    onRedo,
    onRenameSession,
    onToggleMute,
    onOpenCommand,
    onOpenAudioSettings,
    audioSettingsOpen,
  } = props;
  return (
    <header className={styles.globalBar}>
      <div className={styles.brand}>
        <span className={shellStyles.logoMark}>R</span>
        <strong>RIFFRA</strong>
      </div>
      <button
        className={styles.sessionTitle}
        onClick={onRenameSession}
        title="Rename Scratch Session"
      >
        <span className={styles.saveLight} />
        {session.projectName ?? 'Untitled Scratch'}
        <small>Auto-saved</small>
        <Icon name="chevron" />
      </button>
      <div className={styles.historyControls}>
        <button
          aria-label="Undo"
          title="Undo (Ctrl+Z)"
          disabled={!historyState.canUndo}
          onClick={onUndo}
        >
          ↶
        </button>
        <button
          aria-label="Redo"
          title="Redo (Ctrl+Y)"
          disabled={!historyState.canRedo}
          onClick={onRedo}
        >
          ↷
        </button>
      </div>
      <button className={styles.commandTrigger} onClick={onOpenCommand}>
        <Icon name="search" />
        Search or command<kbd>Ctrl K</kbd>
      </button>
      <button
        className={`${styles.enginePill} ${styles[audio.state]} ${audioSettingsOpen ? styles.open : ''}`}
        onClick={onOpenAudioSettings}
        aria-label="Open Audio Settings"
        aria-haspopup="dialog"
        aria-expanded={audioSettingsOpen}
        title="Open Audio Settings"
        data-audio-settings-trigger
      >
        <span />
        <strong>{audio.state === 'ready' ? audio.driver : audio.state}</strong>
        <small>{audio.roundTripMs ? `${audio.roundTripMs} ms` : 'Audio'}</small>
        <Icon name="chevron" />
      </button>
      <button
        className={`${styles.emergencyButton} ${isMuted ? styles.active : ''}`}
        onClick={() => void onToggleMute()}
      >
        <Icon name="stop" />
        {isMuted ? 'UNMUTE' : 'MUTE'}
      </button>
    </header>
  );
}
