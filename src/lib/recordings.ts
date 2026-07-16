import type { RecordingAsset } from './domain';

export function isUsableRecording(recording: RecordingAsset): boolean {
  return (
    recording.state === 'completed' &&
    !recording.error &&
    Boolean(recording.rawAssetId) &&
    Boolean(recording.processedAssetId) &&
    recording.samplesWritten > 0 &&
    (recording.sampleRate ?? 0) > 0
  );
}
