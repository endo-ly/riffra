import type { ReactNode } from 'react';
import { selectHostReady } from '@/shared/session/selectors';
import { useSession } from '@/shared/session/session-context';
import styles from './ProjectBoundRegion.module.css';

export function ProjectBoundRegion({ children }: { children: ReactNode }) {
  const connected = useSession(selectHostReady);
  const scope = useSession((state) => state.scope);
  const switching = useSession((state) => state.project.switching);
  return (
    <fieldset
      key={scope.generation}
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
