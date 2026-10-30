import clsx from 'clsx';
import type { AudioStatus } from '@/model/generated';
import { useSession } from '@/shared/session/session-context';
import { isEmergencyMuteActive } from '@/shared/audio/audio-safety';

import { AudioMonitor } from '@/features/audio/AudioMonitor';
import { TransportControls } from '@/features/transport/TransportControls';
import { Icon } from '@/shared/ui/primitives';
import { ProjectHostSelector } from './ProjectHostSelector';
import styles from './GlobalControlBar.module.css';

interface GlobalControlBarProps {
  onOpenCommand: () => void;
  onOpenAudioSettings: () => void;
  audioSettingsOpen: boolean;
}

export function GlobalControlBar(ui: GlobalControlBarProps) {
  const project = useSession((state) => state.project);
  const host = useSession((state) => state.host);
  const audio = useSession((state) => state.audio);
  const recording = useSession((state) => state.recording);
  const props = {
    ...ui,
    audio: audio.status,
    session: project.boot!.canonical.session,
    historyState: project.boot!.canonical.history,
    hostConnected: host.connection.mode !== 'disconnected' && !host.switching,
    projectSwitching: project.switching,
    isMuted: isEmergencyMuteActive(audio.status),
    recordingCommandPending: recording.commandPending || recording.stopPending,
    onToggleRecording: recording.toggle,
    onUndo: project.undo,
    onRedo: project.redo,
    onToggleMute: audio.toggleMute,
    onResetFeedback: audio.resetFeedback,
  };
  const audioStateLabel = getAudioStateLabel(props.audio.state);
  const audioDetail = props.audio.feedbackSuspected
    ? props.audio.message
    : [
        props.audio.driver,
        props.audio.roundTripMs !== null ? `${props.audio.roundTripMs.toFixed(1)} ms` : null,
      ]
        .filter(Boolean)
        .join(' · ');

  return (
    <header
      className={clsx(styles.controlBar, !props.hostConnected && styles.disconnected)}
      aria-label="Global controls"
      data-global-control-bar
    >
      <div className={styles.projectControls}>
        <ProjectHostSelector />
        <fieldset
          className={styles.projectHostBoundControls}
          disabled={!props.hostConnected || props.projectSwitching}
        >
          <div className={styles.historyControls} role="group" aria-label="History">
            <button
              type="button"
              aria-label="Undo"
              title="Undo (Ctrl+Z)"
              disabled={!props.historyState.canUndo}
              onClick={props.onUndo}
            >
              <Icon name="undo" />
            </button>
            <button
              type="button"
              aria-label="Redo"
              title="Redo (Ctrl+Y)"
              disabled={!props.historyState.canRedo}
              onClick={props.onRedo}
            >
              <Icon name="redo" />
            </button>
          </div>
        </fieldset>
      </div>

      <fieldset
        className={clsx(styles.transportControls, styles.hostBoundControls)}
        disabled={!props.hostConnected || props.projectSwitching}
      >
        <TransportControls
          session={props.session}
          recordingActive={props.audio.recording.active || props.audio.recording.processing}
          recordingCommandPending={props.recordingCommandPending}
          onToggleRecording={props.onToggleRecording}
        />
      </fieldset>

      <fieldset
        className={clsx(styles.audioControls, styles.hostBoundControls)}
        disabled={!props.hostConnected}
      >
        <div className={styles.audioControlGroup} aria-disabled={props.projectSwitching}>
          <AudioMonitor session={props.session} disabled={props.projectSwitching} />
          <button
            type="button"
            className={styles.commandTrigger}
            disabled={props.projectSwitching}
            onClick={props.onOpenCommand}
            aria-label="Search or command"
            title="Search or command (Ctrl K)"
          >
            <Icon name="search" />
          </button>
          <button
            type="button"
            className={clsx(
              styles.audioStatus,
              styles[props.audio.state],
              props.audioSettingsOpen && styles.open,
            )}
            disabled={props.projectSwitching}
            onClick={props.onOpenAudioSettings}
            aria-label={`Open Audio Settings: ${audioStateLabel}`}
            aria-haspopup="dialog"
            aria-expanded={props.audioSettingsOpen}
            title="Open Audio Settings"
            data-audio-settings-trigger
          >
            <span className={styles.audioStatusDot} />
            <span className={styles.audioStatusText}>
              <strong>{audioStateLabel}</strong>
              {audioDetail && <small>{audioDetail}</small>}
            </span>
          </button>
        </div>
        <button
          type="button"
          className={clsx(styles.emergencyButton, props.isMuted && styles.active)}
          onClick={() => void props.onToggleMute()}
          aria-label={props.isMuted ? 'UNMUTE' : 'MUTE'}
          aria-pressed={props.isMuted}
          title={props.isMuted ? 'Unmute' : 'Emergency mute'}
        >
          MUTE
        </button>
        {props.audio.feedbackSuspected && (
          <button
            type="button"
            className={styles.emergencyButton}
            onClick={props.onResetFeedback}
            aria-label="Reset feedback protection"
            title="Reset feedback protection"
          >
            RESET FEEDBACK
          </button>
        )}
      </fieldset>
    </header>
  );
}

function getAudioStateLabel(state: AudioStatus['state']) {
  switch (state) {
    case 'ready':
      return 'Ready';
    case 'starting':
      return 'Starting';
    case 'muted':
      return 'Muted';
    case 'faulted':
      return 'Fault';
    case 'offline':
      return 'Offline';
  }
}
