import { createContext, useContext } from 'react';
import type { ReactNode } from 'react';
import type { NativeApi } from '@/native/native-api';
import type { SessionCommit } from './commit';

interface SessionContextValue {
  api: NativeApi;
  hostGeneration: number;
  commit: SessionCommit;
}
const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({
  value,
  children,
}: {
  value: SessionContextValue;
  children: ReactNode;
}) {
  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

export function useSessionContext(): SessionContextValue {
  const value = useContext(SessionContext);
  if (!value) throw new Error('SessionProvider is required');
  return value;
}
