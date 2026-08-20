import { useCallback, useEffect, useRef } from 'react';
import type { TrackKind } from '@/model/domain';
import {
  buildClipMovesFromDelta,
  trackIdAtPointer,
  type ClipMove,
  type MoveableClip,
  type TrackRowBounds,
} from '@/features/arrange/model/clip-interactions';

interface PointerGestureHandlers {
  onMove: (event: PointerEvent) => void;
  onEnd: (event: PointerEvent) => void;
  onCancel?: () => void;
}

function bindPointerGesture(
  element: HTMLElement,
  handlers: PointerGestureHandlers,
  onDisposed: () => void,
): () => void {
  let active = true;
  const teardown = () => {
    if (!active) return;
    active = false;
    element.removeEventListener('pointermove', handlers.onMove);
    element.removeEventListener('pointerup', finish);
    element.removeEventListener('pointercancel', cancel);
    element.removeEventListener('lostpointercapture', cancel);
  };
  const finish = (event: PointerEvent) => {
    teardown();
    try {
      handlers.onEnd(event);
    } finally {
      onDisposed();
    }
  };
  const cancel = () => {
    teardown();
    try {
      handlers.onCancel?.();
    } finally {
      onDisposed();
    }
  };
  element.addEventListener('pointermove', handlers.onMove);
  element.addEventListener('pointerup', finish);
  element.addEventListener('pointercancel', cancel);
  element.addEventListener('lostpointercapture', cancel);
  return () => {
    teardown();
    try {
      handlers.onCancel?.();
    } finally {
      onDisposed();
    }
  };
}

export function usePointerGesture() {
  const activeGestureRef = useRef<(() => void) | null>(null);
  useEffect(
    () => () => {
      activeGestureRef.current?.();
      activeGestureRef.current = null;
    },
    [],
  );
  return useCallback((element: HTMLElement, handlers: PointerGestureHandlers) => {
    activeGestureRef.current?.();
    let cleanup: (() => void) | null = null;
    const onDisposed = () => {
      if (activeGestureRef.current === cleanup) activeGestureRef.current = null;
    };
    cleanup = bindPointerGesture(element, handlers, onDisposed);
    activeGestureRef.current = cleanup;
  }, []);
}

interface ClipMoveStartEvent {
  clientX: number;
  pointerId: number;
  altKey: boolean;
  currentTarget: HTMLElement;
}

interface ClipMoveGestureOptions<T extends MoveableClip> {
  event: ClipMoveStartEvent;
  clip: T;
  selected: readonly T[];
  kind: TrackKind;
  duplicateAnchor: 'selection' | 'pending';
  pixelsPerTick: number;
  snapTick: (tick: number, temporaryOff?: boolean) => number;
  trackRows: readonly TrackRowBounds[];
  trackIds: readonly string[];
  trackAcceptsKind: (trackId: string, kind: TrackKind) => boolean;
  setSnapGuide: (tick: number | null) => void;
  setMessage: (message: string) => void;
  startGesture: (element: HTMLElement, handlers: PointerGestureHandlers) => void;
  onDuplicate: (anchor: number) => void;
  onMove: (moves: ClipMove[]) => void;
}

export function readTrackRows(fallbackTrackId: string): TrackRowBounds[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[data-arrange-track]')).map((row) => {
    const bounds = row.getBoundingClientRect();
    return {
      trackId: row.dataset.trackId ?? fallbackTrackId,
      top: bounds.top,
      bottom: bounds.bottom,
    };
  });
}

export function restoreClipElementStyle(
  element: HTMLElement,
  left: string,
  width: string,
  setSnapGuide: (tick: number | null) => void,
): void {
  element.style.left = left;
  element.style.width = width;
  setSnapGuide(null);
}

export function restoreFadeHandleStyle(handle: HTMLElement, left: string): void {
  handle.style.left = left;
}

export function bindClipMoveGesture<T extends MoveableClip>(
  options: ClipMoveGestureOptions<T>,
): void {
  const {
    event,
    clip,
    selected,
    kind,
    duplicateAnchor,
    pixelsPerTick,
    snapTick,
    trackRows,
    trackIds,
    trackAcceptsKind,
    setSnapGuide,
    setMessage,
    startGesture,
    onDuplicate,
    onMove,
  } = options;
  const originX = event.clientX;
  const originTick = clip.startTick;
  const element = event.currentTarget;
  const originLeft = element.style.left;
  let pendingTick = originTick;
  let pendingTrack = clip.trackId;
  let duplicate = event.altKey;
  element.setPointerCapture?.(event.pointerId);

  const move = (pointer: PointerEvent) => {
    pendingTick = snapTick(
      originTick + (pointer.clientX - originX) / pixelsPerTick,
      pointer.altKey,
    );
    duplicate = pointer.altKey;
    element.style.left = `${pendingTick * pixelsPerTick}px`;
    setSnapGuide(pendingTick);
    pendingTrack = trackIdAtPointer(trackRows, pointer.clientY, clip.trackId);
  };
  const finish = () => {
    element.style.left = originLeft;
    setSnapGuide(null);
    if (pendingTick === originTick && pendingTrack === clip.trackId) return;
    const deltaTick = pendingTick - originTick;
    if (duplicate) {
      const anchor =
        duplicateAnchor === 'selection'
          ? Math.min(...selected.map((item) => item.startTick)) + deltaTick
          : pendingTick;
      onDuplicate(anchor);
      return;
    }
    const targetMoves = buildClipMovesFromDelta(
      selected,
      clip.trackId,
      pendingTrack,
      deltaTick,
      trackIds,
    );
    if (targetMoves.some((move) => !trackAcceptsKind(move.trackId, kind))) {
      setMessage(
        kind === 'audio'
          ? 'Audio Clips can only be placed on an Audio Track.'
          : 'MIDI Clips can only be placed on an Instrument Track.',
      );
      return;
    }
    onMove(targetMoves);
  };

  startGesture(element, {
    onMove: move,
    onEnd: finish,
    onCancel: () => {
      element.style.left = originLeft;
      setSnapGuide(null);
    },
  });
}
