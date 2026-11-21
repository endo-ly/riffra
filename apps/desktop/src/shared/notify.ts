import { useSyncExternalStore } from 'react';
import { errorText } from './errors';
import type { Tone } from '@/shared/ui/message/tone';

type ToastTone = Extract<Tone, 'info' | 'success' | 'danger'>;

export interface Toast {
  id: number;
  tone: ToastTone;
  title: string;
  message?: string;
  cause?: string;
  /** Advances when an identical toast is raised again, restarting its display time. */
  revision: number;
}

interface ToastContent {
  tone: ToastTone;
  title: string;
  message?: string;
  cause?: string;
}

const MAX_VISIBLE = 3;
const listeners = new Set<() => void>();
let toasts: readonly Toast[] = [];
let nextId = 0;

function emit() {
  for (const listener of listeners) listener();
}

function sameContent(toast: Toast, content: ToastContent) {
  return (
    toast.tone === content.tone &&
    toast.title === content.title &&
    toast.message === content.message &&
    toast.cause === content.cause
  );
}

function raise(content: ToastContent) {
  const existing = toasts.find((toast) => sameContent(toast, content));
  toasts = existing
    ? toasts.map((toast) =>
        toast.id === existing.id ? { ...toast, revision: toast.revision + 1 } : toast,
      )
    : [...toasts, { ...content, id: ++nextId, revision: 0 }].slice(-MAX_VISIBLE);
  emit();
}

/** Announces something that happened once: the result of a command or a background task. */
export const notify = {
  info(title: string, options: { message?: string } = {}) {
    raise({ tone: 'info', title, message: options.message });
  },
  success(title: string, options: { message?: string } = {}) {
    raise({ tone: 'success', title, message: options.message });
  },
  error(title: string, options: { message?: string; cause?: unknown } = {}) {
    raise({
      tone: 'danger',
      title,
      message: options.message,
      cause: options.cause === undefined ? undefined : errorText(options.cause),
    });
  },
};

export function dismissToast(id: number) {
  if (!toasts.some((toast) => toast.id === id)) return;
  toasts = toasts.filter((toast) => toast.id !== id);
  emit();
}

export function clearToasts() {
  toasts = [];
  emit();
}

export function useToasts(): readonly Toast[] {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => toasts,
  );
}
