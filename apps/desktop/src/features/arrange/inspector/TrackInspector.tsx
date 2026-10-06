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
import { formatGainDb, formatPan } from '@/shared/audio/mix-format';
import { MixValueField } from './MixValueField';
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

export function TrackInspector(props: TrackInspectorProps) {
  const [name, setName] = useState(props.track.name);
  const [colorOpen, setColorOpen] = useState(false);
  const { operationMessage, runOperation, setOperationMessage } = useInspectorOperation();
  useEffect(() => setName(props.track.name), [props.track.id, props.track.name]);
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
        <MixValueField
          label="Gain"
          name="Track gain"
          value={props.track.gainDb}
          min={-60}
          max={12}
          step={0.5}
          inputStep={0.1}
          format={formatGainDb}
          onCommit={(gainDb) => commit(props.api.updateTrack(props.track.id, { gainDb }))}
        />
        <MixValueField
          label="Pan"
          name="Track pan"
          value={props.track.pan}
          min={-1}
          max={1}
          step={0.05}
          inputStep={0.05}
          format={formatPan}
          onCommit={(pan) => commit(props.api.updateTrack(props.track.id, { pan }))}
        />
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
        <header className={styles.sectionHeader}>
          <strong>DEVICES</strong>
          <button type="button" className={styles.headerAction} onClick={props.onOpenDevices}>
            Open Devices
          </button>
        </header>
        <div className={styles.fieldColumn}>
          {props.track.kind === 'instrument' && (
            <div className={styles.summaryRow}>
              <span>Instrument</span>
              <strong>{props.track.instrument?.name ?? 'None'}</strong>
            </div>
          )}
          <div className={styles.summaryRow}>
            <span>Effects</span>
            <strong>{props.track.effects.length}</strong>
          </div>
        </div>
      </section>
      {operationMessage && (
        <p className={styles.message} role="status">
          {operationMessage}
        </p>
      )}
    </div>
  );
}
