import type { ReactNode } from 'react';
import shellStyles from '../AppShell.module.css';
import styles from './BootScreen.module.css';

interface BootScreenProps {
  selector: ReactNode;
  connected: boolean;
  reason: string | null;
  error: string | null;
  loading: boolean;
  onRetry: () => Promise<void>;
}

export function BootScreen({
  selector,
  connected,
  reason,
  error,
  loading,
  onRetry,
}: BootScreenProps) {
  return (
    <div className={styles.bootScreen}>
      <div className={styles.bootHostSelector}>{selector}</div>
      <span className={shellStyles.logoMark}>R</span>
      <strong>Riffra</strong>
      {connected && error ? (
        <>
          <small>Could not load the connected Host</small>
          <p role="alert">{error}</p>
          <button type="button" disabled={loading} onClick={() => void onRetry()}>
            Retry
          </button>
        </>
      ) : (
        <small>
          {connected
            ? 'Loading the connected Host…'
            : (reason ?? 'Host disconnected — reconnect to continue')}
        </small>
      )}
    </div>
  );
}
