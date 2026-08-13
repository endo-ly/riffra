import type { AssetId, SessionAudioPair } from '@/model/domain';
import { invoke } from '../invoke';

export async function createSamplePad(assetId: AssetId, name: string): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('create_sample_pad', {
    assetId,
    name,
  });
}

export async function updateSamplePad(
  padId: string,
  patch: { startMs?: number; endMs?: number; gainDb?: number; loopEnabled?: boolean },
): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('update_sample_pad', {
    padId,
    patch,
  });
}

export async function removeSamplePad(padId: string): Promise<SessionAudioPair> {
  return invoke<SessionAudioPair>('remove_sample_pad', {
    padId,
  });
}
