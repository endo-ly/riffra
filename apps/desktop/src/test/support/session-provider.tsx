import { useCallback, useMemo, useRef } from 'react';
import type { ReactNode } from 'react';
import type { CanonicalState } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { createSessionCommit } from '@/shared/session/commit';
import { SessionProvider } from '@/shared/session/session-context';

const ignoreCanonical = () => undefined;
export function TestSessionProvider({
  children,
  api,
  applyCanonicalState = ignoreCanonical,
  hostGeneration = 0,
  getCanonicalState,
}: {
  children: ReactNode;
  api: NativeApi;
  applyCanonicalState?: (canonical: CanonicalState) => void;
  hostGeneration?: number;
  getCanonicalState?: () => CanonicalState | null;
}) {
  const canonicalRef = useRef<CanonicalState | null>(null);
  const apply = useCallback(
    (canonical: CanonicalState) => {
      canonicalRef.current = canonical;
      applyCanonicalState(canonical);
    },
    [applyCanonicalState],
  );
  const readCanonical = useCallback(
    () => (getCanonicalState ? getCanonicalState() : canonicalRef.current),
    [getCanonicalState],
  );
  const commit = useMemo(() => createSessionCommit(apply), [apply]);
  const value = useMemo(
    () => ({ api, hostGeneration, commit, getCanonicalState: readCanonical }),
    [api, hostGeneration, commit, readCanonical],
  );
  return <SessionProvider value={value}>{children}</SessionProvider>;
}
