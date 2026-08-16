import clsx from 'clsx';
import type { AudioStatus, CreativeSession, HistoryState } from '@/model/domain';
import type { ArrangeApi, AudioApi, ProjectSettingsApi } from '@/native/native-api';
import { TransportBar } from '@/features/transport/TransportBar';
import { Icon } from '@/shared/ui/primitives';
import shellStyles from '../AppShell.module.css';
import styles from './GlobalControlBar.module.css';

interface GlobalControlBarProps {
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
  setSession: (session: CreativeSession) => void;
  setAudio: (audio: AudioStatus) => void;
  transportPlaying: boolean;
  onPlay: () => void;
  onStop: () => void;
  onGoToStart: () => void;
  recordingCommandPending: boolean;
  onToggleRecording: () => void;
  transportApi: Pick<ArrangeApi, 'updateArrangementTimebase' | 'updateTimelineLoopRange'> &
    Pick<AudioApi, 'previewMasterGainDb' | 'setMasterGainDb'> &
    Pick<ProjectSettingsApi, 'updateSessionSettings'>;
}

export function GlobalControlBar(props: GlobalControlBarProps) {
  return (
    <header className={styles.controlBar} data-global-control-bar>
      <div className={styles.projectControls}>
        <div className={styles.brand}>
          <span className={shellStyles.logoMark}>R</span>
          <strong>RIFFRA</strong>
        </div>
        <button
          className={styles.sessionTitle}
          onClick={props.onRenameSession}
          title="Rename Scratch Session"
        >
          <span className={styles.saveLight} />
          {props.session.projectName ?? 'Untitled Scratch'}
          <small>Auto-saved</small>
          <Icon name="chevron" />
        </button>
        <div className={styles.historyControls}>
          <button
            aria-label="Undo"
            title="Undo (Ctrl+Z)"
            disabled={!props.historyState.canUndo}
            onClick={props.onUndo}
          >
            ↶
          </button>
          <button
            aria-label="Redo"
            title="Redo (Ctrl+Y)"
            disabled={!props.historyState.canRedo}
            onClick={props.onRedo}
          >
            ↷
          </button>
        </div>
      </div>

      <div className={styles.transportControls}>
        <TransportBar
          session={props.session}
          setSession={props.setSession}
          audio={props.audio}
          setAudio={props.setAudio}
          transportPlaying={props.transportPlaying}
          onPlay={props.onPlay}
          onStop={props.onStop}
          onGoToStart={props.onGoToStart}
          recordingCommandPending={props.recordingCommandPending}
          onToggleRecording={props.onToggleRecording}
          api={props.transportApi}
        />
      </div>

      <div className={styles.audioControls}>
        <button
          className={styles.commandTrigger}
          onClick={props.onOpenCommand}
          aria-label="Search or command"
          title="Search or command (Ctrl K)"
        >
          <Icon name="search" />
        </button>
        <button
          className={clsx(
            styles.enginePill,
            styles[props.audio.state],
            props.audioSettingsOpen && styles.open,
          )}
          onClick={props.onOpenAudioSettings}
          aria-label="Open Audio Settings"
          aria-haspopup="dialog"
          aria-expanded={props.audioSettingsOpen}
          title="Open Audio Settings"
          data-audio-settings-trigger
        >
          <span />
          <strong>{props.audio.state === 'ready' ? props.audio.driver : props.audio.state}</strong>
          <small>{props.audio.roundTripMs ? `${props.audio.roundTripMs} ms` : 'Audio'}</small>
          <Icon name="chevron" />
        </button>
        <button
          className={clsx(styles.emergencyButton, props.isMuted && styles.active)}
          onClick={() => void props.onToggleMute()}
          aria-label={props.isMuted ? 'UNMUTE' : 'MUTE'}
          title={props.isMuted ? 'Unmute' : 'Emergency mute'}
        >
          <Icon name="stop" />
        </button>
      </div>
    </header>
  );
}
