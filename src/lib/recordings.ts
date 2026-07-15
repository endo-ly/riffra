import type { RecordingAsset, Session, TimelineClip } from './domain';

function relocatedPath(path: string, previousDirectory: string, nextDirectory: string): string {
  const previous = previousDirectory.replace(/[\\/]+$/, '');
  if (path.toLocaleLowerCase() === previous.toLocaleLowerCase()) return nextDirectory;
  const suffix = path.slice(previous.length);
  if (
    !path.toLocaleLowerCase().startsWith(previous.toLocaleLowerCase()) ||
    !/^[\\/]/.test(suffix)
  ) {
    return path;
  }
  return `${nextDirectory}${suffix}`;
}

/** Keeps active-session references valid when an Inbox take is renamed or moved. */
export function relocateRecordingReferences(
  session: Session,
  recording: RecordingAsset,
  nextId: string,
): Session {
  const nextDirectory = nextId.replace(/^recording:/, '');
  const nextName = nextDirectory.split(/[\\/]/).filter(Boolean).at(-1) ?? recording.name;
  return {
    ...session,
    timeline: session.timeline.map((clip) => {
      const assetPath = relocatedPath(clip.assetPath, recording.path, nextDirectory);
      return assetPath === clip.assetPath
        ? clip
        : { ...clip, assetPath, name: clip.name === recording.name ? nextName : clip.name };
    }),
    samplePads: session.samplePads.map((pad) => {
      const assetPath = relocatedPath(pad.assetPath, recording.path, nextDirectory);
      return assetPath === pad.assetPath
        ? pad
        : { ...pad, assetPath, name: pad.name === recording.name ? nextName : pad.name };
    }),
  };
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
  session: Session,
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
