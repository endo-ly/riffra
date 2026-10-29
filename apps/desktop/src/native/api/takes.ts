import type { ArrangementMutationResult, AudioStatus, AudioTakeVariant } from '@/model/domain';
import { dispatchControl } from '../invoke';

export interface TakeApi {
  setAudioClipTakeVariant(
    clipId: string,
    variant: AudioTakeVariant,
  ): Promise<ArrangementMutationResult>;
  startTakeComparison(takeId: string): Promise<AudioStatus>;
  switchTakeComparisonVariant(variant: AudioTakeVariant): Promise<AudioStatus>;
  stopTakeComparison(): Promise<AudioStatus>;
  activateTake(sessionId: string, takeId: string): Promise<ArrangementMutationResult>;
  placeTakeAsSeparateClip(takeId: string): Promise<ArrangementMutationResult>;
}

async function setAudioClipTakeVariant(
  clipId: string,
  variant: AudioTakeVariant,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'audio-clip.take-variant.set', params: { clipId, variant } });
}

async function startTakeComparison(takeId: string): Promise<AudioStatus> {
  return dispatchControl({ command: 'take.comparison.start', params: { takeId } });
}

async function switchTakeComparisonVariant(variant: AudioTakeVariant): Promise<AudioStatus> {
  return dispatchControl({ command: 'take.comparison.switch', params: { variant } });
}

async function stopTakeComparison(): Promise<AudioStatus> {
  return dispatchControl({ command: 'take.comparison.stop', params: {} });
}

async function activateTake(sessionId: string, takeId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'take.activate', params: { sessionId, takeId } });
}

async function placeTakeAsSeparateClip(takeId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'take.place-separate-clip', params: { takeId } });
}

export const takesApi: TakeApi = {
  setAudioClipTakeVariant,
  startTakeComparison,
  switchTakeComparisonVariant,
  stopTakeComparison,
  activateTake,
  placeTakeAsSeparateClip,
};
