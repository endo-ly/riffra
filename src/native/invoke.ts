import { invoke as tauriInvoke } from '@tauri-apps/api/core';

/**
 * Whether the Tauri runtime bridge is available. False in the browser preview
 * (Vite dev server without the Tauri shell), true inside the Tauri app. Tauri
 * injects `__TAURI_INTERNALS__` on `window` when the native bridge is live.
 */
export function isNativeRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
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
  return tauriInvoke<T>(command, args);
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
