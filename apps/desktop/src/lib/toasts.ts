import { useSyncExternalStore } from 'react';

export type ToastKind = 'info' | 'error';

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface Toast {
  id: number;
  text: string;
  kind: ToastKind;
  action?: ToastAction;
  persistent: boolean;
}

const AUTO_DISMISS_MS = 5_000;
const listeners = new Set<() => void>();
const autoDismissTimers = new Map<number, number>();

interface StoredToast extends Toast {
  source?: string;
}

let toasts: StoredToast[] = [];
let nextId = 0;

function emit() {
  for (const listener of listeners) listener();
}

interface ToastOptions {
  kind?: ToastKind;
  action?: ToastAction;
  persistent?: boolean;
}

export function toast(text: string, options: ToastOptions = {}): number {
  const id = ++nextId;
  const item: Toast = {
    id,
    text,
    kind: options.kind ?? 'info',
    action: options.action,
    persistent: options.persistent ?? false,
  };
  toasts = [...toasts, item];
  emit();
  scheduleAutoDismiss(item);
  return id;
}

export function showToast(
  source: string,
  text: string | null,
  options: ToastOptions = {},
): number | null {
  const existing = toasts.find((item) => item.source === source);
  if (text === null) {
    if (existing) dismiss(existing.id);
    return null;
  }

  const item: StoredToast = {
    id: existing?.id ?? ++nextId,
    source,
    text,
    kind: options.kind ?? 'info',
    action: options.action,
    persistent: options.persistent ?? false,
  };
  toasts = existing
    ? toasts.map((current) => (current.id === existing.id ? item : current))
    : [...toasts, item];
  emit();
  scheduleAutoDismiss(item);
  return item.id;
}

export function clearToast(source: string) {
  const existing = toasts.find((item) => item.source === source);
  if (existing) dismiss(existing.id);
}

export function dismiss(id: number) {
  if (!toasts.some((item) => item.id === id)) return;
  const timer = autoDismissTimers.get(id);
  if (timer !== undefined) {
    window.clearTimeout(timer);
    autoDismissTimers.delete(id);
  }
  toasts = toasts.filter((item) => item.id !== id);
  emit();
}

export function clearAllToasts() {
  for (const timer of autoDismissTimers.values()) window.clearTimeout(timer);
  autoDismissTimers.clear();
  toasts = [];
  emit();
}

function scheduleAutoDismiss(item: Toast) {
  const previousTimer = autoDismissTimers.get(item.id);
  if (previousTimer !== undefined) window.clearTimeout(previousTimer);
  autoDismissTimers.delete(item.id);
  if (item.persistent) return;
  const timer = window.setTimeout(() => dismiss(item.id), AUTO_DISMISS_MS);
  autoDismissTimers.set(item.id, timer);
}

export function useToasts(): Toast[] {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => toasts,
  );
}
