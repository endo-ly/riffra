import type { PointerEvent as ReactPointerEvent } from 'react';
import type { usePointerGesture } from '@/shared/usePointerGesture';
import type { TrackKind } from '@/model/generated';
import {
  buildClipMovesFromDelta,
  trackIdAtPointer,
  type ClipMove,
  type MoveableClip,
  type TrackRowBounds,
} from '@/features/timeline/model/clip-interactions';

interface ClipMoveGestureOptions<T extends MoveableClip> {
  event: ReactPointerEvent<HTMLElement>;
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
  startGesture: ReturnType<typeof usePointerGesture>;
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
  const originY = event.clientY;
  const originTick = clip.startTick;
  const element = event.currentTarget;
  const originLeft = element.style.left;
  let pendingTick = originTick;
  let pendingTrack = clip.trackId;
  let duplicate = event.altKey;
  let horizontalIntent = false;
  let verticalIntent = false;
  let horizontalOriginX = originX;
  let previousX = originX;
  let previousY = originY;

  const move = (pointer: PointerEvent) => {
    const horizontalDelta = pointer.clientX - originX;
    const verticalDelta = pointer.clientY - originY;
    const horizontalStep = pointer.clientX - previousX;
    const verticalStep = pointer.clientY - previousY;
    if (!horizontalIntent) {
      if (verticalIntent) {
        if (Math.abs(verticalStep) >= Math.abs(horizontalStep)) {
          horizontalOriginX = pointer.clientX;
        } else if (Math.abs(pointer.clientX - horizontalOriginX) >= 4) {
          horizontalIntent = true;
        }
      } else if (
        Math.abs(horizontalDelta) >= 4 &&
        Math.abs(horizontalDelta) >= Math.abs(verticalDelta) / 2
      ) {
        horizontalIntent = true;
      } else if (
        Math.abs(verticalDelta) >= 4 &&
        Math.abs(verticalDelta) >= Math.abs(horizontalDelta) * 2
      ) {
        verticalIntent = true;
        horizontalOriginX = pointer.clientX;
      }
    }
    pendingTick = horizontalIntent
      ? snapTick(
          originTick +
            (verticalIntent ? pointer.clientX - horizontalOriginX : horizontalDelta) /
              pixelsPerTick,
          pointer.altKey,
        )
      : originTick;
    duplicate = pointer.altKey;
    element.style.left = `${pendingTick * pixelsPerTick}px`;
    setSnapGuide(horizontalIntent ? pendingTick : null);
    pendingTrack = trackIdAtPointer(trackRows, pointer.clientY, clip.trackId);
    previousX = pointer.clientX;
    previousY = pointer.clientY;
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

  startGesture(event, {
    onMove: move,
    onEnd: finish,
    onCancel: () => {
      element.style.left = originLeft;
      setSnapGuide(null);
    },
  });
}
