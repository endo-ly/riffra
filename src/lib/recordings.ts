import type { AssetId, AudioClip, CreativeSession, RecordingAsset } from './domain';

/** Keeps active-session references valid when an Inbox take is renamed or moved. */
export function relocateRecordingReferences(
  session: CreativeSession,
  recording: RecordingAsset,
  nextId: string,
): CreativeSession {
  // Asset references are stable across recording-folder moves. The native
  // recording operation updates the canonical Asset location; the session
  // itself must not be rewritten to carry a path.
  void recording;
  void nextId;
  return session;
}

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
  session: CreativeSession,
  recording: RecordingAsset,
  assetId: AssetId,
): AudioClip | null {
  if (!isUsableRecording(recording) || !recording.processedPath) return null;
  if (session.arrangement.audioClips.some((clip) => clip.assetId === assetId)) return null;
  const startMs = session.arrangement.audioClips.reduce(
    (end, clip) => Math.max(end, clip.positionMs + clip.durationMs),
    0,
  );
  const durationMs = Math.max(
    1,
    Math.round((recording.samplesWritten / (recording.sampleRate ?? 1)) * 1_000),
  );
  return {
    id: `clip:${recording.id}`,
    name: recording.name,
    trackId: session.arrangement.tracks[0]?.id ?? 'main',
    assetId,
    positionMs: startMs,
    durationMs,
    sourceStartMs: 0,
    sourceEndMs: 0,
    gainDb: 0,
    pan: 0,
    fadeInMs: 0,
    fadeOutMs: 0,
    loopEnabled: false,
    muted: false,
  };
}
