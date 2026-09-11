import { useCallback, useEffect, useRef, useState, type MutableRefObject } from 'react';
import type { ProjectTimebase } from '@/model/domain';
import type { TransportStatus } from '@/native/contracts';
import { TRACK_HEADER_WIDTH, BASE_PIXELS_PER_QUARTER } from '../model/arrange-timeline';

interface UseArrangeViewportOptions {
  timebase: ProjectTimebase;
  transport: TransportStatus | null;
  displayTickRef: MutableRefObject<number>;
}

const MIN_ZOOM = 0.35;
const MAX_ZOOM = 4;
const FOLLOW_LINE_RATIO = 0.32;
const REVEAL_MARGIN = 48;

export function useArrangeViewport({
  timebase,
  transport,
  displayTickRef,
}: UseArrangeViewportOptions) {
  const [zoom, setZoom] = useState(1);
  const [scrollTop, setScrollTop] = useState(0);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const programmaticScrollRef = useRef(false);
  const followPausedRef = useRef(false);
  const previousDiscontinuityRef = useRef<number | null>(null);
  const pixelsPerTick = (BASE_PIXELS_PER_QUARTER * zoom) / timebase.ppq;

  const applyZoom = (next: number, clientX?: number) => {
    const bounded = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, next));
    const scroller = scrollerRef.current;
    if (!scroller) return setZoom(bounded);
    const bounds = scroller.getBoundingClientRect();
    const cursor = (clientX ?? bounds.left + bounds.width / 2) - bounds.left;
    const tick = Math.max(0, (scroller.scrollLeft + cursor - TRACK_HEADER_WIDTH) / pixelsPerTick);
    setZoom(bounded);
    requestAnimationFrame(() => {
      const nextPixels = (BASE_PIXELS_PER_QUARTER * bounded) / timebase.ppq;
      programmaticScrollRef.current = true;
      scroller.scrollLeft = Math.max(0, TRACK_HEADER_WIDTH + tick * nextPixels - cursor);
    });
  };

  const revealTimelineTick = useCallback(
    (tick: number) => {
      const scroller = scrollerRef.current;
      if (!scroller) return;

      const targetX = TRACK_HEADER_WIDTH + Math.max(0, tick) * pixelsPerTick;
      const left = scroller.scrollLeft;
      const right = left + scroller.clientWidth;
      const margin = Math.min(REVEAL_MARGIN, scroller.clientWidth / 2);
      let nextScrollLeft = left;
      if (targetX < left + margin) {
        nextScrollLeft = Math.max(0, targetX - margin);
      } else if (targetX > right - margin) {
        nextScrollLeft = Math.max(0, targetX - scroller.clientWidth + margin);
      }
      if (nextScrollLeft === left) return;

      programmaticScrollRef.current = true;
      followPausedRef.current = false;
      scroller.scrollLeft = nextScrollLeft;
    },
    [pixelsPerTick],
  );

  const zoomToRange = useCallback(
    (startTick: number, endTick: number) => {
      const scroller = scrollerRef.current;
      if (!scroller) return;
      const span = Math.max(1, endTick - startTick);
      const usableWidth = Math.max(1, scroller.clientWidth - TRACK_HEADER_WIDTH - 32);
      const bounded = Math.min(
        MAX_ZOOM,
        Math.max(MIN_ZOOM, (usableWidth / span / BASE_PIXELS_PER_QUARTER) * timebase.ppq),
      );
      setZoom(bounded);
      requestAnimationFrame(() => {
        const nextPixels = (BASE_PIXELS_PER_QUARTER * bounded) / timebase.ppq;
        programmaticScrollRef.current = true;
        scroller.scrollLeft = Math.max(0, TRACK_HEADER_WIDTH + startTick * nextPixels - 16);
      });
    },
    [timebase.ppq],
  );

  // Track vertical scroll so the ruler and ruler corner stay sticky to the top
  // of the scrolling viewport without leaving the timeline's horizontal flow.
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let frame = 0;
    const onScroll = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setScrollTop(scroller.scrollTop));
    };
    scroller.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      scroller.removeEventListener('scroll', onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);

  // Follow the playhead during playback: once the playhead crosses the follow
  // line the view scrolls continuously to keep it there. A manual scroll pauses
  // the follow while the playhead stays in view; when it leaves the viewport the
  // follow resumes automatically.
  useEffect(() => {
    if (transport?.state !== 'playing') return;
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let frame = 0;
    const update = () => {
      const playheadX = TRACK_HEADER_WIDTH + displayTickRef.current * pixelsPerTick;
      const left = scroller.scrollLeft;
      const followOffset = scroller.clientWidth * FOLLOW_LINE_RATIO;
      if (followPausedRef.current) {
        if (playheadX < left || playheadX > left + scroller.clientWidth) {
          followPausedRef.current = false;
        }
      }
      if (!followPausedRef.current && (playheadX < left || playheadX >= left + followOffset)) {
        programmaticScrollRef.current = true;
        scroller.scrollLeft = Math.max(0, playheadX - followOffset);
      }
      frame = requestAnimationFrame(update);
    };
    frame = requestAnimationFrame(update);
    return () => cancelAnimationFrame(frame);
  }, [displayTickRef, pixelsPerTick, transport?.state]);

  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const onScroll = () => {
      if (programmaticScrollRef.current) {
        programmaticScrollRef.current = false;
        return;
      }
      followPausedRef.current = true;
    };
    scroller.addEventListener('scroll', onScroll, { passive: true });
    return () => scroller.removeEventListener('scroll', onScroll);
  }, []);

  useEffect(() => {
    if (!transport) return;
    const previous = previousDiscontinuityRef.current;
    previousDiscontinuityRef.current = transport.discontinuity;
    if (previous === null || previous === transport.discontinuity) return;

    const scroller = scrollerRef.current;
    if (!scroller) return;
    const playheadX = TRACK_HEADER_WIDTH + transport.timelineTick * pixelsPerTick;
    programmaticScrollRef.current = true;
    scroller.scrollLeft = Math.max(0, playheadX - scroller.clientWidth * FOLLOW_LINE_RATIO);
    followPausedRef.current = false;
  }, [pixelsPerTick, transport]);

  return {
    scrollerRef,
    zoom,
    pixelsPerTick,
    applyZoom,
    zoomToRange,
    revealTimelineTick,
    scrollTop,
  };
}
