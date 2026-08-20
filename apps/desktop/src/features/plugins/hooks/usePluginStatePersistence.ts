import { useEffect, useRef } from 'react';
import type { CreativeSession } from '@/model/domain';
import type { ArrangeApi, NativeEventApi } from '@/native/native-api';
import { getCurrentWindow } from '@/native/window';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

type PluginPersistenceApi = Pick<
  ArrangeApi,
  'persistTrackPluginState' | 'persistTrackPluginParameter'
> &
  Pick<NativeEventApi, 'onTrackPluginStateChanged' | 'onTrackPluginParameterChanged'>;

interface UsePluginStatePersistenceOptions {
  api: PluginPersistenceApi;
  setSession: (session: CreativeSession) => void;
  setAutosaveError: (message: string | null) => void;
}

/** Persists plugin editor changes and flushes the final batch on window close. */
export function usePluginStatePersistence({
  api,
  setSession,
  setAutosaveError,
}: UsePluginStatePersistenceOptions) {
  const {
    onTrackPluginParameterChanged,
    onTrackPluginStateChanged,
    persistTrackPluginParameter,
    persistTrackPluginState,
  } = api;
  const pendingPluginChanges = useRef(
    new Map<
      string,
      {
        trackId: string;
        deviceId: string;
        parameters: Map<number, number>;
        state: Parameters<typeof persistTrackPluginState>[0] | null;
      }
    >(),
  );

  useEffect(() => {
    let pluginSaveTimer: ReturnType<typeof setTimeout> | null = null;
    let pluginSaveRunning = false;
    let pluginSaveFlushRequested = false;
    let pluginSaveCompletion: Promise<boolean> | null = null;
    let closeRequested = false;
    type PluginChangeBatch = [
      string,
      {
        trackId: string;
        deviceId: string;
        parameters: Map<number, number>;
        state: Parameters<typeof persistTrackPluginState>[0] | null;
      },
    ];

    const mergeFailedPluginBatch = (batch: PluginChangeBatch[]) => {
      for (const [key, failed] of batch) {
        const current = pendingPluginChanges.current.get(key);
        if (current == null) {
          pendingPluginChanges.current.set(key, {
            trackId: failed.trackId,
            deviceId: failed.deviceId,
            parameters: new Map(failed.parameters),
            state: failed.state,
          });
          continue;
        }
        if (current.state == null && failed.state != null) current.state = failed.state;
        if (current.state == null) {
          for (const [parameterIndex, value] of failed.parameters) {
            if (!current.parameters.has(parameterIndex)) {
              current.parameters.set(parameterIndex, value);
            }
          }
        }
      }
    };

    const runPluginFlush = async (): Promise<boolean> => {
      let succeeded = true;
      try {
        do {
          pluginSaveFlushRequested = false;
          const batch = [...pendingPluginChanges.current.entries()] as PluginChangeBatch[];
          pendingPluginChanges.current.clear();
          if (batch.length === 0) break;
          try {
            let latest: CreativeSession | null = null;
            let projectionError: string | null = null;
            for (const [, pending] of batch) {
              if (pending.state != null) {
                applyArrangementMutation(
                  await persistTrackPluginState(pending.state),
                  (session) => {
                    latest = session;
                  },
                  (message) => {
                    projectionError = message;
                  },
                );
              }
              for (const [parameterIndex, value] of pending.parameters) {
                applyArrangementMutation(
                  await persistTrackPluginParameter({
                    trackId: pending.trackId,
                    deviceId: pending.deviceId,
                    parameterIndex,
                    value,
                  }),
                  (session) => {
                    latest = session;
                  },
                  (message) => {
                    projectionError = message;
                  },
                );
              }
            }
            if (latest != null) {
              setSession(latest);
              setAutosaveError(projectionError);
            }
          } catch (error: unknown) {
            mergeFailedPluginBatch(batch);
            succeeded = false;
            setAutosaveError(
              error instanceof Error
                ? error.message
                : `Track Plugin state could not be saved: ${String(error)}`,
            );
            break;
          }
        } while (pluginSaveFlushRequested || pendingPluginChanges.current.size > 0);
      } finally {
        pluginSaveRunning = false;
      }
      return succeeded;
    };

    const flushPluginChanges = (): Promise<boolean> => {
      if (pluginSaveRunning) {
        pluginSaveFlushRequested = true;
        return pluginSaveCompletion ?? Promise.resolve(true);
      }
      pluginSaveRunning = true;
      const completion = runPluginFlush();
      pluginSaveCompletion = completion;
      void completion.then(
        () => {
          if (pluginSaveCompletion === completion) pluginSaveCompletion = null;
        },
        () => {
          if (pluginSaveCompletion === completion) pluginSaveCompletion = null;
        },
      );
      return completion;
    };

    const schedulePluginSave = () => {
      if (closeRequested || pluginSaveTimer != null || pluginSaveRunning) return;
      pluginSaveTimer = setTimeout(() => {
        pluginSaveTimer = null;
        void flushPluginChanges().then(() => {
          if (pendingPluginChanges.current.size > 0) schedulePluginSave();
        });
      }, 100);
    };

    const enqueuePluginParameter = (change: {
      trackId: string;
      deviceId: string;
      parameterIndex: number;
      value: number;
    }) => {
      if (closeRequested) return;
      const key = `${change.trackId}\u0000${change.deviceId}`;
      const pending = pendingPluginChanges.current.get(key) ?? {
        trackId: change.trackId,
        deviceId: change.deviceId,
        parameters: new Map<number, number>(),
        state: null,
      };
      pending.parameters.set(change.parameterIndex, change.value);
      pendingPluginChanges.current.set(key, pending);
      schedulePluginSave();
    };

    const unlistenTrackPluginParameter = onTrackPluginParameterChanged(enqueuePluginParameter);
    const unlistenTrackPluginState = onTrackPluginStateChanged((change) => {
      if (closeRequested) return;
      const key = `${change.trackId}\u0000${change.deviceId}`;
      pendingPluginChanges.current.set(key, {
        trackId: change.trackId,
        deviceId: change.deviceId,
        parameters: new Map(),
        state: change,
      });
      void flushPluginChanges();
    });

    let unlistenClose: (() => void) | null = null;
    let closeListenerCancelled = false;
    try {
      const currentWindow = getCurrentWindow();
      void currentWindow
        .onCloseRequested(async (event) => {
          if (closeRequested) {
            event.preventDefault();
            return;
          }
          closeRequested = true;
          event.preventDefault();
          if (pluginSaveTimer != null) {
            clearTimeout(pluginSaveTimer);
            pluginSaveTimer = null;
          }
          let saved = false;
          try {
            saved = await Promise.race([
              flushPluginChanges(),
              new Promise<boolean>((resolve) => window.setTimeout(() => resolve(false), 3000)),
            ]);
          } catch (error) {
            console.error('[native] final Plugin State flush failed', error);
          }
          if (!saved) console.error('[native] final Plugin State flush timed out or failed');
          try {
            await currentWindow.destroy();
          } catch (error) {
            console.error('[native] window close failed', error);
            closeRequested = false;
          }
        })
        .then((unlisten) => {
          if (closeListenerCancelled) unlisten();
          else unlistenClose = unlisten;
        })
        .catch(() => undefined);
    } catch {
      // The browser test/runtime has no Tauri window; native builds register it.
    }

    return () => {
      closeListenerCancelled = true;
      if (pluginSaveTimer != null) clearTimeout(pluginSaveTimer);
      unlistenClose?.();
      if (!closeRequested) void flushPluginChanges();
      unlistenTrackPluginParameter();
      unlistenTrackPluginState();
    };
  }, [
    onTrackPluginParameterChanged,
    onTrackPluginStateChanged,
    persistTrackPluginParameter,
    persistTrackPluginState,
    setAutosaveError,
    setSession,
  ]);
}
