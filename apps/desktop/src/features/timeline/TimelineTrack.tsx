import type { EditorCommit } from './hooks/useTimelineEditor';
import type { CSSProperties } from 'react';
import type { AudioAnalysis, AudioClip, MidiClip, ProjectTimebase, Track } from '@/model/generated';
import { trackLaneHeight, type TrackTimeline, type TrackSize } from './model/timeline-layout';
import { resolveTrackColor } from '@/model/track-color';
import { TrackHeader } from './TrackHeader';
import { TrackLane } from './TrackLane';
import styles from './TimelineView.module.css';

export interface TimelineTrackProps {
  sessionId: string;
  track: Track;
  trackIndex?: number;
  timeline: TrackTimeline;
  timebase: ProjectTimebase;
  analyses: Record<string, AudioAnalysis | null>;
  selectedClipIds: string[];
  selected: boolean;
  focused: boolean;
  /** Whether any track in the session is soloed, which silences the others. */
  soloActive: boolean;
  unavailableClipIds: string[];
  timelineWidth: number;
  pixelsPerTick: number;
  trackSize: TrackSize;
  onCommit: EditorCommit;
  onDrop: (event: React.DragEvent, trackId: string, trackKind: Track['kind']) => void;
  onContextMenu?: (event: React.MouseEvent, trackId: string, tick: number) => void;
  onMove: (event: React.PointerEvent<HTMLButtonElement>, clip: AudioClip) => void;
  onMoveMidi: (event: React.PointerEvent<HTMLButtonElement>, clip: MidiClip) => void;
  onTrimMidi: (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: MidiClip,
    side: 'left' | 'right',
  ) => void;
  onSelect: (clipId: string, append?: boolean) => void;
  onSelectTrack: () => void;
  onTrim: (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: AudioClip,
    side: 'left' | 'right',
  ) => void;
  onFade: (event: React.PointerEvent<HTMLSpanElement>, clip: AudioClip, side: 'in' | 'out') => void;
  onOpenMidiEditor?: (clip: MidiClip) => void;
  onDoubleClickLane?: (event: React.MouseEvent, trackId: string, tick: number) => void;
  onAudioClipContextMenu?: (event: React.MouseEvent, clip: AudioClip) => void;
  onMidiClipContextMenu?: (event: React.MouseEvent, clip: MidiClip) => void;
  onRename: (name: string) => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onReorder: (sourceTrackId: string, insertAfter: boolean) => void;
  onSetTrackSize?: (size: TrackSize) => void;
  onOpenDevices: () => void;
}

export function TimelineTrack(props: TimelineTrackProps) {
  const audible = !props.track.muted && (!props.soloActive || props.track.solo);
  return (
    <div
      className={styles.trackRow}
      style={
        {
          '--track-height': `${props.timeline.laneCount * trackLaneHeight(props.trackSize)}px`,
          '--track-color': resolveTrackColor(props.track, props.trackIndex ?? 0),
        } as CSSProperties
      }
      data-arrange-track
      data-track-id={props.track.id}
      data-selected={props.selected || undefined}
      data-focused={props.focused || undefined}
      data-inaudible={!audible || undefined}
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => props.onDrop(event, props.track.id, props.track.kind)}
    >
      <TrackHeader {...props} />
      <TrackLane {...props} />
    </div>
  );
}
