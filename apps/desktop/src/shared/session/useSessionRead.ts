import { useEffect, useState } from 'react';
import { HostConnectionChangedError } from '@/native/invoke';
import { errorText } from '@/shared/errors';
import { useSession } from './session-context';
import type { SessionScope } from './store';

interface SessionRead<T> {
  key: string;
  inputs: readonly unknown[];
  load: () => Promise<T>;
}

interface PendingRead {
  inputs: readonly unknown[];
  promise: Promise<unknown>;
}

const pendingReads = new WeakMap<SessionScope, Map<string, PendingRead>>();

function read<T>(scope: SessionScope, request: SessionRead<T>): Promise<T> {
  let pending = pendingReads.get(scope);
  if (!pending) {
    pending = new Map();
    pendingReads.set(scope, pending);
  }
  const current = pending.get(request.key);
  if (
    current &&
    current.inputs.length === request.inputs.length &&
    current.inputs.every((input, index) => Object.is(input, request.inputs[index]))
  )
    return current.promise as Promise<T>;
  const entry: PendingRead = {
    inputs: request.inputs,
    promise: Promise.resolve().then(request.load),
  };
  pending.set(request.key, entry);
  const remove = () => {
    if (pending.get(request.key) === entry) pending.delete(request.key);
  };
  void entry.promise.then(remove, remove);
  return entry.promise as Promise<T>;
}

/** Shares in-flight reads within one Host/Project/session, without retaining native data. */
export function useSessionRead<T>(
  request: SessionRead<T> | null,
  options: {
    keepPrevious?: boolean;
    enabled?: boolean;
  } = {},
) {
  const { keepPrevious = false, enabled = true } = options;
  const scope = useSession((state) => state.scope);
  const [result, setResult] = useState<{
    request: SessionRead<T>;
    scope: typeof scope;
    data: T | null;
    error: string | null;
  } | null>(null);
  useEffect(() => {
    if (!request || !enabled) return;
    let active = true;
    void read(scope, request).then(
      (data) => {
        if (active) setResult({ request, scope, data, error: null });
      },
      (cause: unknown) => {
        if (active)
          setResult({
            request,
            scope,
            data: null,
            error: cause instanceof HostConnectionChangedError ? null : errorText(cause),
          });
      },
    );
    return () => {
      active = false;
    };
  }, [scope, request, enabled]);
  const current = result?.request === request && result.scope === scope ? result : null;
  const previous =
    keepPrevious && request?.key === result?.request.key && result?.scope === scope ? result : null;
  return {
    data: current?.data ?? previous?.data ?? null,
    error: current?.error ?? null,
    loading: request !== null && (!enabled || current === null),
  };
}
