import { useEffect, useState, type CSSProperties } from 'react';
import type {
  Arrangement,
  ArrangementMutationResult,
  CanonicalState,
  AutomationLane,
  AutomationTarget,
  InstrumentLibraryItem,
  InstrumentMidiInspection,
  PluginEntry,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '../arrange-api';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
import { Toolbar } from '@/shared/ui/Toolbar';
import { resolveTrackColor } from '../model/track-colors';
import { DeviceParameterEditor } from './DeviceParameterEditor';
import { DeviceChain } from './DeviceChain';
import { InstrumentMidiSettingsEditor } from './InstrumentMidiSettingsEditor';
import type { DeviceTarget } from './device-target';
import controls from '@/shared/ui/controls.module.css';
import styles from './Devices.module.css';

interface DevicesPanelProps {
  projectId: string;
  arrangement: Arrangement;
  automationLanes: AutomationLane[];
  playing: boolean;
  currentTick: number;
  onOpenAutomation: (target: AutomationTarget) => void;
  target: DeviceTarget | null;
  api: ArrangeWorkspaceApi;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  instruments: InstrumentLibraryItem[];
  plugins: PluginEntry[];
  missingDeviceIds: string[];
  onDisableMissingPlugin: (id: string) => Promise<void>;
  onReplaceMissingPlugin: (id: string, path: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
}

export function DevicesPanel(props: DevicesPanelProps) {
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [midiInspection, setMidiInspection] = useState<InstrumentMidiInspection | null>(null);
  const { target } = props;
  const track = target?.track;
  const instrumentLibraryItem =
    track?.instrument?.source.type === 'internal'
      ? props.instruments.find((item) => {
          const resource = track.instrument?.source;
          if (resource?.type !== 'internal') return false;
          return resource.resource.type === 'builtInPreset'
            ? item.presetId === resource.resource.presetId
            : item.id === resource.resource.instrumentId;
        })
      : undefined;
  const device =
    track?.instrument?.id === selectedDeviceId
      ? track.instrument
      : target?.effects.find((item) => item.id === selectedDeviceId);
  useEffect(() => {
    if (!device) setSelectedDeviceId(null);
  }, [device]);
  const ownerKey = target ? JSON.stringify(target.owner) : '';
  useEffect(() => setSelectedDeviceId(null), [props.projectId, ownerKey]);
  const plugin =
    device &&
    ('plugin' in device ? device.plugin : device.source.type === 'vst3' ? device.source : null);
  const unavailable = Boolean(
    plugin && (plugin.disabledPlaceholder || props.missingDeviceIds.includes(device!.id)),
  );

  useEffect(() => {
    if (track?.kind !== 'instrument') {
      setMidiInspection(null);
      return;
    }
    let cancelled = false;
    void props.api
      .getInstrumentMidiSettings(track.id)
      .then((inspection) => {
        if (!cancelled) setMidiInspection(inspection);
      })
      .catch((failure: unknown) => {
        if (!cancelled) setError(failure instanceof Error ? failure.message : String(failure));
      });
    return () => {
      cancelled = true;
    };
  }, [props.api, props.arrangement.revision, track?.id, track?.kind]);

  const runOperation = (operation: Promise<unknown>) => {
    setError(null);
    void operation.catch((failure: unknown) =>
      setError(failure instanceof Error ? failure.message : String(failure)),
    );
  };
  const commit = (operation: Promise<ArrangementMutationResult>) =>
    runOperation(
      operation.then((result) => {
        applyArrangementMutation(result, props.applyCanonicalState, setError);
      }),
    );
  const color = track ? resolveTrackColor(track, props.arrangement.tracks.indexOf(track)) : null;
  const effectCount = target?.effects.length ?? 0;
  const deviceSummary = target
    ? [track?.instrument?.name, `${effectCount} effect${effectCount === 1 ? '' : 's'}`]
        .filter(Boolean)
        .join(' · ')
    : null;
  return (
    <div
      className={styles.panel}
      aria-label="Devices"
      style={color ? ({ '--track-color': color } as CSSProperties) : undefined}
    >
      <Toolbar label="Devices toolbar">
        <span className={styles.identity} title={target?.name}>
          {target?.name ?? 'Devices'}
          {deviceSummary && <small>{deviceSummary}</small>}
        </span>
      </Toolbar>
      <div className={styles.content}>
        {target ? (
          <DeviceChain
            {...props}
            target={target}
            selectedDeviceId={selectedDeviceId}
            onSelectDevice={setSelectedDeviceId}
            commit={commit}
            runOperation={runOperation}
          />
        ) : (
          <p className={controls.placeholder}>Select a channel to edit its devices.</p>
        )}
        {target && (
          <div className={styles.workbench} data-midi={Boolean(track?.instrument)}>
            {device && plugin && !unavailable ? (
              <DeviceParameterEditor
                key={`${props.projectId}:${JSON.stringify(target.owner)}:${device.id}:${plugin.path}`}
                api={props.api}
                automationLanes={props.automationLanes}
                automationReadActive={
                  props.playing &&
                  props.automationLanes.some(
                    (lane) =>
                      lane.target.kind === 'deviceParameter' && lane.target.deviceId === device.id,
                  )
                }
                currentTick={props.currentTick}
                onOpenAutomation={(parameter) => {
                  if (parameter.parameterId)
                    props.onOpenAutomation({
                      kind: 'deviceParameter',
                      deviceId: device.id,
                      parameterId: parameter.parameterId,
                    });
                }}
                owner={target.owner}
                deviceId={device.id}
                stateData={plugin.stateData}
                parameterValues={plugin.parameterValues}
                applyCanonicalState={props.applyCanonicalState}
                arrangement={props.arrangement}
                isInstrument={track?.instrument?.id === device.id}
                commit={commit}
                onOpenSidechainAutomation={(sidechainSendId) =>
                  props.onOpenAutomation({ kind: 'sidechainSendLevel', sidechainSendId })
                }
              />
            ) : (
              <p className={controls.placeholder}>
                {device && !plugin
                  ? 'Built-in Instrument · use Change, Clear or Bypass to edit this device.'
                  : 'Select a device to edit its parameters.'}
              </p>
            )}
            {track?.instrument && (
              <InstrumentMidiSettingsEditor
                settings={track.instrument.midi}
                recommendedRange={instrumentLibraryItem?.recommendedRange ?? null}
                warnings={midiInspection?.trackId === track.id ? midiInspection.warnings : []}
                onSave={(settings) =>
                  commit(props.api.setInstrumentMidiSettings(track.id, settings))
                }
              />
            )}
          </div>
        )}
        {error && (
          <p className={styles.error} role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
