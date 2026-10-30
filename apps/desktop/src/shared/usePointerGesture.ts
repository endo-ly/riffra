import { useCallback, useEffect, useMemo, useRef } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';

interface PointerGestureHandlers {
  onMove: (event: PointerEvent) => void;
  onEnd: (event: PointerEvent) => void;
  onCancel?: () => void;
}

interface PointerGestureOptions {
  /** Cancels the gesture on Escape, e.g. to restore a dragged automation point. */
  cancelOnEscape?: boolean;
}

interface PointerGestureStarter {
  (
    event: ReactPointerEvent<HTMLElement>,
    handlers: PointerGestureHandlers,
    options?: PointerGestureOptions,
  ): void;
  cancel(): void;
}

/** Runs one pointer drag at a time and cancels it when a new drag starts or the owner unmounts. */
export function usePointerGesture(): PointerGestureStarter {
  const activeGesture = useRef<(() => void) | null>(null);

  const cancelActiveGesture = useCallback(() => {
    const cancel = activeGesture.current;
    activeGesture.current = null;
    cancel?.();
  }, []);

  useEffect(() => cancelActiveGesture, [cancelActiveGesture]);

  const startGesture = useCallback(
    (
      event: ReactPointerEvent<HTMLElement>,
      handlers: PointerGestureHandlers,
      options: PointerGestureOptions = {},
    ) => {
      cancelActiveGesture();
      const element = event.currentTarget;
      const pointerId = event.pointerId;
      element.setPointerCapture?.(pointerId);
      let active = true;
      let cancelGesture = () => undefined;

      const cleanup = () => {
        if (!active) return false;
        active = false;
        window.removeEventListener('pointermove', onMove);
        window.removeEventListener('pointerup', onEnd);
        window.removeEventListener('pointercancel', cancelGestureForPointer);
        element.removeEventListener('lostpointercapture', cancelGestureForPointer);
        if (options.cancelOnEscape) window.removeEventListener('keydown', onKeyDown, true);
        if (element.hasPointerCapture?.(pointerId)) element.releasePointerCapture?.(pointerId);
        if (activeGesture.current === cancelGesture) activeGesture.current = null;
        return true;
      };

      const onMove = (pointer: PointerEvent) => {
        if (pointer.pointerId === pointerId) handlers.onMove(pointer);
      };
      const onEnd = (pointer: PointerEvent) => {
        if (pointer.pointerId !== pointerId || !cleanup()) return;
        handlers.onEnd(pointer);
      };
      const cancelGestureForPointer = (pointer: PointerEvent) => {
        if (pointer.pointerId !== pointerId || !cleanup()) return;
        handlers.onCancel?.();
      };
      const onKeyDown = (keyEvent: KeyboardEvent) => {
        if (keyEvent.key !== 'Escape') return;
        keyEvent.preventDefault();
        cancelGesture();
      };
      cancelGesture = () => {
        if (!cleanup()) return;
        handlers.onCancel?.();
      };

      activeGesture.current = cancelGesture;
      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onEnd);
      window.addEventListener('pointercancel', cancelGestureForPointer);
      element.addEventListener('lostpointercapture', cancelGestureForPointer);
      if (options.cancelOnEscape) window.addEventListener('keydown', onKeyDown, true);
    },
    [cancelActiveGesture],
  );

  return useMemo(
    () => Object.assign(startGesture, { cancel: cancelActiveGesture }),
    [cancelActiveGesture, startGesture],
  );
}
