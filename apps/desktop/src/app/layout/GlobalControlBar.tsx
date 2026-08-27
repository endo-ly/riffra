import clsx from 'clsx';
import type {
  AudioStatus,
  CanonicalState,
  CreativeSession,
  HistoryState,
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
} from '@/model/domain';
import { AudioMonitor } from '@/features/audio/AudioMonitor';
import type { AudioMonitorApi } from '@/features/audio/audio-api';
import { TransportControls } from '@/features/transport/TransportControls';
import type { TransportControlsApi } from '@/features/transport/transport-api';
import { Icon } from '@/shared/ui/primitives';
import { HostSelector } from './HostSelector';
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
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  setAudio: (audio: AudioStatus) => void;
  transportPlaying: boolean;
  onPlay: () => void;
  onStop: () => void;
  onGoToStart: () => void;
  recordingCommandPending: boolean;
  onToggleRecording: () => void;
  transportControlsApi: TransportControlsApi;
  audioMonitorApi: AudioMonitorApi;
  hostConnectionState: HostConnectionState;
  localHosts: LocalHostInfo[];
  hostSwitching: boolean;
  hostConnectionError: string | null;
  onRefreshHosts: () => Promise<unknown>;
  onSwitchHost: (target: HostTarget) => Promise<unknown>;
  onReconnectHost: () => Promise<unknown>;
  hostConnected: boolean;
}

export function GlobalControlBar(props: GlobalControlBarProps) {
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
        <HostSelector
          state={props.hostConnectionState}
          hosts={props.localHosts}
          switching={props.hostSwitching}
          error={props.hostConnectionError}
          onRefresh={props.onRefreshHosts}
          onSwitch={props.onSwitchHost}
          onReconnect={props.onReconnectHost}
        />
        <fieldset className={styles.projectHostBoundControls} disabled={!props.hostConnected}>
          <button
            type="button"
            className={styles.sessionTitle}
            onClick={props.onRenameSession}
            title="Rename Scratch Session"
          >
            <span className={styles.sessionName}>
              {props.session.projectName ?? 'Untitled Scratch'}
            </span>
            <span className={styles.sessionStatus}>
              <span className={styles.saveLight} />
              Auto-saved
            </span>
            <Icon name="chevron" />
          </button>
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
        disabled={!props.hostConnected}
      >
        <TransportControls
          session={props.session}
          applyCanonicalState={props.applyCanonicalState}
          recordingActive={props.audio.recording.active}
          transportPlaying={props.transportPlaying}
          onPlay={props.onPlay}
          onStop={props.onStop}
          onGoToStart={props.onGoToStart}
          recordingCommandPending={props.recordingCommandPending}
          onToggleRecording={props.onToggleRecording}
          api={props.transportControlsApi}
        />
      </fieldset>

      <fieldset
        className={clsx(styles.audioControls, styles.hostBoundControls)}
        disabled={!props.hostConnected}
      >
        <AudioMonitor
          session={props.session}
          applyCanonicalState={props.applyCanonicalState}
          setAudio={props.setAudio}
          api={props.audioMonitorApi}
        />
        <button
          type="button"
          className={styles.commandTrigger}
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
            <small>{audioDetail || 'Audio device'}</small>
          </span>
          <Icon name="chevron" />
        </button>
        <button
          type="button"
          className={clsx(styles.emergencyButton, props.isMuted && styles.active)}
          onClick={() => void props.onToggleMute()}
          aria-label={props.isMuted ? 'UNMUTE' : 'MUTE'}
          aria-pressed={props.isMuted}
          title={props.isMuted ? 'Unmute' : 'Emergency mute'}
        >
          <Icon name="stop" />
        </button>
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
