import type { ReactNode } from 'react';
import { useSession } from '@/shared/session/session-context';
import styles from './ProjectBoundRegion.module.css';

export function ProjectBoundRegion({ children }: { children: ReactNode }) {
  const host = useSession((state) => state.host);
  const switching = useSession((state) => state.project.switching);
  const connected = host.connection.mode !== 'disconnected' && !host.switching;
  return (
    <fieldset
      className={styles.projectBoundRegion}
      data-project-switching={switching ? 'true' : undefined}
      aria-busy={switching}
      inert={switching}
      disabled={!connected || switching}
    >
      {children}
    </fieldset>
  );
}
