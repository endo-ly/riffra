import type { RuntimeProjectionStatus } from '@/model/domain';
import { invoke } from '../invoke';

export async function retryRuntimeProjection(): Promise<RuntimeProjectionStatus> {
  return await invoke<RuntimeProjectionStatus>('retry_runtime_projection');
}

export async function playTimeline(transportSequence: number): Promise<void> {
  await invoke<void>('play_timeline', { transportSequence });
}

export async function stopTimeline(transportSequence: number): Promise<void> {
  await invoke<void>('stop_timeline', { transportSequence });
}

export async function goToStartTimeline(transportSequence: number): Promise<void> {
  await invoke<void>('go_to_start_timeline', { transportSequence });
}

export async function seekTimeline(tick: number): Promise<void> {
  await invoke<void>('seek_timeline', { tick });
}
