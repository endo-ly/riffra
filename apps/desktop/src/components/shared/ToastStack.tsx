import { useEffect } from 'react';
import { clearAllToasts, dismiss, useToasts } from '@/lib/toasts';
import styles from './ToastStack.module.css';

export function ToastStack() {
  const toasts = useToasts();
  useEffect(() => () => clearAllToasts(), []);
  if (!toasts.length) return null;
  return (
    <div className={styles.stack} role="status" aria-live="polite">
      {toasts.map((item) => (
        <div
          key={item.id}
          className={`${styles.toast} ${item.kind === 'error' ? styles.error : ''}`}
        >
          <span className={styles.text}>{item.text}</span>
          {item.action && (
            <button type="button" className={styles.action} onClick={item.action.onClick}>
              {item.action.label}
            </button>
          )}
          <button
            type="button"
            className={styles.dismiss}
            aria-label="Dismiss notification"
            onClick={() => dismiss(item.id)}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
