import { useEffect, useState } from 'react';
import { clearToasts, dismissToast, useToasts, type Toast } from '@/shared/notify';
import { ToneIcon } from '@/shared/ui/message/ToneIcon';
import styles from '@/shared/ui/message/ToastStack.module.css';

const DISPLAY_MS: Record<Toast['tone'], number> = {
  info: 4_000,
  success: 4_000,
  danger: 8_000,
};

export function ToastStack() {
  const toasts = useToasts();
  useEffect(() => () => clearToasts(), []);
  if (!toasts.length) return null;
  return (
    <div className={styles.stack}>
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  );
}

function ToastItem({ toast }: { toast: Toast }) {
  const [hovered, setHovered] = useState(false);
  useEffect(() => {
    if (hovered) return;
    const timer = window.setTimeout(() => dismissToast(toast.id), DISPLAY_MS[toast.tone]);
    return () => window.clearTimeout(timer);
  }, [hovered, toast.id, toast.revision, toast.tone]);
  return (
    <div
      className={styles.toast}
      role={toast.tone === 'danger' ? 'alert' : 'status'}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <ToneIcon tone={toast.tone} />
      <div className={styles.body}>
        <strong className={styles.title}>{toast.title}</strong>
        {toast.message && <span className={styles.message}>{toast.message}</span>}
        {toast.cause && <span className={styles.cause}>{toast.cause}</span>}
      </div>
      <button
        type="button"
        className={styles.dismiss}
        aria-label="Dismiss notification"
        onClick={() => dismissToast(toast.id)}
      >
        ×
      </button>
    </div>
  );
}
