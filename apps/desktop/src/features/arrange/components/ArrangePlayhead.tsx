import { useCallback, useEffect, useRef, type MutableRefObject } from 'react';
import { TRACK_HEADER_WIDTH } from '../model/arrange-timeline';
import styles from '../WorkspaceArrange.module.css';

interface ArrangePlayheadProps {
  positionRef: MutableRefObject<number>;
  positionTick: number;
  pixelsPerTick: number;
  playing: boolean;
}

export function ArrangePlayhead({
  positionRef,
  positionTick,
  pixelsPerTick,
  playing,
}: ArrangePlayheadProps) {
  const elementRef = useRef<HTMLDivElement>(null);

  const updatePosition = useCallback(() => {
    const element = elementRef.current;
    if (element) {
      element.style.transform = `translate3d(${
        TRACK_HEADER_WIDTH + positionRef.current * pixelsPerTick
      }px, 0, 0)`;
    }
  }, [pixelsPerTick, positionRef]);

  useEffect(() => {
    if (playing) return;
    updatePosition();
  }, [playing, positionTick, updatePosition]);

  useEffect(() => {
    if (!playing) return;
    let frame = requestAnimationFrame(function animate() {
      updatePosition();
      frame = requestAnimationFrame(animate);
    });
    return () => cancelAnimationFrame(frame);
  }, [playing, updatePosition]);

  return (
    <div ref={elementRef} className={styles.playhead}>
      <span />
    </div>
  );
}
