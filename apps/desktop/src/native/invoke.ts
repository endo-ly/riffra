import { invoke as tauriInvoke } from '@tauri-apps/api/core';

/**
 * Thin bridge to Tauri. Ordering that affects Session or Runtime correctness
 * is owned by the Rust Core and Runtime Reconciler; this module only
 * coalesces high-frequency UI updates through invokeLatest below.
 */
export function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return tauriInvoke<T>(command, args);
}

let mutationTail: Promise<void> = Promise.resolve();

/**
 * Serializes canonical production mutations at the native Adapter boundary.
 * Each caller receives the result of its own operation, in commit order, so
 * Presentation code never compares Session timestamps or merges snapshots.
 */
export function invokeMutation<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const result = mutationTail.then(() => invoke<T>(command, args));
  mutationTail = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}

interface LatestWaiter<T> {
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
}

interface LatestQueue<T> {
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

/**
 * Coalesces a burst of same-key value updates before entering the native bridge.
 * The last payload is authoritative; all callers from the same burst receive
 * that canonical response. This is used for controls such as track mute/solo
 * where sending every intermediate click only creates persistence and runtime
 * work that the user can no longer observe.
 */
export function invokeLatest<T>(
  command: string,
  args: Record<string, unknown>,
  key: string,
): Promise<T> {
  let queue = latestQueues.get(key) as LatestQueue<T> | undefined;
  if (!queue) {
    queue = {
      pendingArgs: null,
      waiters: [],
      running: false,
      timer: null,
    };
    latestQueues.set(key, queue as LatestQueue<unknown>);
  }

  const promise = new Promise<T>((resolve, reject) => {
    queue!.pendingArgs = args;
    queue!.waiters.push({ resolve, reject });
  });

  if (!queue.running && queue.timer === null) {
    queue.timer = setTimeout(() => {
      queue!.timer = null;
      void drainLatestQueue(command, key, queue!);
    }, 16);
  }
  return promise;
}

async function drainLatestQueue<T>(
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
      try {
        const result = await invokeMutation<T>(command, args);
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
 * Invokes a Tauri command when the native runtime is available, otherwise
 * returns `fallback`. Reserved for commands whose only silently-masked failure
 * mode is "the native runtime is absent" (browser preview, smoke tests).
 *
 * Production failures (the Rust command returned `Err`) are not swallowed:
 * they propagate as a rejected Promise so the caller can surface them instead
 * of collapsing a real error into an empty list or null.
 */
export async function invokeOrFallback<T>(
  command: string,
  args: Record<string, unknown>,
  fallback: T,
): Promise<T> {
  if (!isNativeRuntime()) return fallback;
  return invoke<T>(command, args);
}

/** Serializes a canonical mutation while retaining browser-preview fallback. */
export function invokeMutationOrFallback<T>(
  command: string,
  args: Record<string, unknown>,
  fallback: T,
): Promise<T> {
  if (!isNativeRuntime()) return Promise.resolve(fallback);
  return invokeMutation<T>(command, args);
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
