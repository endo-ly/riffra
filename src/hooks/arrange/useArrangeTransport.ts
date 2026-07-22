import { useEffect, useRef, useState } from 'react';
import type { ProjectTimebase, TransportStatus } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

export function useArrangeTransport(api: NativeApi, timebase: ProjectTimebase) {
  const [transport, setTransport] = useState<TransportStatus | null>(null);
  const [displayTick, setDisplayTick] = useState(0);
  const anchor = useRef({ tick: 0, at: performance.now(), playing: false });

  useEffect(
    () =>
      api.onTransportStatus((status) => {
        setTransport(status);
        anchor.current = {
          tick: status.timelineTick,
          at: performance.now(),
          playing: status.state === 'playing',
        };
        setDisplayTick(status.timelineTick);
      }),
    [api],
  );

  useEffect(() => {
    let frame = 0;
    const update = () => {
      const current = anchor.current;
      const elapsed = current.playing ? performance.now() - current.at : 0;
      setDisplayTick(current.tick + (elapsed * timebase.bpm * timebase.ppq) / 60_000);
      frame = requestAnimationFrame(update);
    };
    frame = requestAnimationFrame(update);
    return () => cancelAnimationFrame(frame);
  }, [timebase.bpm, timebase.ppq]);

  const seekLocally = (tick: number) => {
    anchor.current = {
      tick,
      at: performance.now(),
      playing: transport?.state === 'playing',
    };
    setDisplayTick(tick);
  };

  return { transport, displayTick, seekLocally };
}
