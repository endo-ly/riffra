import { errorText } from '@/shared/errors';
import { useEffect, useState, type CSSProperties } from 'react';
import type {
  Arrangement,
  ArrangementMutationResult,
  AutomationLane,
  AutomationTarget,
  InstrumentLibraryItem,
  PluginEntry,
} from '@/model/generated';
import { useSession, useSessionContext } from '@/shared/session/session-context';
import { useShallow } from 'zustand/react/shallow';
import { selectMissingDeviceIds } from '@/shared/session/selectors';
import { selectionOwner } from '@/model/editor-selection';
import { resolveDeviceTarget } from './device-target';
import { useInstrumentMidiInspection } from '@/shared/session/useInstrumentMidiInspection';
import { Toolbar } from '@/shared/ui/Toolbar';
import { resolveTrackColor } from '@/model/track-color';
import { DeviceParameterEditor } from './DeviceParameterEditor';
import { DeviceChain } from './DeviceChain';
import { InstrumentMidiSettingsEditor } from './InstrumentMidiSettingsEditor';
import type { DeviceTarget } from './device-target';
import controls from '@/shared/ui/controls.module.css';
import styles from './Devices.module.css';

interface DevicesPanelProps {
  sessionId: string;
  arrangement: Arrangement;
  automationLanes: AutomationLane[];
  playing: boolean;
  currentTick: number;
  onOpenAutomation: (target: AutomationTarget) => void;
  target: DeviceTarget | null;
  instruments: InstrumentLibraryItem[];
  plugins: PluginEntry[];
  missingDeviceIds: string[];
  onDisableMissingPlugin: (id: string) => Promise<void>;
  onReplaceMissingPlugin: (id: string, path: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
}

export function DevicesPanel() {
  const session = useSession((state) => state.project.boot!.canonical.session);
  const selection = useSession((state) => state.workspace.selection);
  const workspace = useSession((state) => state.workspace);
  const transport = useSession((state) => state.transport);
  const catalog = useSession((state) => state.catalog);
  const missing = useSession((state) => state.missing);
  const missingDeviceIds = useSession(useShallow(selectMissingDeviceIds));
  const owner = selectionOwner(selection);
  const target = owner ? resolveDeviceTarget(session.arrangement, owner) : null;
  return (
    <DevicesPanelView
      sessionId={session.sessionId}
      arrangement={session.arrangement}
      automationLanes={session.arrangement.automationLanes}
      playing={transport.status?.state === 'playing'}
      currentTick={transport.displayTick}
      onOpenAutomation={workspace.openAutomation}
      target={target}
      instruments={catalog.instruments.items}
      plugins={catalog.plugins}
      missingDeviceIds={missingDeviceIds}
      onDisableMissingPlugin={missing.disablePlugin}
      onReplaceMissingPlugin={missing.replacePlugin}
      onRescanMissingPlugins={missing.rescanPlugins}
    />
  );
}

export function DevicesPanelView(props: DevicesPanelProps) {
  const { api, commit: sessionCommit } = useSessionContext();
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
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
  useEffect(() => setSelectedDeviceId(null), [props.sessionId, ownerKey]);
  const plugin =
    device &&
    ('plugin' in device ? device.plugin : device.source.type === 'vst3' ? device.source : null);
  const unavailable = Boolean(
    plugin && (plugin.disabledPlaceholder || props.missingDeviceIds.includes(device!.id)),
  );

  const { inspection: midiInspection, error: inspectionError } = useInstrumentMidiInspection(
    track?.kind === 'instrument' ? track.id : null,
    props.arrangement.revision,
  );
  useEffect(() => {
    if (inspectionError) setError(inspectionError);
  }, [inspectionError]);

  const runOperation = (operation: Promise<unknown>) => {
    setError(null);
    void operation.catch((failure: unknown) => setError(errorText(failure)));
  };
  const commit = (operation: Promise<ArrangementMutationResult>) => {
    setError(null);
    void sessionCommit(operation, { onError: setError });
  };
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
                key={`${props.sessionId}:${JSON.stringify(target.owner)}:${device.id}:${plugin.path}`}
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
                onSave={(settings) => commit(api.setInstrumentMidiSettings(track.id, settings))}
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
