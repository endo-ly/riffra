import { useEffect, useState } from 'react';
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
import { DeviceParameterEditor } from './DeviceParameterEditor';
import { DeviceChain } from './DeviceChain';
import { InstrumentMidiSettingsEditor } from './InstrumentMidiSettingsEditor';
import type { DeviceTarget } from './device-target';
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
  return (
    <div className={styles.panel} aria-label="Devices">
      <header className={styles.header}>
        <strong>DEVICES</strong>
        <span>{target?.name ?? 'Select a channel'}</span>
      </header>
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
        <p>Select a channel to edit its devices.</p>
      )}
      {track?.instrument && (
        <InstrumentMidiSettingsEditor
          settings={track.instrument.midi}
          recommendedRange={instrumentLibraryItem?.recommendedRange ?? null}
          warnings={midiInspection?.trackId === track.id ? midiInspection.warnings : []}
          onSave={(settings) => commit(props.api.setInstrumentMidiSettings(track.id, settings))}
        />
      )}
      {target && device && plugin && !unavailable && (
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
      )}
      {device && !plugin && (
        <p>Built-in Instrument · use Change, Clear or Bypass to edit this device.</p>
      )}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
