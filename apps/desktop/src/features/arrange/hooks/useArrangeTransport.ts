import { useEffect, useRef, useState } from 'react';
import type { ProjectTimebase } from '@/model/domain';
import type { TransportStatus } from '@/native/contracts';
import type { AudioApi, NativeEventApi } from '@/native/native-api';

export function useArrangeTransport(
  api: Pick<NativeEventApi, 'onTransportStatus'> & Pick<AudioApi, 'getAudioStatus'>,
  timebase: ProjectTimebase,
) {
  const [transport, setTransport] = useState<TransportStatus | null>(null);
  const [displayTick, setDisplayTick] = useState(0);
  const displayTickRef = useRef(0);
  const anchor = useRef({ tick: 0, at: performance.now(), playing: false });
  const receivedTransportStatus = useRef(false);

  const publishTick = (tick: number) => {
    displayTickRef.current = tick;
    setDisplayTick(tick);
  };

  const transportMeaningfullyChanged = (
    previous: TransportStatus | null,
    next: TransportStatus,
  ): boolean => {
    if (!previous) return true;
    return (
      previous.state !== next.state ||
      previous.revision !== next.revision ||
      previous.recordingPhase !== next.recordingPhase ||
      previous.recordingStartTick !== next.recordingStartTick ||
      previous.recordingPassOrdinal !== next.recordingPassOrdinal ||
      previous.clockGeneration !== next.clockGeneration ||
      previous.discontinuity !== next.discontinuity ||
      previous.unavailableClipIds.join('\u0000') !== next.unavailableClipIds.join('\u0000') ||
      previous.missingDeviceIds.join('\u0000') !== next.missingDeviceIds.join('\u0000') ||
      previous.armedTrackIds.join('\u0000') !== next.armedTrackIds.join('\u0000')
    );
  };

  useEffect(() => {
    receivedTransportStatus.current = false;
    const unlisten = api.onTransportStatus((status) => {
      receivedTransportStatus.current = true;
      setTransport((previous) =>
        transportMeaningfullyChanged(previous, status) ? status : previous,
      );
      anchor.current = {
        tick: status.timelineTick,
        at: performance.now(),
        playing: status.state === 'playing',
      };
      // A transport status is authoritative at every discontinuity. Publishing
      // through React as well as the animation ref makes stopped seeks visible
      // to the clock and to the playhead effect.
      publishTick(status.timelineTick);
    });
    api
      .getAudioStatus()
      .then((status) => {
        if (receivedTransportStatus.current || status.timelineTick == null) return;
        anchor.current.tick = status.timelineTick;
        anchor.current.at = performance.now();
        publishTick(status.timelineTick);
      })
      .catch(() => undefined);
    return unlisten;
  }, [api]);

  useEffect(() => {
    let frame = 0;
    let lastUiUpdate = 0;
    const update = (now: number) => {
      const current = anchor.current;
      const elapsed = current.playing ? performance.now() - current.at : 0;
      const tick = current.tick + (elapsed * timebase.bpm * timebase.ppq) / 60_000;
      // The playhead itself is animated by a tiny DOM-only component. The
      // editor needs a React snapshot only for the toolbar clock and editing
      // actions; rebuilding every ArrangeTrack on every animation frame made
      // playback consume the WebView's entire event loop.
      displayTickRef.current = tick;
      if (now - lastUiUpdate >= 250) {
        lastUiUpdate = now;
        setDisplayTick(tick);
      }
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
    publishTick(tick);
  };

  return { transport, displayTick, displayTickRef, seekLocally };
}
