import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import type { ControlCommand, ControlCommandResults } from '@/model/generated';
import type { HostConnectionState } from '@/model/domain';

let currentHostGeneration = 0;
let hostConnected = true;

export interface NativeCommandErrorPayload {
  code?: unknown;
  message?: unknown;
  details?: unknown;
}

/** Structured error crossing the Tauri/native command boundary. */
export class NativeCommandError extends Error {
  readonly code: string;
  readonly details: unknown;

  constructor(error: NativeCommandErrorPayload | string | unknown) {
    const payload = typeof error === 'object' && error !== null ? error : undefined;
    const message =
      payload && typeof (payload as NativeCommandErrorPayload).message === 'string'
        ? ((payload as NativeCommandErrorPayload).message as string)
        : typeof error === 'string'
          ? error
          : 'Native command failed';
    super(message);
    this.name = 'NativeCommandError';
    this.code =
      payload && typeof (payload as NativeCommandErrorPayload).code === 'string'
        ? ((payload as NativeCommandErrorPayload).code as string)
        : 'commandFailed';
    this.details = payload ? (payload as NativeCommandErrorPayload).details : undefined;
  }

  get kind(): string | undefined {
    if (typeof this.details !== 'object' || this.details === null) return undefined;
    const kind = (this.details as { kind?: unknown }).kind;
    return typeof kind === 'string' ? kind : undefined;
  }

  get isProjectSwitchFailure(): boolean {
    if (typeof this.details !== 'object' || this.details === null) return false;
    const details = this.details as {
      kind?: unknown;
      projectSwitch?: unknown;
    };
    return (
      details.kind === 'projectSwitchFailed' ||
      (details.kind === 'graphFailed' &&
        typeof details.projectSwitch === 'object' &&
        details.projectSwitch !== null)
    );
  }
}

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  try {
    return await tauriInvoke<T>(command, args);
  } catch (error) {
    if (error instanceof HostConnectionChangedError || error instanceof NativeCommandError) {
      throw error;
    }
    throw new NativeCommandError(error);
  }
}

/** Rejection used when a Host-bound response belongs to a previous connection. */
export class HostConnectionChangedError extends Error {
  constructor() {
    super('Host connection changed while the operation was in flight');
    this.name = 'HostConnectionChangedError';
  }
}

export function adoptHostConnection(state: HostConnectionState): boolean {
  if (state.generation < currentHostGeneration) return false;
  currentHostGeneration = state.generation;
  hostConnected = state.mode !== 'disconnected';
  return true;
}

/**
 * Thin bridge to Tauri. Ordering that affects Session or Runtime correctness
 * is owned by the Rust Core and Runtime Reconciler; this module only
 * coalesces high-frequency UI updates through invokeLatestHost below.
 */
/** Invokes a command and rejects a response that crossed a Host switch. */
export async function invokeHost<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (!hostConnected) {
    throw new HostConnectionChangedError();
  }
  const generation = currentHostGeneration;
  try {
    const value = await invoke<T>(command, args);
    if (generation !== currentHostGeneration) throw new HostConnectionChangedError();
    return value;
  } catch (error) {
    if (generation !== currentHostGeneration) throw new HostConnectionChangedError();
    throw error;
  }
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

/** Coalesces Host-owned high-frequency updates with a Host connection guard. */
function invokeLatestHost<T>(
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

/** The value returned by the Host for one Control Command. */
export type ControlResult<C extends ControlCommand> = ControlCommandResults[C['command'] &
  keyof ControlCommandResults];

function controlArgs(command: ControlCommand): Record<string, unknown> {
  return { command: command.command, params: command.params };
}

/** Runs one Control Command through the connected Host. */
export function dispatchControl<C extends ControlCommand>(command: C): Promise<ControlResult<C>> {
  return invokeHost<ControlResult<C>>('dispatch_control', controlArgs(command));
}

/** Runs the latest of the Control Commands queued under `key`. */
export function dispatchLatestControl<C extends ControlCommand>(
  command: C,
  key: string,
): Promise<ControlResult<C>> {
  return invokeLatestHost<ControlResult<C>>('dispatch_control', controlArgs(command), key);
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
