import { useState } from 'react';
import { AudioClipView } from './AudioClipView';
import { MidiClipView } from './MidiClipView';
import {
  trackLaneHeight,
  type TimelineAudioItem,
  type TimelineMidiItem,
} from './model/timeline-layout';
import { isBrowserItemDrag } from './hooks/useTimelineDrop';
import type { TimelineTrackProps } from './TimelineTrack';
import styles from './TrackLane.module.css';

export function TrackLane(props: TimelineTrackProps) {
  const [dragOver, setDragOver] = useState(false);
  const audioItems = props.timeline.items.filter(
    (item): item is TimelineAudioItem => item.kind === 'audio',
  );
  const midiItems = props.timeline.items.filter(
    (item): item is TimelineMidiItem => item.kind === 'midi',
  );
  const laneHeight = trackLaneHeight(props.trackSize);
  return (
    <div
      className={`${styles.lane} ${dragOver ? styles.laneDragOver : ''}`}
      data-selected={props.selected || undefined}
      data-inaudible={props.track.muted || (props.soloActive && !props.track.solo) || undefined}
      data-midi-empty-lane={props.track.kind === 'instrument' ? true : undefined}
      style={{ width: props.timelineWidth }}
      onDragOver={(event) => {
        if (!isBrowserItemDrag(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = 'copy';
      }}
      onDragEnter={() => setDragOver(true)}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node)) setDragOver(false);
      }}
      onDrop={(event) => {
        event.stopPropagation();
        setDragOver(false);
        props.onDrop(event, props.track.id, props.track.kind);
      }}
      onContextMenu={(event) => {
        if ((event.target as HTMLElement).closest('[data-clip-id]')) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        const tick = Math.max(0, (event.clientX - bounds.left) / props.pixelsPerTick);
        props.onContextMenu?.(event, props.track.id, tick);
      }}
      onDoubleClick={(event) => {
        if ((event.target as HTMLElement).closest('[data-clip-id]')) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        const tick = Math.max(0, (event.clientX - bounds.left) / props.pixelsPerTick);
        props.onDoubleClickLane?.(event, props.track.id, tick);
      }}
    >
      {audioItems.map(({ clip, key }) => (
        <AudioClipView
          key={key}
          clip={clip}
          analysis={props.analyses[clip.assetId]}
          timebase={props.timebase}
          pixelsPerTick={props.pixelsPerTick}
          lane={props.timeline.lanes.get(key) ?? 0}
          laneHeight={laneHeight}
          selected={props.selectedClipIds.includes(clip.id)}
          missing={props.unavailableClipIds.includes(clip.id)}
          onSelect={props.onSelect}
          onMove={props.onMove}
          onTrim={props.onTrim}
          onFade={props.onFade}
          onContextMenu={props.onAudioClipContextMenu}
        />
      ))}
      {midiItems.map(({ clip, key }) => (
        <MidiClipView
          key={key}
          clip={clip}
          pixelsPerTick={props.pixelsPerTick}
          lane={props.timeline.lanes.get(key) ?? 0}
          laneHeight={laneHeight}
          selected={props.selectedClipIds.includes(clip.id)}
          onSelect={props.onSelect}
          onMove={props.onMoveMidi}
          onTrim={props.onTrimMidi}
          onOpenEditor={props.onOpenMidiEditor}
          onContextMenu={props.onMidiClipContextMenu}
        />
      ))}
    </div>
  );
}
