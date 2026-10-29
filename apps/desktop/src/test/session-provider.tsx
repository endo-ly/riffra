import { useMemo } from 'react';
import type { ReactNode } from 'react';
import type { CanonicalState } from '@/model/domain';
import type { NativeApi } from '@/native/native-api';
import { createSessionCommit } from '@/shared/session/commit';
import { SessionProvider } from '@/shared/session/session-context';

const ignoreCanonical = () => undefined;
export function TestSessionProvider({
  children,
  api,
  applyCanonicalState = ignoreCanonical,
  hostGeneration = 0,
}: {
  children: ReactNode;
  api: NativeApi;
  applyCanonicalState?: (canonical: CanonicalState) => void;
  hostGeneration?: number;
}) {
  const commit = useMemo(() => createSessionCommit(applyCanonicalState), [applyCanonicalState]);
  const value = useMemo(() => ({ api, hostGeneration, commit }), [api, hostGeneration, commit]);
  return <SessionProvider value={value}>{children}</SessionProvider>;
}
