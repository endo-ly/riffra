import type { CSSProperties, MouseEvent, ReactNode } from 'react';
import { useEffect, useRef } from 'react';
import { useState } from 'react';
import { createPortal } from 'react-dom';
import styles from './Modal.module.css';

interface ModalEntry {
  close: () => void;
  closeDisabled: boolean;
}

const openModals: ModalEntry[] = [];

function handleWindowKeyDown(event: KeyboardEvent) {
  if (event.key !== 'Escape' || event.defaultPrevented) return;
  const topModal = openModals.at(-1);
  if (!topModal || topModal.closeDisabled) return;
  event.preventDefault();
  topModal.close();
}

interface ModalProps {
  /** Accessible name of the modal surface. */
  label: string;
  /** Vertical placement of the surface: centered, or below the top edge like a command palette. */
  placement?: 'center' | 'top';
  /** Keeps the modal open while an operation must not be abandoned. */
  closeDisabled?: boolean;
  /** Whether pressing outside the surface closes it; off for modals holding unsaved input. */
  closeOnBackdrop?: boolean;
  onClose: () => void;
  /** Class of the surface; the surface owns its own size and layout. */
  className: string;
  style?: CSSProperties;
  children: ReactNode;
}

export function Modal({
  label,
  placement = 'center',
  closeDisabled = false,
  closeOnBackdrop = false,
  onClose,
  className,
  style,
  children,
}: ModalProps) {
  const entry = useRef<ModalEntry>({ close: onClose, closeDisabled });
  const [returnFocus] = useState(() =>
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  entry.current.close = onClose;
  entry.current.closeDisabled = closeDisabled;

  useEffect(() => {
    const modalEntry = entry.current;
    const focusTarget = returnFocus;
    openModals.push(modalEntry);
    if (openModals.length === 1) window.addEventListener('keydown', handleWindowKeyDown);
    return () => {
      const index = openModals.indexOf(modalEntry);
      if (index !== -1) openModals.splice(index, 1);
      if (openModals.length === 0) window.removeEventListener('keydown', handleWindowKeyDown);
      focusTarget?.focus();
    };
  }, [returnFocus]);

  const handleBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget && closeOnBackdrop && !closeDisabled) onClose();
  };

  return createPortal(
    <div
      className={styles.backdrop}
      data-placement={placement}
      role="presentation"
      onMouseDown={handleBackdropMouseDown}
    >
      <section
        className={className}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        style={style}
      >
        {children}
      </section>
    </div>,
    document.body,
  );
}
