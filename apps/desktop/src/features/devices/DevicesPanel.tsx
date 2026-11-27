import { useSessionCommand } from '@/session/hooks/useSessionCommand';
import { errorText } from '@/shared/errors';
import { useEffect, useState, type CSSProperties } from 'react';
import type { Arrangement, ArrangementMutationResult, EffectOwner } from '@/model/generated';
import { useSession, useSessionContext } from '@/session/session-context';
import { useShallow } from 'zustand/react/shallow';
import { selectMissingDeviceIds } from '@/session/selectors';
import { ownerSelection, selectionOwner, type EditorSelection } from '@/model/editor-selection';
import { resolveDeviceTarget } from '@/features/devices/model/device-target';
import { useInstrumentMidiInspection } from '@/session/hooks/useInstrumentMidiInspection';
import { resolveTrackColorById } from '@/model/track-color';
import { DeviceParameterEditor } from './DeviceParameterEditor';
import { DeviceChain } from './DeviceChain';
import { InstrumentMidiSettingsEditor } from './InstrumentMidiSettingsEditor';
import controls from '@/shared/ui/controls.module.css';
import styles from './Devices.module.css';
import { InlineMessage } from '@/shared/ui/message/InlineMessage';

function resolveOwnerFromSelection(
  arrangement: Arrangement,
  selection: EditorSelection,
): EffectOwner | null {
  const direct = selectionOwner(selection);
  if (direct) return direct;
  if (selection.kind === 'clips' && selection.clipIds.length > 0) {
    const clipId = selection.clipIds[0];
    const audioClip = arrangement.audioClips.find((clip) => clip.id === clipId);
    if (audioClip) return { kind: 'track', id: audioClip.trackId };
    const midiClip = arrangement.midiClips.find((clip) => clip.id === clipId);
    if (midiClip) return { kind: 'track', id: midiClip.trackId };
  }
  return null;
}

function encodeOwner(owner: EffectOwner): string {
  return owner.kind === 'master' ? 'master' : `${owner.kind}:${owner.id}`;
}

function decodeOwner(value: string): EffectOwner {
  if (value === 'master') return { kind: 'master' };
  const [kind, ...rest] = value.split(':');
  return { kind: kind === 'bus' ? 'bus' : 'track', id: rest.join(':') };
}

export function DevicesPanel() {
  const session = useSession((state) => state.project.boot!.canonical.session);
  const selection = useSession((state) => state.workspace.selection);
  const owner = resolveOwnerFromSelection(session.arrangement, selection);
  const ownerKey = owner ? encodeOwner(owner) : '';
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
  const owner = resolveOwnerFromSelection(session.arrangement, selection);
  const target = owner ? resolveDeviceTarget(session.arrangement, owner) : null;
  const sessionCommit = useSessionCommand(JSON.stringify(target?.owner));
  const track = target?.track;
  const defaultDeviceId = track?.instrument?.id ?? target?.effects[0]?.id ?? null;
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(defaultDeviceId);
  const [error, setError] = useState<string | null>(null);
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
  const effectiveSelectedDeviceId =
    selectedDeviceId &&
    (track?.instrument?.id === selectedDeviceId ||
      target?.effects.some((item) => item.id === selectedDeviceId))
      ? selectedDeviceId
      : defaultDeviceId;
  const device =
    track?.instrument?.id === effectiveSelectedDeviceId
      ? track.instrument
      : target?.effects.find((item) => item.id === effectiveSelectedDeviceId);
  const isInstrumentSelected = Boolean(track?.instrument && device?.id === track.instrument.id);
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
  return (
    <div
      className={styles.panel}
      aria-label="Devices"
      style={color ? ({ '--track-color': color } as CSSProperties) : undefined}
    >
      <div className={styles.content}>
        <div className={styles.channelBar}>
          <span className={styles.channelDot} aria-hidden="true" />
          <select
            className={`${controls.control} ${styles.channelSelect}`}
            aria-label="Devices channel"
            value={owner ? encodeOwner(owner) : ''}
            onChange={(event) => {
              if (!event.currentTarget.value) return;
              workspace.setSelection(ownerSelection(decodeOwner(event.currentTarget.value)));
            }}
          >
            {!owner && (
              <option value="" disabled>
                Select a channel…
              </option>
            )}
            {session.arrangement.tracks.length > 0 && (
              <optgroup label="Tracks">
                {session.arrangement.tracks.map((item) => (
                  <option key={item.id} value={encodeOwner({ kind: 'track', id: item.id })}>
                    {item.name}
                  </option>
                ))}
              </optgroup>
            )}
            {session.arrangement.mixer.buses.length > 0 && (
              <optgroup label="Buses">
                {session.arrangement.mixer.buses.map((bus) => (
                  <option key={bus.id} value={encodeOwner({ kind: 'bus', id: bus.id })}>
                    {bus.name}
                  </option>
                ))}
              </optgroup>
            )}
            <option value="master">Master</option>
          </select>
        </div>
        {target ? (
          <DeviceChain
            target={target}
            instruments={catalog.instruments.items}
            plugins={catalog.plugins}
            missingDeviceIds={missingDeviceIds}
            selectedDeviceId={effectiveSelectedDeviceId}
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
          <div className={styles.workbench} data-midi={isInstrumentSelected}>
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
                  isInstrument={isInstrumentSelected}
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
                  : 'Add an instrument or effect to edit its parameters.'}
              </p>
            )}
            {isInstrumentSelected && track?.instrument && (
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
