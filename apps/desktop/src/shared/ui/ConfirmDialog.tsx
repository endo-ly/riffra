import { Modal } from './Modal';
import styles from './ConfirmDialog.module.css';

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel = 'Confirm',
  danger = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Modal label={title} closeOnBackdrop onClose={onCancel} className={styles.dialog}>
      <h2>{title}</h2>
      <p>{message}</p>
      <div className={styles.actions}>
        <button autoFocus type="button" onClick={onCancel}>
          Cancel
        </button>
        <button
          type="button"
          className={danger ? styles.danger : styles.confirm}
          onClick={onConfirm}
        >
          {confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
