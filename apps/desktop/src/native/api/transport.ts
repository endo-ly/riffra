import type { RuntimeProjectionStatus } from '@/model/domain';
import { invokeHost } from '../invoke';

export async function getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus> {
  return await invokeHost<RuntimeProjectionStatus>('get_runtime_projection_status');
}

export async function retryRuntimeProjection(): Promise<RuntimeProjectionStatus> {
  return await invokeHost<RuntimeProjectionStatus>('retry_runtime_projection');
}

export async function playTimeline(transportSequence: number): Promise<void> {
  await invokeHost<void>('play_timeline', { transportSequence });
}

export async function stopTimeline(transportSequence: number): Promise<void> {
  await invokeHost<void>('stop_timeline', { transportSequence });
}

export async function goToStartTimeline(transportSequence: number): Promise<void> {
  await invokeHost<void>('go_to_start_timeline', { transportSequence });
}

export async function seekTimeline(tick: number): Promise<void> {
  await invokeHost<void>('seek_timeline', { tick });
}
