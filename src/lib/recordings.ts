import type { RecordingAsset, ScratchSession, TimelineClip } from './domain';

export function isUsableRecording(recording: RecordingAsset): boolean {
  return (
    recording.state === 'completed' &&
    !recording.error &&
    Boolean(recording.rawPath) &&
    Boolean(recording.processedPath) &&
    recording.samplesWritten > 0 &&
    (recording.sampleRate ?? 0) > 0
  );
}

export function createTimelineClip(
  session: ScratchSession,
  recording: RecordingAsset,
): TimelineClip | null {
  if (!isUsableRecording(recording) || !recording.processedPath) return null;
  if (session.timeline.some((clip) => clip.assetPath === recording.processedPath)) return null;
  const startMs = session.timeline.reduce(
    (end, clip) => Math.max(end, clip.startMs + clip.durationMs),
    0,
  );
  const durationMs = Math.max(
    1,
    Math.round((recording.samplesWritten / (recording.sampleRate ?? 1)) * 1_000),
  );
  return {
    id: `clip:${recording.id}`,
    assetPath: recording.processedPath,
    name: recording.name,
    trackId: session.tracks[0]?.id ?? 'main',
    startMs,
    durationMs,
    sourceInMs: 0,
    sourceOutMs: 0,
    loopEnabled: false,
    gainDb: 0,
    fadeInMs: 0,
    fadeOutMs: 0,
    pan: 0,
    muted: false,
  };
}
