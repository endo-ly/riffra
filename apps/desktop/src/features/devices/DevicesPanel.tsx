import { useSessionCommand } from '@/session/hooks/useSessionCommand';
import { errorText } from '@/shared/errors';
import { useEffect, useState, type CSSProperties } from 'react';
import type { ArrangementMutationResult } from '@/model/generated';
import { useSession, useSessionContext } from '@/session/session-context';
import { useShallow } from 'zustand/react/shallow';
import { selectMissingDeviceIds } from '@/session/selectors';
import { selectionOwner } from '@/model/editor-selection';
import { resolveDeviceTarget } from '@/features/devices/model/device-target';
import { useInstrumentMidiInspection } from '@/session/hooks/useInstrumentMidiInspection';
import { Toolbar } from '@/shared/ui/Toolbar';
import { resolveTrackColorById } from '@/model/track-color';
import { DeviceParameterEditor } from './DeviceParameterEditor';
import { DeviceChain } from './DeviceChain';
import { InstrumentMidiSettingsEditor } from './InstrumentMidiSettingsEditor';
import controls from '@/shared/ui/controls.module.css';
import styles from './Devices.module.css';
import { InlineMessage } from '@/shared/ui/message/InlineMessage';

export function DevicesPanel() {
  const session = useSession((state) => state.project.boot!.canonical.session);
  const selection = useSession((state) => state.workspace.selection);
  const owner = selectionOwner(selection);
  const ownerKey = owner ? JSON.stringify(owner) : '';
  return <DevicesPanelContent key={`${session.sessionId}:${ownerKey}`} />;
}

function DevicesPanelContent() {
  const { api } = useSessionContext();
  const session = useSession((state) => state.project.boot!.canonical.session);
  const selection = useSession((state) => state.workspace.selection);
  const workspace = useSession((state) => state.workspace);
  const catalog = useSession((state) => state.catalog);
  const missing = useSession((state) => state.missing);
  const missingDeviceIds = useSession(useShallow(selectMissingDeviceIds));
  const owner = selectionOwner(selection);
  const target = owner ? resolveDeviceTarget(session.arrangement, owner) : null;
  const sessionCommit = useSessionCommand(JSON.stringify(target?.owner));
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const track = target?.track;
  const instrumentLibraryItem =
    track?.instrument?.source.type === 'internal'
      ? catalog.instruments.items.find((item) => {
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
  const plugin =
    device &&
    ('plugin' in device ? device.plugin : device.source.type === 'vst3' ? device.source : null);
  const unavailable = Boolean(
    plugin && (plugin.disabledPlaceholder || missingDeviceIds.includes(device!.id)),
  );

  const { inspection: midiInspection, error: inspectionError } = useInstrumentMidiInspection(
    track?.kind === 'instrument' ? track.id : null,
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
  const color = track ? resolveTrackColorById(session.arrangement.tracks, track.id) : null;
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
            target={target}
            instruments={catalog.instruments.items}
            plugins={catalog.plugins}
            missingDeviceIds={missingDeviceIds}
            selectedDeviceId={selectedDeviceId}
            onSelectDevice={setSelectedDeviceId}
            commit={commit}
            runOperation={runOperation}
            onDisableMissingPlugin={missing.disablePlugin}
            onReplaceMissingPlugin={missing.replacePlugin}
            onRescanMissingPlugins={missing.rescanPlugins}
          />
        ) : (
          <p className={controls.placeholder}>Select a channel to edit its devices.</p>
        )}
        {target && (
          <div className={styles.workbench} data-midi={Boolean(track?.instrument)}>
            {device && plugin && !unavailable ? (
              device.bypassed ? (
                <p className={controls.placeholder}>Bypassed · unbypass to edit this device.</p>
              ) : (
                <DeviceParameterEditor
                  key={`${session.sessionId}:${JSON.stringify(target.owner)}:${device.id}:${plugin.path}`}
                  automationLanes={session.arrangement.automationLanes}
                  onOpenAutomation={(parameter) => {
                    if (parameter.parameterId)
                      workspace.openAutomation({
                        kind: 'deviceParameter',
                        deviceId: device.id,
                        parameterId: parameter.parameterId,
                      });
                  }}
                  owner={target.owner}
                  deviceId={device.id}
                  pluginPath={plugin.path}
                  stateData={plugin.stateData}
                  parameterValues={plugin.parameterValues}
                  arrangement={session.arrangement}
                  isInstrument={track?.instrument?.id === device.id}
                  commit={commit}
                  onOpenSidechainAutomation={(sidechainSendId) =>
                    workspace.openAutomation({ kind: 'sidechainSendLevel', sidechainSendId })
                  }
                />
              )
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
        {error && <InlineMessage tone="danger">{error}</InlineMessage>}
      </div>
    </div>
  );
}
