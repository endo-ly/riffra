import type { AudioAnalysis, AudioClip, ProjectTimebase } from '@/lib/domain';
import { AudioWaveform } from './AudioWaveform';
import { clipDurationTicks } from '@/lib/arrange-timeline';
import styles from './WorkspaceArrange.module.css';

interface AudioClipViewProps {
  clip: AudioClip;
  analysis?: AudioAnalysis | null;
  timebase: ProjectTimebase;
  pixelsPerTick: number;
  lane: number;
  laneHeight: number;
  selected: boolean;
  onSelect: (clipId: string, append?: boolean) => void;
  onMove: (event: React.PointerEvent<HTMLButtonElement>, clip: AudioClip) => void;
  onTrim: (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: AudioClip,
    side: 'left' | 'right',
  ) => void;
  onFade: (event: React.PointerEvent<HTMLSpanElement>, clip: AudioClip, side: 'in' | 'out') => void;
}

export function AudioClipView(props: AudioClipViewProps) {
  const { clip } = props;
  const duration = clipDurationTicks(clip, props.timebase);
  const sourceFrames = Math.max(1, clip.sourceRange.end - clip.sourceRange.start);
  const loopBoundaries = clip.loopEnabled
    ? Math.max(0, Math.ceil(clip.timelineDuration.frames / sourceFrames) - 1)
    : 0;
  return (
    <button
      data-clip-id={clip.id}
      aria-pressed={props.selected}
      className={`${styles.clip} ${props.selected ? styles.selected : ''} ${clip.loopEnabled ? styles.looped : ''}`}
      style={{
        left: clip.startTick * props.pixelsPerTick,
        width: Math.max(24, duration * props.pixelsPerTick),
        top: props.lane * props.laneHeight + 6,
        height: props.laneHeight - 12,
        opacity: clip.muted ? 0.48 : 1,
      }}
      onPointerDown={(event) => props.onMove(event, clip)}
      onClick={(event) => {
        if (!event.ctrlKey && !props.selected) props.onSelect(clip.id);
      }}
      title={`Click to select · Drag to move · ${clip.name} · ${(clip.timelineDuration.frames / clip.sourceSampleRate).toFixed(2)} s`}
    >
      <AudioWaveform analysis={props.analysis} clip={clip} />
      <header className={styles.clipHeader}>
        <strong>{clip.name}</strong>
        <span>
          {clip.muted
            ? 'MUTED'
            : clip.loopEnabled
              ? 'LOOP'
              : `${clip.gainDb > 0 ? '+' : ''}${clip.gainDb.toFixed(1)} dB`}
        </span>
      </header>
      <div className={styles.clipGainLine} style={{ top: `${50 - clip.gainDb * 0.7}%` }} />
      {Array.from({ length: loopBoundaries }, (_, index) => (
        <i
          className={styles.loopBoundary}
          key={index}
          style={{
            left: `${(((index + 1) * sourceFrames) / clip.timelineDuration.frames) * 100}%`,
          }}
        />
      ))}
      <span
        data-clip-handle
        className={`${styles.trimHandle} ${styles.trimLeft}`}
        onPointerDown={(event) => props.onTrim(event, clip, 'left')}
      />
      <span
        data-clip-handle
        className={`${styles.trimHandle} ${styles.trimRight}`}
        onPointerDown={(event) => props.onTrim(event, clip, 'right')}
      />
      <span
        data-clip-handle
        className={`${styles.fadeHandle} ${styles.fadeIn}`}
        style={{ left: `${(clip.fadeIn.frames / clip.timelineDuration.frames) * 100}%` }}
        onPointerDown={(event) => props.onFade(event, clip, 'in')}
      />
      <span
        data-clip-handle
        className={`${styles.fadeHandle} ${styles.fadeOut}`}
        style={{ left: `${100 - (clip.fadeOut.frames / clip.timelineDuration.frames) * 100}%` }}
        onPointerDown={(event) => props.onFade(event, clip, 'out')}
      />
    </button>
  );
}
