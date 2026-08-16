import { createPortal } from 'react-dom';
import type { AudioStatus, Track } from '@/model/domain';
import type { AudioApi } from '@/native/native-api';
import { ToolbarButton } from '@/shared/ui/Toolbar';
import { PlaySurfaceContent } from './PlaySurfaceContent';
import styles from './PlaySurfacePanel.module.css';

export type PlaySurfaceMode = 'closed' | 'compact' | 'expanded';

interface PlaySurfacePanelProps {
  host: HTMLElement | null;
  mode: PlaySurfaceMode;
  track: Track | null;
  summary: string;
  onModeChange: (mode: PlaySurfaceMode) => void;
  audio: AudioStatus;
  api: Pick<AudioApi, 'sendMidiToTrack'>;
  runtimeReady: boolean;
  missingDeviceIds: string[];
  onChooseInstrument: () => void;
  onSummaryChange: (summary: string) => void;
}

export function PlaySurfacePanel({
  host,
  mode,
  track,
  summary,
  onModeChange,
  audio,
  api,
  runtimeReady,
  missingDeviceIds,
  onChooseInstrument,
  onSummaryChange,
}: PlaySurfacePanelProps) {
  if (!host || mode === 'closed') return null;

  const trackName = track?.name ?? 'No instrument selected';
  const summaryText = summary || `${trackName} · Keyboard · Computer keyboard off`;
  return createPortal(
    <section
      className={`${styles.panel} ${mode === 'expanded' ? styles.expanded : styles.compact}`}
      aria-label="Play Surface"
      data-play-surface
    >
      <header className={styles.header}>
        <div className={styles.identity}>
          <span className={styles.eyebrow}>PLAY SURFACE</span>
          <strong>{trackName}</strong>
          <span className={styles.summary}>{summaryText}</span>
        </div>
        <div className={styles.actions} aria-label="Play Surface controls">
          <ToolbarButton
            icon={mode === 'expanded' ? 'collapse' : 'expand'}
            ariaLabel={mode === 'expanded' ? 'Compact Play Surface' : 'Expand Play Surface'}
            title={mode === 'expanded' ? 'Compact Play Surface' : 'Expand Play Surface'}
            onClick={() => onModeChange(mode === 'expanded' ? 'compact' : 'expanded')}
          />
          <ToolbarButton
            icon="close"
            ariaLabel="Close Play Surface"
            title="Close Play Surface"
            onClick={() => onModeChange('closed')}
          />
        </div>
      </header>
      <div className={styles.content} aria-hidden={mode === 'compact'}>
        <PlaySurfaceContent
          track={track}
          audio={audio}
          api={api}
          runtimeReady={runtimeReady}
          missingDeviceIds={missingDeviceIds}
          onChooseInstrument={onChooseInstrument}
          onSummaryChange={onSummaryChange}
        />
      </div>
    </section>,
    host,
  );
}
