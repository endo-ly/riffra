import type { RuntimeProjectionStatus } from '@/model/domain';
import { dispatchControl } from '../invoke';

export async function getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus> {
  return dispatchControl({ command: 'runtime.projection.get', params: {} });
}

export async function retryRuntimeProjection(): Promise<RuntimeProjectionStatus> {
  return dispatchControl({ command: 'runtime.projection.retry', params: {} });
}

export async function playTimeline(): Promise<void> {
  await dispatchControl({ command: 'transport.play', params: {} });
}

export async function stopTimeline(): Promise<void> {
  await dispatchControl({ command: 'transport.stop', params: {} });
}

export async function goToStartTimeline(): Promise<void> {
  await dispatchControl({ command: 'transport.go-to-start', params: {} });
}

export async function seekTimeline(tick: number): Promise<void> {
  await dispatchControl({ command: 'transport.seek', params: { tick } });
}
