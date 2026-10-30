import type {
  ArrangementMutationResult,
  AudioStatus,
  CreativeSession,
  Track,
} from '@/model/generated';
import { formatGainDb, formatPan } from '@/shared/audio/mix-format';
import { useSessionContext } from '@/shared/session/session-context';
import { MIX_GAIN_DB, MIX_PAN } from '@/model/mix-range';
import { useChannelMixValue } from '@/shared/session/useChannelMixValue';
import { Icon } from '@/shared/ui/primitives';
import clsx from 'clsx';
import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import { resolveTrackColor, TRACK_COLOR_PALETTE } from '@/model/track-color';
import sharedStyles from './Inspector.module.css';
import styles from './TrackInspector.module.css';
import { MixValueField } from './MixValueField';

interface TrackInspectorProps {
  track: Track;
  session: CreativeSession;
  audio: AudioStatus;
  onOpenDevices: () => void;
}

export function TrackInspector(props: TrackInspectorProps) {
  const { api, commit: sessionCommit } = useSessionContext();
  const [name, setName] = useState(props.track.name);
  const [colorOpen, setColorOpen] = useState(false);
  const [operationMessage, setOperationMessage] = useState<string | null>(null);
  const gain = useChannelMixValue(props.track, 'gainDb', {
    sessionId: props.session.sessionId,
    onError: setOperationMessage,
  });
  const pan = useChannelMixValue(props.track, 'pan', {
    sessionId: props.session.sessionId,
    onError: setOperationMessage,
  });
  useEffect(() => setName(props.track.name), [props.track.id, props.track.name]);
  const commit = useCallback(
    (operation: Promise<ArrangementMutationResult>) => {
      setOperationMessage(null);
      void sessionCommit(operation, { onError: setOperationMessage });
    },
    [sessionCommit],
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
    <div
      className={sharedStyles.inspector}
      style={{ '--track-color': displayColor } as CSSProperties}
    >
      <div className={sharedStyles.identity}>
        <button
          type="button"
          className={sharedStyles.identityTile}
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
                  commit(api.updateTrack(props.track.id, { color: hex }));
                }}
              />
            ))}
            <button
              type="button"
              className={clsx(styles.colorSwatch, styles.colorClear)}
              aria-label="Clear track color"
              onClick={() => {
                setColorOpen(false);
                commit(api.updateTrack(props.track.id, { color: '' }));
              }}
            >
              ×
            </button>
          </div>
        )}
        <div className={sharedStyles.identityBlock}>
          <input
            className={sharedStyles.identityName}
            aria-label="Track name"
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            onBlur={() => {
              const next = name.trim();
              if (next && next !== props.track.name) {
                commit(api.updateTrack(props.track.id, { name: next }));
              } else {
                setName(props.track.name);
              }
            }}
          />
          <small>{instrumentTrack ? 'Instrument track' : 'Audio track'}</small>
        </div>
      </div>

      <section className={sharedStyles.section} aria-label="Track mix">
        <div className={sharedStyles.rows}>
          <MixValueField
            label="Volume"
            name="Track gain"
            control={gain}
            range={MIX_GAIN_DB}
            fillOrigin={MIX_GAIN_DB.min}
            format={formatGainDb}
          />
          <MixValueField
            label="Pan"
            name="Track pan"
            control={pan}
            range={MIX_PAN}
            fillOrigin={0}
            format={formatPan}
          />
        </div>
      </section>

      <section className={sharedStyles.section} aria-label="Track input">
        <header className={sharedStyles.sectionHeader}>
          <strong>INPUT</strong>
        </header>
        {instrumentTrack ? (
          <div className={styles.controlPair}>
            <select
              className={sharedStyles.control}
              aria-label="MIDI input"
              value={props.track.midiInput.deviceId ?? ''}
              onChange={(event) =>
                commit(
                  api.setTrackMidiInput(props.track.id, {
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
              className={sharedStyles.control}
              aria-label="MIDI channel"
              value={props.track.midiInput.channel ?? ''}
              onChange={(event) =>
                commit(
                  api.setTrackMidiInput(props.track.id, {
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
          <div className={sharedStyles.rows}>
            <select
              className={sharedStyles.control}
              aria-label="Audio input"
              value={props.track.audioInput?.channelIndex ?? ''}
              onChange={(event) =>
                commit(
                  api.setTrackAudioInput(
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
            <div className={sharedStyles.labeledRow}>
              <span className={sharedStyles.rowLabel}>Monitor</span>
              <div className={sharedStyles.segmented} role="group" aria-label="Monitoring">
                {(['off', 'auto', 'on'] as const).map((monitoring) => (
                  <button
                    type="button"
                    key={monitoring}
                    aria-pressed={props.track.monitoring === monitoring}
                    onClick={() => commit(api.updateTrack(props.track.id, { monitoring }))}
                  >
                    {monitoring === 'off' ? 'Off' : monitoring === 'auto' ? 'Auto' : 'On'}
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}
      </section>

      <section className={sharedStyles.section} aria-label="Track devices">
        <header className={sharedStyles.sectionHeader}>
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
        <p className={sharedStyles.message} role="status">
          {operationMessage}
        </p>
      )}
    </div>
  );
}
