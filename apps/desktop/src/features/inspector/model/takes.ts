import type { AudioTakeVariant, CreativeSession, RecordingTakeRecord } from '@/model/generated';

type RecordingSession = CreativeSession['arrangement']['recordingSessions'][number];

function passOrdinal(session: RecordingSession, arrangement: CreativeSession['arrangement']) {
  return session.passIds.reduce((latest, passId) => {
    const pass = arrangement.recordingPasses.find((item) => item.id === passId);
    return Math.max(latest, pass?.ordinal ?? -1);
  }, -1);
}

export function compareRecordingSessions(
  first: RecordingSession,
  second: RecordingSession,
  arrangement: CreativeSession['arrangement'],
) {
  return (
    passOrdinal(first, arrangement) - passOrdinal(second, arrangement) ||
    first.startTick - second.startTick ||
    first.id.localeCompare(second.id)
  );
}

export function compareTakes(
  first: RecordingTakeRecord,
  second: RecordingTakeRecord,
  arrangement: CreativeSession['arrangement'],
) {
  const firstPass = arrangement.recordingPasses.find((pass) => pass.id === first.passId);
  const secondPass = arrangement.recordingPasses.find((pass) => pass.id === second.passId);
  return (
    (firstPass?.ordinal ?? -1) - (secondPass?.ordinal ?? -1) ||
    first.startTick - second.startTick ||
    first.id.localeCompare(second.id)
  );
}

export function sourceForVariant(take: RecordingTakeRecord, variant: AudioTakeVariant) {
  return variant === 'raw'
    ? (take.rawAudio ?? take.processedAudio)
    : (take.processedAudio ?? take.rawAudio);
}

export function defaultVariantForTake(
  arrangement: CreativeSession['arrangement'],
  take: RecordingTakeRecord,
  selectedClipIds: string[],
): AudioTakeVariant {
  const selectedClip = arrangement.audioClips.find(
    (clip) => selectedClipIds.includes(clip.id) && clip.recordingTakeId === take.id,
  );
  const relatedClip =
    selectedClip ?? arrangement.audioClips.find((clip) => clip.recordingTakeId === take.id);
  if (relatedClip) return relatedClip.takeVariant;
  return take.processedAudio ? 'processed' : 'raw';
}
