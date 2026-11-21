import { useEffect, useState } from 'react';
import { useWorkspaceWait } from '@/app/status/hooks/useWorkspaceWait';
import { WaitCard } from '@/shared/ui/message/WaitCard';
import styles from '@/app/status/WorkspaceWait.module.css';

/** A wait shorter than this pauses input without flashing the card. */
const REVEAL_DELAY_MS = 500;

/** Covers the work area while editing waits, explaining why and how far it has come. */
export function WorkspaceWait() {
  const wait = useWorkspaceWait();
  const waiting = wait !== null;
  const [revealed, setRevealed] = useState(false);
  useEffect(() => {
    if (!waiting) {
      setRevealed(false);
      return;
    }
    const timer = window.setTimeout(() => setRevealed(true), REVEAL_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [waiting]);
  if (!wait || !revealed) return null;
  return (
    <div className={styles.workspaceWait}>
      <WaitCard {...wait} />
    </div>
  );
}
