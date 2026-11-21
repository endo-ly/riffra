import type { ReactNode } from 'react';
import { selectHostReady } from '@/session/selectors';
import { useSession } from '@/session/session-context';
import styles from '@/app/layout/ProjectBoundRegion.module.css';

/** Rebuilds its content for each Project and disables its controls while the Host is away. */
export function ProjectBoundRegion({ children }: { children: ReactNode }) {
  const connected = useSession(selectHostReady);
  const scope = useSession((state) => state.scope);
  return (
    <fieldset key={scope.generation} className={styles.projectBoundRegion} disabled={!connected}>
      {children}
    </fieldset>
  );
}
