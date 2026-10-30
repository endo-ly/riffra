import type { CSSProperties, ReactNode } from 'react';
import { Icon } from '@/shared/ui/primitives';
import controls from '@/shared/ui/controls.module.css';
import { Modal } from './Modal';
import styles from './Dialog.module.css';

interface DialogProps {
  label: string;
  title: string;
  subtitle: string;
  icon: string;
  /** Tints the identity tile, e.g. with the routed track's color. */
  color?: string;
  /** Keeps the dialog open while an operation must not be abandoned. */
  closeDisabled?: boolean;
  /** Whether pressing outside the dialog closes it; off for dialogs holding unsaved input. */
  closeOnBackdrop?: boolean;
  footer?: ReactNode;
  onClose: () => void;
  children: ReactNode;
}

/** A modal mixing surface: identity header, scrolling sections and an optional action footer. */
export function Dialog({
  label,
  title,
  subtitle,
  icon,
  color,
  closeDisabled = false,
  closeOnBackdrop = false,
  footer,
  onClose,
  children,
}: DialogProps) {
  return (
    <Modal
      label={label}
      closeDisabled={closeDisabled}
      closeOnBackdrop={closeOnBackdrop}
      onClose={onClose}
      className={styles.dialog}
      style={color ? ({ '--dialog-color': color } as CSSProperties) : undefined}
    >
      <header className={styles.header}>
        <span className={styles.tile}>
          <Icon name={icon} />
        </span>
        <span className={styles.identity}>
          <strong>{title}</strong>
          <small>{subtitle}</small>
        </span>
        <button
          autoFocus
          type="button"
          className={controls.iconButton}
          aria-label="Close"
          title="Close"
          disabled={closeDisabled}
          onClick={onClose}
        >
          <Icon name="close" />
        </button>
      </header>
      <div className={styles.body}>{children}</div>
      {footer && <footer className={styles.footer}>{footer}</footer>}
    </Modal>
  );
}

interface DialogSectionProps {
  title: string;
  /** Controls aligned to the right of the section title. */
  action?: ReactNode;
  children: ReactNode;
}

/** One titled group of a mixing dialog. */
export function DialogSection({ title, action, children }: DialogSectionProps) {
  return (
    <section className={styles.section}>
      <header className={styles.sectionHeader}>
        <h3>{title}</h3>
        {action}
      </header>
      {children}
    </section>
  );
}

interface DialogRowProps {
  label: string;
  children: ReactNode;
}

/** A setting on one line, its name in the same column as the other rows of the dialog. */
export function DialogRow({ label, children }: DialogRowProps) {
  return (
    <div className={styles.row}>
      <span>{label}</span>
      {children}
    </div>
  );
}
