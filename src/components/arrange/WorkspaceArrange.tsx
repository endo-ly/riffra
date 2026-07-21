import { useEffect, useMemo, useRef, useState } from 'react';
import type { AssetId, CreativeSession, TransportStatus } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import styles from './WorkspaceArrange.module.css';

const HEADER_WIDTH = 172;
const PIXELS_PER_QUARTER = 92;
const ASSET_MIME = 'application/x-riffra-asset';

interface WorkspaceArrangeProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  api: NativeApi;
}

function clipDurationTicks(
  clip: CreativeSession['arrangement']['audioClips'][number],
  session: CreativeSession,
) {
  return Math.max(
    1,
    Math.round(
      (clip.timelineDuration.frames / clip.timelineDuration.sampleRate) *
        (session.arrangement.timebase.bpm / 60) *
        session.arrangement.timebase.ppq,
    ),
  );
}

export function WorkspaceArrange({ session, setSession, api }: WorkspaceArrangeProps) {
  const { arrangement } = session;
  const { timebase } = arrangement;
  const [transport, setTransport] = useState<TransportStatus | null>(null);
  const [displayTick, setDisplayTick] = useState(0);
  const [selectedClipId, setSelectedClipId] = useState<string | null>(null);
  const [message, setMessage] = useState('Drop an audio Asset to place it on the timeline.');
  const anchor = useRef<{ tick: number; at: number; playing: boolean }>({
    tick: 0,
    at: performance.now(),
    playing: false,
  });

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

  const pixelsPerTick = PIXELS_PER_QUARTER / timebase.ppq;
  const barTicks =
    (timebase.ppq * 4 * timebase.timeSignatureNumerator) / timebase.timeSignatureDenominator;
  const timelineTicks = useMemo(() => {
    const contentEnd = arrangement.audioClips.reduce(
      (end, clip) => Math.max(end, clip.startTick + clipDurationTicks(clip, session)),
      0,
    );
    return Math.max(barTicks * 16, contentEnd + barTicks * 2);
  }, [arrangement.audioClips, barTicks, session]);
  const timelineWidth = timelineTicks * pixelsPerTick;
  const bars = Array.from({ length: Math.ceil(timelineTicks / barTicks) }, (_, index) => index);

  const commit = async (operation: Promise<CreativeSession | null>, success: string) => {
    try {
      const next = await operation;
      if (next) setSession(next);
      setMessage(success);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const dropAsset = async (event: React.DragEvent, trackId?: string) => {
    event.preventDefault();
    const raw = event.dataTransfer.getData(ASSET_MIME);
    if (!raw) return;
    try {
      const asset = JSON.parse(raw) as { id: string; name: string; kind: string };
      if (asset.kind !== 'audio') {
        setMessage('Only audio Assets can be placed on an Audio Track.');
        return;
      }
      const timeline = event.currentTarget.closest(`.${styles.timeline}`);
      const bounds =
        timeline?.getBoundingClientRect() ?? event.currentTarget.getBoundingClientRect();
      const tick = Math.max(
        0,
        Math.round((event.clientX - bounds.left - HEADER_WIDTH) / pixelsPerTick),
      );
      await commit(
        api.addAudioClipToArrangement(asset.id as AssetId, asset.name, tick, trackId),
        `${asset.name} placed at tick ${tick.toLocaleString()}.`,
      );
    } catch {
      setMessage('The dragged Library item is not a valid audio Asset.');
    }
  };

  const seekFromPointer = (event: React.PointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const tick = Math.max(0, Math.round((event.clientX - bounds.left) / pixelsPerTick));
    anchor.current = { tick, at: performance.now(), playing: transport?.state === 'playing' };
    setDisplayTick(tick);
    void api.seekTimeline(tick).catch((error) => setMessage(String(error)));
  };

  const positionLabel = (() => {
    const bar = Math.floor(displayTick / barTicks) + 1;
    const tickInBar = displayTick % barTicks;
    const beatTicks = (timebase.ppq * 4) / timebase.timeSignatureDenominator;
    const beat = Math.floor(tickInBar / beatTicks) + 1;
    return `${bar}.${beat}`;
  })();

  return (
    <section className={styles.workspace} aria-label="Arrange timeline">
      <header className={styles.toolbar}>
        <div>
          <strong>ARRANGE</strong>
          <span>REV {arrangement.revision}</span>
        </div>
        <div className={styles.clock}>
          <strong>{positionLabel}</strong>
          <span>{Math.round(displayTick).toLocaleString()} ticks</span>
        </div>
        <div className={styles.timebase}>
          <span>{timebase.bpm.toFixed(1)} BPM</span>
          <span>
            {timebase.timeSignatureNumerator}/{timebase.timeSignatureDenominator}
          </span>
          <span>{transport?.state ?? 'stopped'}</span>
        </div>
        <button
          onClick={() =>
            void commit(
              api.addTrack(`Audio ${arrangement.tracks.length + 1}`),
              'Audio Track added.',
            )
          }
        >
          + Audio Track
        </button>
      </header>

      <div className={styles.scroller}>
        <div className={styles.timeline} style={{ width: HEADER_WIDTH + timelineWidth }}>
          <div className={styles.rulerCorner}>TRACKS</div>
          <div
            className={styles.ruler}
            aria-label="Timeline ruler"
            style={{ left: HEADER_WIDTH, width: timelineWidth }}
            onPointerDown={seekFromPointer}
          >
            {bars.map((bar) => (
              <span key={bar} style={{ left: bar * barTicks * pixelsPerTick }}>
                {bar + 1}
              </span>
            ))}
          </div>
          <div
            className={styles.playhead}
            style={{ left: HEADER_WIDTH + displayTick * pixelsPerTick }}
          />

          {arrangement.tracks.length === 0 ? (
            <div
              className={styles.empty}
              onDragOver={(event) => event.preventDefault()}
              onDrop={(event) => void dropAsset(event)}
            >
              <strong>Drop audio here</strong>
              <span>The first Audio Track is created automatically.</span>
            </div>
          ) : (
            arrangement.tracks.map((track) => (
              <div
                className={styles.trackRow}
                key={track.id}
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => void dropAsset(event, track.id)}
              >
                <aside className={styles.trackHeader}>
                  <strong>{track.name}</strong>
                  <span>{track.kind}</span>
                  <div>
                    <button
                      className={track.muted ? styles.active : ''}
                      aria-label={`Mute ${track.name}`}
                      onClick={() =>
                        void commit(
                          api.updateTrack(track.id, { muted: !track.muted }),
                          `${track.name} mute updated.`,
                        )
                      }
                    >
                      M
                    </button>
                    <button
                      className={track.solo ? styles.active : ''}
                      aria-label={`Solo ${track.name}`}
                      onClick={() =>
                        void commit(
                          api.updateTrack(track.id, { solo: !track.solo }),
                          `${track.name} solo updated.`,
                        )
                      }
                    >
                      S
                    </button>
                  </div>
                  <input
                    className={styles.trackGain}
                    aria-label={`${track.name} gain`}
                    type="range"
                    min="-60"
                    max="12"
                    step="0.5"
                    defaultValue={track.gainDb}
                    onPointerUp={(event) =>
                      void commit(
                        api.updateTrack(track.id, { gainDb: Number(event.currentTarget.value) }),
                        `${track.name} gain updated.`,
                      )
                    }
                  />
                </aside>
                <div className={styles.lane} style={{ width: timelineWidth }}>
                  {bars.map((bar) => (
                    <i key={bar} style={{ left: bar * barTicks * pixelsPerTick }} />
                  ))}
                  {arrangement.audioClips
                    .filter((clip) => clip.trackId === track.id)
                    .map((clip) => {
                      const duration = clipDurationTicks(clip, session);
                      return (
                        <button
                          key={clip.id}
                          className={`${styles.clip} ${selectedClipId === clip.id ? styles.selected : ''}`}
                          style={{
                            left: clip.startTick * pixelsPerTick,
                            width: Math.max(18, duration * pixelsPerTick),
                            opacity: clip.muted ? 0.45 : 1,
                          }}
                          onClick={() => setSelectedClipId(clip.id)}
                          onPointerDown={(event) => {
                            const originX = event.clientX;
                            const originTick = clip.startTick;
                            event.currentTarget.setPointerCapture(event.pointerId);
                            const target = event.currentTarget;
                            const onMove = (move: PointerEvent) => {
                              const next = Math.max(
                                0,
                                Math.round(originTick + (move.clientX - originX) / pixelsPerTick),
                              );
                              target.style.left = `${next * pixelsPerTick}px`;
                              target.dataset.pendingTick = String(next);
                            };
                            const onUp = () => {
                              target.removeEventListener('pointermove', onMove);
                              target.removeEventListener('pointerup', onUp);
                              const next = Number(target.dataset.pendingTick ?? originTick);
                              delete target.dataset.pendingTick;
                              if (next !== originTick)
                                void commit(
                                  api.updateAudioClip(clip.id, { startTick: next }),
                                  `${clip.name} moved.`,
                                );
                            };
                            target.addEventListener('pointermove', onMove);
                            target.addEventListener('pointerup', onUp);
                          }}
                        >
                          <strong>{clip.name}</strong>
                          <span>
                            {(
                              clip.timelineDuration.frames / clip.timelineDuration.sampleRate
                            ).toFixed(2)}
                            s
                          </span>
                        </button>
                      );
                    })}
                </div>
              </div>
            ))
          )}
        </div>
      </div>
      <footer className={styles.status}>
        <span>{message}</span>
        {selectedClipId && (
          <button
            onClick={() =>
              void commit(api.removeAudioClip(selectedClipId), 'Clip removed.').then(() =>
                setSelectedClipId(null),
              )
            }
          >
            Remove selected clip
          </button>
        )}
      </footer>
    </section>
  );
}

export { ASSET_MIME };
