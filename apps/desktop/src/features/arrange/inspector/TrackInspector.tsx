import { useCallback, useEffect, useState } from 'react';
import type { AudioStatus, CreativeSession, PluginEntry, Track } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { TrackPluginChainEditor } from './TrackPluginChainEditor';
import { PluginPicker } from './PluginPicker';
import { useInspectorOperation } from './useInspectorOperation';
import styles from './TrackInspector.module.css';

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

export function TrackInspector(props: TrackInspectorProps) {
  const [name, setName] = useState(props.track.name);
  const [gainDb, setGainDb] = useState(props.track.gainDb);
  const [pan, setPan] = useState(props.track.pan);
  const [instrumentPickerOpen, setInstrumentPickerOpen] = useState(false);
  const [replaceTarget, setReplaceTarget] = useState<{ deviceId: string } | null>(null);
  const { operationMessage, runOperation } = useInspectorOperation();
  useEffect(() => setName(props.track.name), [props.track.id, props.track.name]);
  useEffect(() => setGainDb(props.track.gainDb), [props.track.id, props.track.gainDb]);
  useEffect(() => setPan(props.track.pan), [props.track.id, props.track.pan]);
  const commit = useCallback(
    (operation: Promise<CreativeSession>) => {
      runOperation(operation, props.setSession);
    },
    [props.setSession, runOperation],
  );
  const setInstrument = (plugin: PluginEntry) => {
    commit(props.api.setTrackInstrument(props.track.id, plugin.path));
  };
  const instrumentUnavailable =
    props.track.instrument?.disabledPlaceholder ||
    (props.track.instrument ? props.missingDeviceIds.includes(props.track.instrument.id) : false);
  const replaceMissing = (deviceId: string) => {
    setReplaceTarget({ deviceId });
  };
  return (
    <div className={styles.inspector}>
      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>TRACK</strong>
        </header>
        <label className={styles.field}>
          <span>Name</span>
          <input
            className={styles.control}
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
        </label>
      </section>
      {props.track.kind === 'audio' ? (
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
        </section>
      ) : (
        <>
          <section className={styles.section}>
            <header className={styles.sectionHeader}>
              <strong>MIDI INPUT</strong>
            </header>
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
          </section>
          <section className={styles.section}>
            <header className={styles.sectionHeader}>
              <strong>INSTRUMENT</strong>
            </header>
            {(instrumentPickerOpen || replaceTarget) && (
              <PluginPicker
                api={props.api}
                plugins={props.plugins}
                title={replaceTarget ? 'Replace Plugin' : 'Choose Instrument'}
                onSelect={(plugin) => {
                  if (replaceTarget) {
                    runOperation(props.onReplaceMissingPlugin(replaceTarget.deviceId, plugin.path));
                    setReplaceTarget(null);
                  } else {
                    setInstrument(plugin);
                    setInstrumentPickerOpen(false);
                  }
                }}
                onClose={() => {
                  setInstrumentPickerOpen(false);
                  setReplaceTarget(null);
                }}
              />
            )}
            <div className={styles.instrumentRow}>
              <div>
                <strong>{props.track.instrument?.name ?? 'None'}</strong>
                <small>
                  {props.track.instrument ? 'VST3 instrument' : 'No instrument selected'}
                </small>
              </div>
              {!instrumentUnavailable && (
                <button type="button" onClick={() => setInstrumentPickerOpen(true)}>
                  {props.track.instrument ? 'Change' : 'Choose'}
                </button>
              )}
            </div>
            {instrumentUnavailable && (
              <strong className={styles.warning}>
                {props.track.instrument?.disabledPlaceholder
                  ? 'DISABLED PLACEHOLDER'
                  : 'MISSING PLUGIN'}
              </strong>
            )}
            {props.track.instrument && instrumentUnavailable && (
              <div className={styles.actions}>
                <button onClick={() => runOperation(props.onRescanMissingPlugins())}>
                  Re-scan
                </button>
                <button onClick={() => replaceMissing(props.track.instrument!.id)}>Replace</button>
                {!props.track.instrument.disabledPlaceholder && (
                  <button
                    onClick={() =>
                      runOperation(props.onDisableMissingPlugin(props.track.instrument!.id))
                    }
                  >
                    Disable
                  </button>
                )}
              </div>
            )}
            {props.track.instrument && !instrumentUnavailable && (
              <div className={styles.actions}>
                <button
                  onClick={() =>
                    runOperation(
                      props.api.openTrackPluginEditor(props.track.id, props.track.instrument!.id),
                    )
                  }
                >
                  Edit
                </button>
                <button onClick={() => commit(props.api.clearTrackInstrument(props.track.id))}>
                  Clear
                </button>
              </div>
            )}
          </section>
        </>
      )}
      {props.track.kind === 'audio' && (
        <section className={styles.section}>
          <header className={styles.sectionHeader}>
            <strong>MONITORING</strong>
          </header>
          <div className={styles.segmented} role="group" aria-label="Monitoring">
            {(['off', 'auto', 'on'] as const).map((monitoring) => (
              <button
                type="button"
                key={monitoring}
                className={props.track.monitoring === monitoring ? styles.active : ''}
                aria-pressed={props.track.monitoring === monitoring}
                onClick={() => commit(props.api.updateTrack(props.track.id, { monitoring }))}
              >
                {monitoring === 'off' ? 'Off' : monitoring === 'auto' ? 'Auto' : 'On'}
              </button>
            ))}
          </div>
        </section>
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
      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>MIX</strong>
        </header>
        <label className={styles.field}>
          <span>Volume</span>
          <input
            className={styles.range}
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
        <label className={styles.field}>
          <span>Pan</span>
          <input
            className={styles.range}
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
      </section>
    </div>
  );
}
