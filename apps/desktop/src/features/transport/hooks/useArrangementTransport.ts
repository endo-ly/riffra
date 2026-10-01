import { useEffect, useRef, useState } from 'react';
import type { ProjectTimebase } from '@/model/domain';
import type { TransportStatus } from '@/model/domain';
import type { AudioApi, NativeEventApi } from '@/native/native-api';
/** Arrangement Transport state shared by the Global Control Bar and the Arrange editors. */
export type ArrangementTransport = ReturnType<typeof useArrangementTransport>;

/**
 * Follows the native Arrangement Transport and interpolates its playhead between
 * transport statuses. The playhead restarts from the native position whenever the
 * Host or the Active Project changes. Without a timebase it holds the last reported tick.
 */
export function useArrangementTransport(
  api: Pick<NativeEventApi, 'onTransportStatus'> & Pick<AudioApi, 'getAudioStatus'>,
  timebase: ProjectTimebase | null,
  hostGeneration = 0,
  projectId: string | null = null,
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
      previous.armedTrackIds.join('\u0000') !== next.armedTrackIds.join('\u0000')
    );
  };

  useEffect(() => {
    receivedTransportStatus.current = false;
    setTransport(null);
    anchor.current = { tick: 0, at: performance.now(), playing: false };
    publishTick(0);
  }, [hostGeneration, projectId]);

  useEffect(() => {
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
    return unlisten;
  }, [api]);

  useEffect(() => {
    api
      .getAudioStatus()
      .then((status) => {
        if (receivedTransportStatus.current || status.timelineTick == null) return;
        anchor.current.tick = status.timelineTick;
        anchor.current.at = performance.now();
        publishTick(status.timelineTick);
      })
      .catch(() => undefined);
  }, [api, hostGeneration, projectId]);

  const bpm = timebase?.bpm;
  const ppq = timebase?.ppq;
  useEffect(() => {
    if (bpm === undefined || ppq === undefined) return;
    let frame = 0;
    let lastUiUpdate = 0;
    const update = (now: number) => {
      const current = anchor.current;
      const elapsed = current.playing ? performance.now() - current.at : 0;
      const tick = current.tick + (elapsed * bpm * ppq) / 60000;
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
  }, [bpm, ppq]);

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
