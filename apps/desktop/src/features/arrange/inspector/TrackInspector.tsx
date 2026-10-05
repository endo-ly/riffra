import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import clsx from 'clsx';
import type {
  ArrangementMutationResult,
  AudioStatus,
  CanonicalState,
  CreativeSession,
  Track,
} from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { Icon } from '@/shared/ui/primitives';
import { resolveTrackColor, TRACK_COLOR_PALETTE } from '../model/track-colors';
import { useInspectorOperation } from './useInspectorOperation';
import styles from './Inspector.module.css';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface TrackInspectorProps {
  track: Track;
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  audio: AudioStatus;
  onOpenDevices: () => void;
  api: ArrangeInspectorApi;
}

function formatPan(pan: number) {
  if (Math.abs(pan) < 0.01) return 'C';
  return `${pan < 0 ? 'L' : 'R'} ${Math.round(Math.abs(pan) * 100)}`;
}

export function TrackInspector(props: TrackInspectorProps) {
  const [name, setName] = useState(props.track.name);
  const [gainDb, setGainDb] = useState(props.track.gainDb);
  const [pan, setPan] = useState(props.track.pan);
  const [gainEdit, setGainEdit] = useState(false);
  const [panEdit, setPanEdit] = useState(false);
  const [colorOpen, setColorOpen] = useState(false);
  const { operationMessage, runOperation, setOperationMessage } = useInspectorOperation();
  useEffect(() => setName(props.track.name), [props.track.id, props.track.name]);
  useEffect(() => setGainDb(props.track.gainDb), [props.track.id, props.track.gainDb]);
  useEffect(() => setPan(props.track.pan), [props.track.id, props.track.pan]);
  const commit = useCallback(
    (operation: Promise<ArrangementMutationResult>) => {
      runOperation(operation, (result) =>
        applyArrangementMutation(result, props.applyCanonicalState, setOperationMessage),
      );
    },
    [props.applyCanonicalState, runOperation, setOperationMessage],
  );
  const trackIndex = props.session.arrangement.tracks.findIndex((t) => t.id === props.track.id);
  const displayColor = resolveTrackColor(props.track, trackIndex);
  return (
    <div className={styles.inspector}>
      <div className={styles.identity}>
        <button
          type="button"
          className={styles.colorDot}
          aria-label="Track color"
          title={props.track.color ? `Color ${props.track.color}` : `Auto color ${displayColor}`}
          style={{ '--track-color': displayColor } as CSSProperties}
          data-color={props.track.color ? 'custom' : 'auto'}
          onClick={() => setColorOpen((open) => !open)}
        />
        {colorOpen && (
          <div className={styles.colorPalette} role="group" aria-label="Track color">
            {TRACK_COLOR_PALETTE.map((hex) => (
              <button
                key={hex}
                type="button"
                className={styles.colorSwatch}
                aria-label={`Set track color ${hex}`}
                aria-pressed={props.track.color === hex}
                style={{ '--swatch': hex } as CSSProperties}
                onClick={() => {
                  setColorOpen(false);
                  commit(props.api.updateTrack(props.track.id, { color: hex }));
                }}
              />
            ))}
            <button
              type="button"
              className={clsx(styles.colorSwatch, styles.colorClear)}
              aria-label="Clear track color"
              onClick={() => {
                setColorOpen(false);
                commit(props.api.updateTrack(props.track.id, { color: '' }));
              }}
            >
              ×
            </button>
          </div>
        )}
        <span className={styles.identityIcon}>
          <Icon name={props.track.kind === 'audio' ? 'wave' : 'note'} />
        </span>
        <input
          className={styles.identityName}
          aria-label="Track name"
          value={name}
          onChange={(event) => setName(event.currentTarget.value)}
          onBlur={() => {
            const next = name.trim();
            if (next && next !== props.track.name) {
              commit(props.api.updateTrack(props.track.id, { name: next }));
            } else {
              setName(props.track.name);
            }
          }}
        />
      </div>

      <div className={styles.mixCluster} aria-label="Track mix">
        <label className={styles.mixField}>
          <span>
            Gain{' '}
            {gainEdit ? (
              <input
                className={styles.valueInput}
                autoFocus
                type="number"
                step="0.1"
                value={gainDb}
                onChange={(event) => setGainDb(Number(event.currentTarget.value))}
                onBlur={() => {
                  setGainEdit(false);
                  const next = Number(gainDb);
                  if (Number.isFinite(next) && next !== props.track.gainDb)
                    commit(props.api.updateTrack(props.track.id, { gainDb: next }));
                  else setGainDb(props.track.gainDb);
                }}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') (event.currentTarget as HTMLInputElement).blur();
                  if (event.key === 'Escape') {
                    setGainDb(props.track.gainDb);
                    setGainEdit(false);
                  }
                }}
              />
            ) : (
              <button
                type="button"
                className={styles.value}
                aria-label="Edit track gain"
                onClick={() => setGainEdit(true)}
              >
                {gainDb > 0 ? '+' : ''}
                {gainDb.toFixed(1)} dB
              </button>
            )}
          </span>
          <input
            className={styles.range}
            aria-label="Track gain"
            type="range"
            min="-60"
            max="12"
            step="0.5"
            value={gainDb}
            onChange={(event) => setGainDb(Number(event.currentTarget.value))}
            onPointerUp={() => {
              if (gainDb !== props.track.gainDb)
                commit(props.api.updateTrack(props.track.id, { gainDb }));
            }}
            onKeyUp={() => {
              if (gainDb !== props.track.gainDb)
                commit(props.api.updateTrack(props.track.id, { gainDb }));
            }}
          />
        </label>
        <label className={styles.mixField}>
          <span>
            Pan{' '}
            {panEdit ? (
              <input
                className={styles.valueInput}
                autoFocus
                type="number"
                step="0.05"
                value={pan}
                onChange={(event) => setPan(Number(event.currentTarget.value))}
                onBlur={() => {
                  setPanEdit(false);
                  const next = Number(pan);
                  if (Number.isFinite(next) && next !== props.track.pan)
                    commit(props.api.updateTrack(props.track.id, { pan: next }));
                  else setPan(props.track.pan);
                }}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') (event.currentTarget as HTMLInputElement).blur();
                  if (event.key === 'Escape') {
                    setPan(props.track.pan);
                    setPanEdit(false);
                  }
                }}
              />
            ) : (
              <button
                type="button"
                className={styles.value}
                aria-label="Edit track pan"
                onClick={() => setPanEdit(true)}
              >
                {formatPan(pan)}
              </button>
            )}
          </span>
          <input
            className={styles.range}
            aria-label="Track pan"
            type="range"
            min="-1"
            max="1"
            step="0.05"
            value={pan}
            onChange={(event) => setPan(Number(event.currentTarget.value))}
            onPointerUp={() => {
              if (pan !== props.track.pan) commit(props.api.updateTrack(props.track.id, { pan }));
            }}
            onKeyUp={() => {
              if (pan !== props.track.pan) commit(props.api.updateTrack(props.track.id, { pan }));
            }}
          />
        </label>
      </div>

      {props.track.kind === 'audio' ? (
        <>
          <section className={styles.section}>
            <header className={styles.sectionHeader}>
              <strong>INPUT</strong>
            </header>
            <select
              className={styles.control}
              aria-label="Audio input"
              value={props.track.audioInput?.channelIndex ?? ''}
              onChange={(event) =>
                commit(
                  props.api.setTrackAudioInput(
                    props.track.id,
                    event.currentTarget.value === '' ? null : Number(event.currentTarget.value),
                  ),
                )
              }
            >
              <option value="">None</option>
              {props.audio.inputChannels.map((channel) => (
                <option key={channel.index} value={channel.index}>
                  {channel.name}
                </option>
              ))}
              {props.track.audioInput &&
                !props.audio.inputChannels.some(
                  (channel) => channel.index === props.track.audioInput?.channelIndex,
                ) && (
                  <option value={props.track.audioInput.channelIndex}>
                    Input {props.track.audioInput.channelIndex + 1} · Unavailable
                  </option>
                )}
            </select>
            <div
              className={clsx(styles.segmented, styles.segmentedGap)}
              role="group"
              aria-label="Monitoring"
            >
              {(['off', 'auto', 'on'] as const).map((monitoring) => (
                <button
                  type="button"
                  key={monitoring}
                  aria-pressed={props.track.monitoring === monitoring}
                  onClick={() => commit(props.api.updateTrack(props.track.id, { monitoring }))}
                >
                  {monitoring === 'off' ? 'Off' : monitoring === 'auto' ? 'Auto' : 'On'}
                </button>
              ))}
            </div>
          </section>
        </>
      ) : (
        <>
          <section className={styles.section}>
            <header className={styles.sectionHeader}>
              <strong>MIDI INPUT</strong>
            </header>
            <div className={styles.fieldColumn}>
              <select
                className={styles.control}
                aria-label="MIDI input"
                value={props.track.midiInput.deviceId ?? ''}
                onChange={(event) =>
                  commit(
                    props.api.setTrackMidiInput(props.track.id, {
                      ...props.track.midiInput,
                      deviceId: event.currentTarget.value || undefined,
                    }),
                  )
                }
              >
                <option value="">All Inputs</option>
                {props.audio.midiInputs.map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.name}
                  </option>
                ))}
              </select>
              <select
                className={styles.control}
                aria-label="MIDI channel"
                value={props.track.midiInput.channel ?? ''}
                onChange={(event) =>
                  commit(
                    props.api.setTrackMidiInput(props.track.id, {
                      ...props.track.midiInput,
                      channel: event.currentTarget.value
                        ? Number(event.currentTarget.value)
                        : undefined,
                    }),
                  )
                }
              >
                <option value="">All Channels</option>
                {Array.from({ length: 16 }, (_, index) => index + 1).map((channel) => (
                  <option key={channel} value={channel}>
                    Channel {channel}
                  </option>
                ))}
              </select>
            </div>
          </section>
        </>
      )}
      <section className={styles.section} aria-label="Track devices">
        {props.track.kind === 'instrument' && (
          <>
            <strong>INSTRUMENT</strong>
            <p>{props.track.instrument?.name ?? 'None'}</p>
          </>
        )}
        <strong>EFFECTS</strong>
        <p>{props.track.effects.length} Effects</p>
        <button type="button" className={styles.smallButton} onClick={props.onOpenDevices}>
          Open Devices
        </button>
      </section>
      {operationMessage && (
        <p className={styles.message} role="status">
          {operationMessage}
        </p>
      )}
    </div>
  );
}
