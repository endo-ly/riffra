import type { Track, TransportStatus } from '@/model/generated';
import { useSession } from '@/shared/session/session-context';
import { TRACK_HEADER_WIDTH } from './model/timeline-layout';
import styles from './TimelineView.module.css';

interface RecordingPreviewProps {
  pixelsPerTick: number;
  tracks: readonly Track[];
}

/** The take the transport is writing, drawn over the lanes while it records. */
export function RecordingPreview({ pixelsPerTick, tracks }: RecordingPreviewProps) {
  const status = useSession((state) => state.transport.status);
  if (!status || status.recordingPhase === 'idle') return null;
  return <RecordingExtent status={status} pixelsPerTick={pixelsPerTick} tracks={tracks} />;
}

/** Reads the moving playhead, so only the take grows as the transport records. */
function RecordingExtent({
  status,
  pixelsPerTick,
  tracks,
}: RecordingPreviewProps & { status: TransportStatus }) {
  const displayTick = useSession((state) => state.transport.displayTick);
  const armedNames = status.armedTrackIds
    .map((trackId) => tracks.find((track) => track.id === trackId)?.name ?? trackId)
    .join(' · ');
  return (
    <div
      className={styles.recordingPreview}
      style={{
        left: TRACK_HEADER_WIDTH + status.recordingStartTick * pixelsPerTick,
        width: Math.max(1, displayTick - status.recordingStartTick) * pixelsPerTick,
      }}
    >
      {status.recordingPhase.toUpperCase()} · {armedNames} · PASS {status.recordingPassOrdinal}
    </div>
  );
}
