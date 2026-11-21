import { useState } from 'react';
import type { NativeApi } from '@/native/native-api';
import { defaultNativeApi } from '@/native/api';
import { SessionProvider } from '@/session/session-context';
import { createSessionStore } from '@/session/store';
import { AppShell } from '@/app/AppShell';

export default function App({ api = defaultNativeApi }: { api?: NativeApi } = {}) {
  const [store] = useState(() => createSessionStore(api));
  return (
    <SessionProvider api={api} store={store}>
      <AppShell />
    </SessionProvider>
  );
}
