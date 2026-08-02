import { invoke as tauriInvoke } from '@tauri-apps/api/core';

// Tauri can dispatch several async commands at once. Canonical-session
// mutations are still kept in one FIFO for the application operations that
// read, persist, and return a complete session snapshot. Workspace navigation
// is deliberately outside this list: it is view state and its Rust operation
// no longer enters durable Session persistence.
// A command belongs to the canonical Session lane, the runtime lane, or both.
// Keeping the overlap in one list prevents a command from silently drifting
// out of one of the ordering boundaries when a new operation is added.
const canonicalOnlyCommands = [
  'save_scratch_session',
  'restore_recovery_generation',
  'import_scratch_session',
  'create_sample_pad',
  'update_sample_pad',
  'remove_sample_pad',
  'set_track_instrument',
  'clear_track_instrument',
  'persist_track_plugin_state',
  'persist_track_plugin_parameter',
  'set_master_gain_db',
  'start_recording',
  'stop_recording',
  'map_rack_macro',
  'capture_snapshot',
];

const runtimeOnlyCommands = [
  'restore_current_rack',
  'open_plugin_editor',
  'open_track_plugin_editor',
];

// Runtime graph operations need a second FIFO for legacy Play-rack and editor
// commands that still perform a synchronous native transaction. Arrangement
// projection is owned by the backend Runtime Reconciler, and transport/play
// commands have their own critical path, so neither is allowed to wait behind
// a VST construction task here.
const canonicalAndRuntimeCommands = [
  'add_audio_clip_to_arrangement',
  'add_midi_clip_to_arrangement',
  'update_audio_clip',
  'remove_timeline_clips',
  'trim_audio_clip',
  'split_audio_clip',
  'duplicate_audio_clip',
  'move_audio_clips',
  'update_midi_clip',
  'move_midi_clips',
  'trim_midi_clip',
  'split_midi_clip',
  'duplicate_midi_clip',
  'paste_timeline_clips',
  'crossfade_audio_clips',
  'update_arrangement_timebase',
  'update_timeline_loop_range',
  'update_timeline_punch_range',
  'update_session_settings',
  'add_track',
  'update_track',
  'set_track_automation',
  'set_track_audio_input',
  'set_track_midi_input',
  'add_track_effect',
  'remove_track_effect',
  'reorder_track_effects',
  'set_track_device_bypassed',
  'set_track_device_parameter',
  'remove_track',
  'duplicate_track',
  'reorder_track',
  'add_marker',
  'update_marker',
  'remove_marker',
  'add_midi_note',
  'update_midi_note',
  'update_midi_notes',
  'remove_midi_note',
  'quantize_midi_notes',
  'duplicate_midi_notes',
  'set_audio_clip_take_variant',
  'activate_take',
  'place_take_as_separate_clip',
  'start_arrange_recording',
  'record_another_take',
  'stop_arrange_recording',
  'relink_missing_dependency',
  'disable_missing_plugin',
  'replace_missing_track_plugin',
  'load_plugin_into_rack',
  'clear_plugin_from_rack',
  'set_rack_plugin_bypassed',
  'set_rack_plugin_parameter',
  'set_rack_macro_value',
  'recall_snapshot',
  'load_rack_definition_asset',
];

const serializedCommands = new Set([...canonicalOnlyCommands, ...canonicalAndRuntimeCommands]);
const runtimeSerializedCommands = new Set([...runtimeOnlyCommands, ...canonicalAndRuntimeCommands]);

let serializedTail: Promise<void> = Promise.resolve();
let runtimeSerializedTail: Promise<void> = Promise.resolve();

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
 * Invokes a native command, serializing canonical mutations while allowing
 * independent probes and meter/status reads to run immediately.
 */
export function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!isNativeRuntime()) {
    return tauriInvoke<T>(command, args);
  }

  const waits: Promise<void>[] = [];
  const canonical = serializedCommands.has(command);
  const runtime = runtimeSerializedCommands.has(command);
  if (canonical) waits.push(serializedTail.catch(() => undefined));
  if (runtime) waits.push(runtimeSerializedTail.catch(() => undefined));
  if (waits.length === 0) return tauriInvoke<T>(command, args);

  const operation = Promise.all(waits).then(() => tauriInvoke<T>(command, args));
  const completed = operation.then(
    () => undefined,
    () => undefined,
  );
  if (canonical) serializedTail = completed;
  if (runtime) runtimeSerializedTail = completed;
  return operation;
}

/**
 * Coalesces a burst of same-key value updates before entering the native FIFO.
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
        const result = await invoke<T>(command, args);
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
