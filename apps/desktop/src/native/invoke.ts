import { invoke as tauriInvoke } from '@tauri-apps/api/core';

let currentHostGeneration = 0;
let hostConnected = true;

/** Rejection used when a Host-bound response belongs to a previous connection. */
export class HostConnectionChangedError extends Error {
  constructor() {
    super('Host connection changed while the operation was in flight');
    this.name = 'HostConnectionChangedError';
  }
}

export function setHostGeneration(generation: number): void {
  currentHostGeneration = generation;
}

export function setHostConnectionAvailability(connected: boolean): void {
  hostConnected = connected;
}

export function getHostGeneration(): number {
  return currentHostGeneration;
}

/**
 * Thin bridge to Tauri. Ordering that affects Session or Runtime correctness
 * is owned by the Rust Core and Runtime Reconciler; this module only
 * coalesces high-frequency UI updates through invokeLatestHost below.
 */
export function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return tauriInvoke<T>(command, args);
}

/** Invokes a command and rejects a response that crossed a Host switch. */
export async function invokeHost<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (!hostConnected) {
    throw new HostConnectionChangedError();
  }
  const generation = currentHostGeneration;
  const value = await tauriInvoke<T>(command, args);
  if (generation !== currentHostGeneration) {
    throw new HostConnectionChangedError();
  }
  return value;
}

interface LatestWaiter<T> {
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
}

interface LatestQueue<T> {
  generation: number;
  pendingArgs: Record<string, unknown> | null;
  waiters: LatestWaiter<T>[];
  running: boolean;
  timer: ReturnType<typeof setTimeout> | null;
}

const latestQueues = new Map<string, LatestQueue<unknown>>();

/**
 * Whether the Tauri runtime bridge is available. False in the browser preview
 * (Vite dev server without the Tauri shell), true inside the Tauri app. Tauri
 * injects `__TAURI_INTERNALS__` on `window` when the native bridge is live.
 */
export function isNativeRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Browser-compatible fallback wrapper for Host-owned commands. */
export async function invokeHostOrFallback<T>(
  command: string,
  args: Record<string, unknown>,
  fallback: T,
): Promise<T> {
  if (!isNativeRuntime()) return fallback;
  return invokeHost<T>(command, args);
}

/** Coalesces Host-owned high-frequency updates with a generation guard. */
export function invokeLatestHost<T>(
  command: string,
  args: Record<string, unknown>,
  key: string,
): Promise<T> {
  const generation = currentHostGeneration;
  const queueKey = `host:${generation}:${key}`;
  let queue = latestQueues.get(queueKey) as LatestQueue<T> | undefined;
  if (!queue) {
    queue = {
      generation,
      pendingArgs: null,
      waiters: [],
      running: false,
      timer: null,
    };
    latestQueues.set(queueKey, queue as LatestQueue<unknown>);
  }
  const promise = new Promise<T>((resolve, reject) => {
    queue!.pendingArgs = args;
    queue!.waiters.push({ resolve, reject });
  });
  if (!queue.running && queue.timer === null) {
    queue.timer = setTimeout(() => {
      queue!.timer = null;
      void drainLatestHostQueue(command, queueKey, queue!);
    }, 16);
  }
  return promise;
}

async function drainLatestHostQueue<T>(
  command: string,
  key: string,
  queue: LatestQueue<T>,
): Promise<void> {
  queue.running = true;
  try {
    while (queue.pendingArgs !== null) {
      const args = queue.pendingArgs;
      queue.pendingArgs = null;
      const waiters = queue.waiters.splice(0);
      if (queue.generation !== currentHostGeneration) {
        const error = new HostConnectionChangedError();
        waiters.forEach(({ reject }) => reject(error));
        continue;
      }
      try {
        const result = await invokeHost<T>(command, args);
        waiters.forEach(({ resolve }) => resolve(result));
      } catch (error) {
        waiters.forEach(({ reject }) => reject(error));
      }
    }
  } finally {
    queue.running = false;
    if (queue.pendingArgs === null && queue.waiters.length === 0 && queue.timer === null) {
      latestQueues.delete(key);
    }
  }
}

/**
 * Default rejection handler for fire-and-forget NativeApi calls. Logs the error
 * so a production failure does not disappear as an unhandled Promise rejection.
 * Commands that own a richer error surface (audio status, autosave error) keep
 * their own handlers; this is the floor for everything else.
 */
export function logNativeError(label: string): (error: unknown) => void {
  return (error) => {
    console.error(`[native] ${label} failed:`, error);
  };
}
