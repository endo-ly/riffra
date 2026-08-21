import { useCallback, useEffect, useState } from 'react';
import clsx from 'clsx';
import type {
  ArrangementMutationResult,
  AudioStatus,
  CreativeSession,
  PluginEntry,
  Track,
} from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { Icon } from '@/shared/ui/primitives';
import { TrackPluginChainEditor } from './TrackPluginChainEditor';
import { PluginPicker } from './PluginPicker';
import { useInspectorOperation } from './useInspectorOperation';
import styles from './Inspector.module.css';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface TrackInspectorProps {
  track: Track;
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  audio: AudioStatus;
  missingDeviceIds: string[];
  plugins: PluginEntry[];
  onDisableMissingPlugin: (deviceId: string) => Promise<void>;
  onReplaceMissingPlugin: (deviceId: string, newPath: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
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
  const [instrumentPickerOpen, setInstrumentPickerOpen] = useState(false);
  const [replaceTarget, setReplaceTarget] = useState<{ deviceId: string } | null>(null);
  const { operationMessage, runOperation, setOperationMessage } = useInspectorOperation();
  useEffect(() => setName(props.track.name), [props.track.id, props.track.name]);
  useEffect(() => setGainDb(props.track.gainDb), [props.track.id, props.track.gainDb]);
  useEffect(() => setPan(props.track.pan), [props.track.id, props.track.pan]);
  const commit = useCallback(
    (operation: Promise<ArrangementMutationResult>) => {
      runOperation(operation, (result) =>
        applyArrangementMutation(result, props.setSession, setOperationMessage),
      );
    },
    [props.setSession, runOperation, setOperationMessage],
  );
  const setInstrument = (plugin: PluginEntry) => {
    commit(props.api.setTrackInstrument(props.track.id, plugin.path));
  };
  const instrumentUnavailable =
    props.track.instrument?.disabledPlaceholder ||
    (props.track.instrument ? props.missingDeviceIds.includes(props.track.instrument.id) : false);
  const instrument = props.track.instrument;
  return (
    <div className={styles.inspector}>
      <div className={styles.identity}>
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
            <span className={styles.value}>
              {gainDb > 0 ? '+' : ''}
              {gainDb.toFixed(1)} dB
            </span>
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
            Pan <span className={styles.value}>{formatPan(pan)}</span>
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
          <section className={styles.section}>
            <header className={styles.sectionHeader}>
              <strong>INSTRUMENT</strong>
            </header>
            {instrumentPickerOpen && (
              <PluginPicker
                api={props.api}
                plugins={props.plugins}
                title="Choose Instrument"
                onSelect={(plugin) => {
                  setInstrument(plugin);
                  setInstrumentPickerOpen(false);
                }}
                onClose={() => setInstrumentPickerOpen(false)}
              />
            )}
            <div className={styles.deviceRow}>
              <span className={styles.deviceIcon}>
                <Icon name="module" />
              </span>
              <div className={styles.deviceMeta}>
                <strong>{instrument?.name ?? 'None'}</strong>
                {!instrument && <small>No instrument selected</small>}
              </div>
              <div className={clsx(styles.deviceActions, styles.visible)}>
                {!instrumentUnavailable && (
                  <button
                    type="button"
                    className={styles.textButton}
                    onClick={() => setInstrumentPickerOpen(true)}
                  >
                    {instrument ? 'Change' : 'Choose'}
                  </button>
                )}
                {instrument && !instrumentUnavailable && (
                  <>
                    <button
                      type="button"
                      className={clsx(styles.textButton, styles.plain)}
                      onClick={() =>
                        runOperation(props.api.openTrackPluginEditor(props.track.id, instrument.id))
                      }
                    >
                      Edit
                    </button>
                    <button
                      type="button"
                      className={clsx(styles.textButton, styles.plain)}
                      onClick={() => commit(props.api.clearTrackInstrument(props.track.id))}
                    >
                      Clear
                    </button>
                  </>
                )}
              </div>
            </div>
            {instrumentUnavailable && (
              <div className={styles.missingState}>
                <strong>
                  {instrument?.disabledPlaceholder ? 'DISABLED PLACEHOLDER' : 'MISSING PLUGIN'}
                </strong>
                <button
                  type="button"
                  className={styles.smallButton}
                  onClick={() => runOperation(props.onRescanMissingPlugins())}
                >
                  Re-scan
                </button>
                <button
                  type="button"
                  className={styles.smallButton}
                  onClick={() => setReplaceTarget({ deviceId: instrument!.id })}
                >
                  Replace
                </button>
                {!instrument?.disabledPlaceholder && (
                  <button
                    type="button"
                    className={styles.smallButton}
                    onClick={() => runOperation(props.onDisableMissingPlugin(instrument!.id))}
                  >
                    Disable
                  </button>
                )}
              </div>
            )}
            {replaceTarget && (
              <PluginPicker
                api={props.api}
                plugins={props.plugins}
                title="Replace Plugin"
                onSelect={(plugin) => {
                  runOperation(props.onReplaceMissingPlugin(replaceTarget.deviceId, plugin.path));
                  setReplaceTarget(null);
                }}
                onClose={() => setReplaceTarget(null)}
              />
            )}
          </section>
        </>
      )}

      <TrackPluginChainEditor
        track={props.track}
        api={props.api}
        plugins={props.plugins}
        commit={commit}
        missingDeviceIds={props.missingDeviceIds}
        onDisableMissingPlugin={props.onDisableMissingPlugin}
        onReplaceMissingPlugin={props.onReplaceMissingPlugin}
        onRescanMissingPlugins={props.onRescanMissingPlugins}
        runOperation={runOperation}
      />
      {operationMessage && (
        <p className={styles.message} role="status">
          {operationMessage}
        </p>
      )}
    </div>
  );
}
