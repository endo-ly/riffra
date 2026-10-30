import { useCallback, useEffect, useRef } from 'react';
import { useSession } from '@/shared/session/session-context';
import { TRACK_HEADER_WIDTH } from './model/timeline-layout';
import styles from './TimelineView.module.css';

export function TimelinePlayhead({ pixelsPerTick }: { pixelsPerTick: number }) {
  const positionRef = useSession((state) => state.transport.playhead);
  const positionTick = useSession((state) => state.transport.displayTick);
  const playing = useSession((state) => state.transport.status?.state === 'playing');
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
