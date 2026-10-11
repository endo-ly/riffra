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
  const instrumentTrack = props.track.kind === 'instrument';
  const effectCount = props.track.effects.length;
  const effectSummary =
    effectCount === 0 ? 'No effects' : `${effectCount} effect${effectCount === 1 ? '' : 's'}`;
  const deviceSummary = instrumentTrack
    ? [props.track.instrument?.name ?? 'No instrument', effectSummary]
    : [effectSummary];
  return (
    <div className={styles.inspector} style={{ '--track-color': displayColor } as CSSProperties}>
      <div className={styles.identity}>
        <button
          type="button"
          className={styles.identityTile}
          aria-label="Track color"
          aria-expanded={colorOpen}
          title={props.track.color ? `Color ${props.track.color}` : `Auto color ${displayColor}`}
          onClick={() => setColorOpen((open) => !open)}
        >
          <Icon name={instrumentTrack ? 'note' : 'wave'} />
        </button>
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
        <div className={styles.identityBlock}>
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
          <small>{instrumentTrack ? 'Instrument track' : 'Audio track'}</small>
        </div>
      </div>

      <section className={styles.section} aria-label="Track mix">
        <div className={styles.rows}>
          <MixValueField
            label="Volume"
            name="Track gain"
            value={props.track.gainDb}
            min={-60}
            max={12}
            origin={-60}
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
            origin={0}
            step={0.05}
            inputStep={0.05}
            format={formatPan}
            onCommit={(pan) => commit(props.api.updateTrack(props.track.id, { pan }))}
          />
        </div>
      </section>

      <section className={styles.section} aria-label="Track input">
        <header className={styles.sectionHeader}>
          <strong>INPUT</strong>
        </header>
        {instrumentTrack ? (
          <div className={styles.controlPair}>
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
              <option value="">All Ch</option>
              {Array.from({ length: 16 }, (_, index) => index + 1).map((channel) => (
                <option key={channel} value={channel}>
                  Ch {channel}
                </option>
              ))}
            </select>
          </div>
        ) : (
          <div className={styles.rows}>
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
            <div className={styles.labeledRow}>
              <span className={styles.rowLabel}>Monitor</span>
              <div className={styles.segmented} role="group" aria-label="Monitoring">
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
            </div>
          </div>
        )}
      </section>

      <section className={styles.section} aria-label="Track devices">
        <header className={styles.sectionHeader}>
          <strong>DEVICES</strong>
        </header>
        <button
          type="button"
          className={styles.deviceCard}
          aria-label={`Open Devices: ${deviceSummary.join(', ')}`}
          onClick={props.onOpenDevices}
        >
          <span className={styles.deviceIcon}>
            <Icon name={instrumentTrack ? 'keys' : 'module'} />
          </span>
          <span className={styles.deviceText}>
            <strong>{deviceSummary[0]}</strong>
            {deviceSummary[1] && <small>{deviceSummary[1]}</small>}
          </span>
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
