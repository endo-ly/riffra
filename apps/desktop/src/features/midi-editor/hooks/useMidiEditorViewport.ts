import { useCallback, useEffect, useRef, useState, type MutableRefObject } from 'react';
import type { MidiClip } from '@/model/generated';
import { PITCH_HIGH } from '../model/pitch-grid';

const ZOOM_STEP = 1.25;
export const LANE_LABEL_WIDTH = 48;

export function useMidiEditorViewport(
  clip: MidiClip | null,
  playheadTick: number,
  playheadTickRef: MutableRefObject<number>,
  playing: boolean,
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
  const centeredClipIdRef = useRef<string | undefined>(undefined);
  const clipId = clip?.id;
  const hasClip = clip !== null;
  const clipNoteCenter = clip?.notes.length
    ? clip.notes.reduce((total, note) => total + note.note, 0) / clip.notes.length
    : 60;
  const visibleTicks = Math.max(clip?.durationTicks ?? 1920, 1920);
  const clipStartTick = clip?.startTick ?? 0;
  const applyHorizontalZoom = useCallback(
    (next: number, clientX?: number) => {
      const bounded = Math.min(1, Math.max(0.05, next));
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

  const playheadTickInClip = playheadTick - clipStartTick;

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

  // Ctrl+wheel zooms time around the cursor and Alt+wheel zooms pitch; a plain
  // wheel keeps scrolling. The listener is not passive so the WebView's own
  // page zoom never takes Ctrl+wheel.
  useEffect(() => {
    const viewport = laneViewportRef.current;
    if (!viewport) return;
    const onWheel = (event: WheelEvent) => {
      const zoomIn = event.deltaY < 0;
      if (event.ctrlKey || event.metaKey) {
        event.preventDefault();
        applyHorizontalZoom(pixelsPerTick * (zoomIn ? ZOOM_STEP : 1 / ZOOM_STEP), event.clientX);
      } else if (event.altKey) {
        event.preventDefault();
        applyVerticalZoom(rowHeight + (zoomIn ? 2 : -2));
      }
    };
    viewport.addEventListener('wheel', onWheel, { passive: false });
    return () => viewport.removeEventListener('wheel', onWheel);
  }, [applyHorizontalZoom, applyVerticalZoom, hasClip, pixelsPerTick, rowHeight]);

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
  const previousPlayheadTickRef = useRef<number | null>(null);
  useEffect(() => {
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

  useEffect(() => {
    if (!hasClip || clipId === undefined) {
      centeredClipIdRef.current = undefined;
      return;
    }
    if (centeredClipIdRef.current === clipId) return;
    centeredClipIdRef.current = clipId;
    const viewport = laneViewportRef.current;
    if (!viewport) return;
    programmaticScrollRef.current = true;
    viewport.scrollTop = Math.max(
      0,
      (PITCH_HIGH - 1 - clipNoteCenter) * rowHeight - viewport.clientHeight / 2,
    );
  }, [clipId, clipNoteCenter, hasClip, rowHeight]);

  return {
    pixelsPerTick,
    rowHeight,
    visibleTicks,
    laneViewportRef,
    rulerContentRef,
    velocityContentRef,
    rulerPlayheadRef,
    lanePlayheadRef,
    velocityPlayheadRef,
  };
}
