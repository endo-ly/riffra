import { useEffect, useRef, type MutableRefObject } from 'react';
import { TRACK_HEADER_WIDTH } from '../model/arrange-timeline';
import styles from '../WorkspaceArrange.module.css';

interface ArrangePlayheadProps {
  positionRef: MutableRefObject<number>;
  pixelsPerTick: number;
  playing: boolean;
}

export function ArrangePlayhead({ positionRef, pixelsPerTick, playing }: ArrangePlayheadProps) {
  const elementRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const update = () => {
      const element = elementRef.current;
      if (element) {
        element.style.transform = `translate3d(${
          TRACK_HEADER_WIDTH + positionRef.current * pixelsPerTick
        }px, 0, 0)`;
      }
    };
    if (!playing) {
      update();
      return;
    }
    let frame = requestAnimationFrame(function animate() {
      update();
      frame = requestAnimationFrame(animate);
    });
    return () => cancelAnimationFrame(frame);
  }, [pixelsPerTick, playing, positionRef]);

  return (
    <div ref={elementRef} className={styles.playhead}>
      <span />
    </div>
  );
}
