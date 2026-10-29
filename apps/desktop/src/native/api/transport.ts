import type { RuntimeProjectionStatus } from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface TransportApi {
  getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus>;
  retryRuntimeProjection(): Promise<RuntimeProjectionStatus>;
  playTimeline(): Promise<void>;
  pauseTimeline(): Promise<void>;
  stopTimeline(): Promise<void>;
  goToStartTimeline(): Promise<void>;
  seekTimeline(tick: number): Promise<void>;
}

async function getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus> {
  return dispatchControl({ command: 'runtime.projection.get', params: {} });
}

async function retryRuntimeProjection(): Promise<RuntimeProjectionStatus> {
  return dispatchControl({ command: 'runtime.projection.retry', params: {} });
}

async function playTimeline(): Promise<void> {
  await dispatchControl({ command: 'transport.play', params: {} });
}

async function pauseTimeline(): Promise<void> {
  await dispatchControl({ command: 'transport.pause', params: {} });
}

async function stopTimeline(): Promise<void> {
  await dispatchControl({ command: 'transport.stop', params: {} });
}

async function goToStartTimeline(): Promise<void> {
  await dispatchControl({ command: 'transport.go-to-start', params: {} });
}

async function seekTimeline(tick: number): Promise<void> {
  await dispatchControl({ command: 'transport.seek', params: { tick } });
}

export const transportApi: TransportApi = {
  getRuntimeProjectionStatus,
  retryRuntimeProjection,
  playTimeline,
  pauseTimeline,
  stopTimeline,
  goToStartTimeline,
  seekTimeline,
};
