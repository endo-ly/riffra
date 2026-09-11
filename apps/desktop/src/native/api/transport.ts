import type { RuntimeProjectionStatus } from '@/model/domain';
import {
  getHostGeneration,
  getProjectEpoch,
  HostConnectionChangedError,
  invokeHost,
  ProjectChangedError,
} from '../invoke';

interface PendingSeek {
  tick: number;
  waiters: { resolve: () => void; reject: (error: unknown) => void }[];
}

interface TransportCommandQueue {
  hostGeneration: number;
  projectEpoch: number;
  tail: Promise<void>;
  pendingSeek: PendingSeek | null;
  seekTimer: ReturnType<typeof setTimeout> | null;
}

const transportQueues = new Map<string, TransportCommandQueue>();

function currentTransportQueue(): TransportCommandQueue {
  const key = `host:${getHostGeneration()}:project:${getProjectEpoch()}`;
  let queue = transportQueues.get(key);
  if (!queue) {
    queue = {
      hostGeneration: getHostGeneration(),
      projectEpoch: getProjectEpoch(),
      tail: Promise.resolve(),
      pendingSeek: null,
      seekTimer: null,
    };
    transportQueues.set(key, queue);
  }
  return queue;
}

function appendTransportCommand(
  queue: TransportCommandQueue,
  command: string,
  args: Record<string, unknown> = {},
): Promise<void> {
  const operation = queue.tail.then(() => {
    if (queue.hostGeneration !== getHostGeneration()) {
      throw new HostConnectionChangedError();
    }
    if (queue.projectEpoch !== getProjectEpoch()) {
      throw new ProjectChangedError();
    }
    return invokeHost<void>(command, args);
  });
  queue.tail = operation.catch(() => undefined);
  return operation;
}

function flushPendingSeek(queue: TransportCommandQueue): Promise<void> {
  if (queue.seekTimer !== null) {
    clearTimeout(queue.seekTimer);
    queue.seekTimer = null;
  }
  const pending = queue.pendingSeek;
  if (pending === null) return queue.tail;
  queue.pendingSeek = null;
  const operation = appendTransportCommand(queue, 'seek_timeline', { tick: pending.tick });
  void operation.then(
    () => pending.waiters.forEach(({ resolve }) => resolve()),
    (error: unknown) => pending.waiters.forEach(({ reject }) => reject(error)),
  );
  return operation;
}

function queueSeek(tick: number): Promise<void> {
  const queue = currentTransportQueue();
  return new Promise<void>((resolve, reject) => {
    if (queue.pendingSeek === null) {
      queue.pendingSeek = { tick, waiters: [{ resolve, reject }] };
    } else {
      queue.pendingSeek.tick = tick;
      queue.pendingSeek.waiters.push({ resolve, reject });
    }
    if (queue.seekTimer === null) {
      queue.seekTimer = setTimeout(() => {
        queue.seekTimer = null;
        void flushPendingSeek(queue).catch(() => undefined);
      }, 16);
    }
  });
}

function queueTransportCommand(command: string): Promise<void> {
  const queue = currentTransportQueue();
  void flushPendingSeek(queue).catch(() => undefined);
  return appendTransportCommand(queue, command);
}

export async function getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus> {
  return await invokeHost<RuntimeProjectionStatus>('get_runtime_projection_status');
}

export async function retryRuntimeProjection(): Promise<RuntimeProjectionStatus> {
  return await invokeHost<RuntimeProjectionStatus>('retry_runtime_projection');
}

export async function playTimeline(): Promise<void> {
  await queueTransportCommand('play_timeline');
}

export async function stopTimeline(): Promise<void> {
  await queueTransportCommand('stop_timeline');
}

export async function goToStartTimeline(): Promise<void> {
  await queueTransportCommand('go_to_start_timeline');
}

export async function seekTimeline(tick: number): Promise<void> {
  await queueSeek(tick);
}
