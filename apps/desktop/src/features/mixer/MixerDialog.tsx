import type { CSSProperties, ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Icon } from '@/shared/ui/primitives';
import controls from '@/shared/ui/controls.module.css';
import styles from './MixerDialog.module.css';

interface MixerDialogProps {
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
export function MixerDialog({
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
}: MixerDialogProps) {
  // Portaled so the dialog escapes the isolated stacking context of the panel that opens it.
  return createPortal(
    <div
      className={styles.overlay}
      role="presentation"
      onMouseDown={closeOnBackdrop && !closeDisabled ? onClose : undefined}
    >
      <section
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        style={color ? ({ '--dialog-color': color } as CSSProperties) : undefined}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === 'Escape' && !closeDisabled) onClose();
        }}
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
      </section>
    </div>,
    document.body,
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
