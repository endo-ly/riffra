import { useCallback, useEffect, useRef, useState, type MutableRefObject } from 'react';
import type { MidiClip } from '@/model/generated';
import { LANE_LABEL_WIDTH, PITCH_HIGH } from '@/features/midi-editor/model/pitch-grid';
import { scrollWheelVertically } from '@/shared/input';

const ZOOM_STEP = 1.25;
// The minimum fits a few minutes of clip on one screen.
const MIN_PIXELS_PER_TICK = 0.0025;
const MAX_PIXELS_PER_TICK = 1;
const FIT_MARGIN = 16;
/** Longest span, in bars, an opened clip is fitted to; longer clips open at their start. */
const FIT_MAX_BARS = 4;
const WHEEL_ROWS_PER_NOTCH = 2;

function boundPixelsPerTick(value: number) {
  return Math.min(MAX_PIXELS_PER_TICK, Math.max(MIN_PIXELS_PER_TICK, value));
}

export function useMidiEditorViewport(
  clip: MidiClip | null,
  playheadTick: number | null,
  playheadTickRef: MutableRefObject<number>,
  playing: boolean,
  barTicks: number,
) {
  const [pixelsPerTick, setPixelsPerTick] = useState(0.18);
  const [rowHeight, setRowHeight] = useState(12);
  const laneViewportRef = useRef<HTMLDivElement>(null);
  const rulerContentRef = useRef<HTMLDivElement>(null);
  const velocityContentRef = useRef<HTMLDivElement>(null);
  const rulerPlayheadRef = useRef<HTMLElement>(null);
  const lanePlayheadRef = useRef<HTMLElement>(null);
  const velocityPlayheadRef = useRef<HTMLElement>(null);
  const programmaticScrollRef = useRef(false);
  const followPausedRef = useRef(false);
  const framedClipIdRef = useRef<string | undefined>(undefined);
  const previousPlayheadTickRef = useRef<number | null>(null);
  const clipId = clip?.id;
  const hasClip = clip !== null;
  const clipNoteCenter = clip?.notes.length
    ? clip.notes.reduce((total, note) => total + note.note, 0) / clip.notes.length
    : 60;
  const visibleTicks = Math.max(clip?.durationTicks ?? 1920, 1920);
  const clipStartTick = clip?.startTick ?? 0;
  const applyHorizontalZoom = useCallback(
    (next: number, clientX?: number) => {
      const bounded = boundPixelsPerTick(next);
      const viewport = laneViewportRef.current;
      if (!viewport) {
        setPixelsPerTick(bounded);
        return;
      }
      const bounds = viewport.getBoundingClientRect();
      const cursor =
        clientX === undefined
          ? viewport.clientWidth / 2
          : Math.max(0, Math.min(viewport.clientWidth, clientX - bounds.left));
      const tick = Math.max(0, (viewport.scrollLeft + cursor - LANE_LABEL_WIDTH) / pixelsPerTick);
      setPixelsPerTick(bounded);
      requestAnimationFrame(() => {
        programmaticScrollRef.current = true;
        viewport.scrollLeft = Math.max(0, LANE_LABEL_WIDTH + tick * bounded - cursor);
      });
    },
    [pixelsPerTick],
  );

  const applyVerticalZoom = useCallback(
    (next: number) => {
      const bounded = Math.min(24, Math.max(8, next));
      const viewport = laneViewportRef.current;
      if (!viewport) {
        setRowHeight(bounded);
        return;
      }
      const centerPitch =
        PITCH_HIGH - 1 - (viewport.scrollTop + viewport.clientHeight / 2) / rowHeight;
      setRowHeight(bounded);
      requestAnimationFrame(() => {
        programmaticScrollRef.current = true;
        viewport.scrollTop = Math.max(
          0,
          (PITCH_HIGH - 1 - centerPitch) * bounded - viewport.clientHeight / 2,
        );
      });
    },
    [rowHeight],
  );

  const stepHorizontalZoom = useCallback(
    (direction: -1 | 1, clientX?: number) =>
      applyHorizontalZoom(pixelsPerTick * (direction > 0 ? ZOOM_STEP : 1 / ZOOM_STEP), clientX),
    [applyHorizontalZoom, pixelsPerTick],
  );
  const stepVerticalZoom = useCallback(
    (direction: -1 | 1) => applyVerticalZoom(rowHeight + direction * 2),
    [applyVerticalZoom, rowHeight],
  );

  const playheadTickInClip = playheadTick === null ? null : playheadTick - clipStartTick;

  // The ruler and velocity lane sit outside the scrolling viewport, so their
  // content follows the lane scroll through transform updates.
  const syncScrollContent = useCallback(() => {
    const scrollLeft = laneViewportRef.current?.scrollLeft ?? 0;
    if (rulerContentRef.current) {
      rulerContentRef.current.style.transform = `translate3d(${-scrollLeft}px, 0, 0)`;
    }
    if (velocityContentRef.current) {
      velocityContentRef.current.style.transform = `translate3d(${-scrollLeft}px, 0, 0)`;
    }
  }, []);

  const updatePlayhead = useCallback(() => {
    const tick = playheadTickRef.current - clipStartTick;
    const visible = tick >= 0 && tick <= visibleTicks;
    const transform = `translate3d(${tick * pixelsPerTick}px, 0, 0)`;
    for (const element of [
      rulerPlayheadRef.current,
      lanePlayheadRef.current,
      velocityPlayheadRef.current,
    ]) {
      if (!element) continue;
      element.style.display = visible ? '' : 'none';
      element.style.transform = transform;
    }
  }, [clipStartTick, pixelsPerTick, playheadTickRef, visibleTicks]);

  useEffect(() => {
    const viewport = laneViewportRef.current;
    if (!viewport) return;
    let frame = 0;
    const onScroll = () => {
      if (programmaticScrollRef.current) {
        programmaticScrollRef.current = false;
      } else {
        followPausedRef.current = true;
      }
      if (frame) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        frame = 0;
        syncScrollContent();
      });
    };
    viewport.addEventListener('scroll', onScroll, { passive: true });
    syncScrollContent();
    return () => {
      viewport.removeEventListener('scroll', onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [hasClip, syncScrollContent]);

  // Ctrl+wheel zooms time around the cursor, Alt+wheel zooms pitch, and a plain
  // vertical wheel scrolls a couple of pitch rows per notch. The listener is not
  // passive so the WebView's own page zoom never takes Ctrl+wheel.
  useEffect(() => {
    const viewport = laneViewportRef.current;
    if (!viewport) return;
    const onWheel = (event: WheelEvent) => {
      const direction = event.deltaY < 0 ? 1 : -1;
      if (event.ctrlKey || event.metaKey) {
        event.preventDefault();
        stepHorizontalZoom(direction, event.clientX);
      } else if (event.altKey) {
        event.preventDefault();
        stepVerticalZoom(direction);
      } else {
        scrollWheelVertically(viewport, event, WHEEL_ROWS_PER_NOTCH * rowHeight);
      }
    };
    viewport.addEventListener('wheel', onWheel, { passive: false });
    return () => viewport.removeEventListener('wheel', onWheel);
  }, [hasClip, rowHeight, stepHorizontalZoom, stepVerticalZoom]);

  // Follow the playhead during playback: once the playhead crosses the follow
  // line the view scrolls continuously to keep it there. A manual scroll pauses
  // the follow while the playhead stays in view; when it leaves the viewport the
  // follow resumes automatically.
  useEffect(() => {
    if (!playing || !hasClip) return;
    const viewport = laneViewportRef.current;
    if (!viewport) return;
    let frame = requestAnimationFrame(function follow() {
      updatePlayhead();
      const tick = playheadTickRef.current - clipStartTick;
      previousPlayheadTickRef.current = tick;
      const headX = LANE_LABEL_WIDTH + tick * pixelsPerTick;
      const left = viewport.scrollLeft;
      const followOffset = viewport.clientWidth * 0.32;
      if (followPausedRef.current) {
        if (headX < left || headX > left + viewport.clientWidth) {
          followPausedRef.current = false;
        }
      }
      const inClip = tick >= 0 && tick <= visibleTicks;
      if (inClip && !followPausedRef.current && (headX < left || headX >= left + followOffset)) {
        programmaticScrollRef.current = true;
        viewport.scrollLeft = Math.max(0, headX - followOffset);
      }
      syncScrollContent();
      frame = requestAnimationFrame(follow);
    });
    return () => cancelAnimationFrame(frame);
  }, [
    clipStartTick,
    hasClip,
    pixelsPerTick,
    playheadTickRef,
    playing,
    syncScrollContent,
    updatePlayhead,
    visibleTicks,
  ]);

  // Outside playback the position advances only through seeks, so re-sync the
  // playhead elements whenever a React render publishes a new position.
  useEffect(() => {
    updatePlayhead();
  }, [playheadTickInClip, updatePlayhead]);

  // On a seek, bring the playhead back into view the same way the arrange
  // timeline does. The ref keeps tracking during playback so the residual
  // position update right after a stop is not mistaken for a seek.
  useEffect(() => {
    if (playheadTickInClip === null) return;
    const previous = previousPlayheadTickRef.current;
    previousPlayheadTickRef.current = playheadTickInClip;
    const viewport = laneViewportRef.current;
    if (playing || !viewport) return;
    if (previous === null || previous === playheadTickInClip) return;
    if (playheadTickInClip < 0 || playheadTickInClip > visibleTicks) return;
    const headX = LANE_LABEL_WIDTH + playheadTickInClip * pixelsPerTick;
    const left = viewport.scrollLeft;
    if (headX < left || headX > left + viewport.clientWidth) {
      programmaticScrollRef.current = true;
      followPausedRef.current = false;
      viewport.scrollLeft = Math.max(0, headX - viewport.clientWidth * 0.32);
    }
  }, [clipStartTick, pixelsPerTick, playheadTickInClip, playing, visibleTicks]);

  // Frame a newly opened clip: fit its length, up to a few bars, to the
  // measured lane width and center the pitch view on its notes.
  useEffect(() => {
    if (!hasClip || clipId === undefined) {
      framedClipIdRef.current = undefined;
      return;
    }
    if (framedClipIdRef.current === clipId) return;
    framedClipIdRef.current = clipId;
    const viewport = laneViewportRef.current;
    if (!viewport) return;
    const usableWidth = viewport.clientWidth - LANE_LABEL_WIDTH - FIT_MARGIN;
    programmaticScrollRef.current = true;
    if (usableWidth > 0) {
      const fitTicks = Math.min(visibleTicks, barTicks * FIT_MAX_BARS);
      setPixelsPerTick(boundPixelsPerTick(usableWidth / fitTicks));
      viewport.scrollLeft = 0;
    }
    viewport.scrollTop = Math.max(
      0,
      (PITCH_HIGH - 1 - clipNoteCenter) * rowHeight - viewport.clientHeight / 2,
    );
  }, [barTicks, clipId, clipNoteCenter, hasClip, rowHeight, visibleTicks]);

  return {
    pixelsPerTick,
    rowHeight,
    visibleTicks,
    stepHorizontalZoom,
    stepVerticalZoom,
    laneViewportRef,
    rulerContentRef,
    velocityContentRef,
    rulerPlayheadRef,
    lanePlayheadRef,
    velocityPlayheadRef,
  };
}
