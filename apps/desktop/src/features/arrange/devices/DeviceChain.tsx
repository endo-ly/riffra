import { useState } from 'react';
import type {
  ArrangementMutationResult,
  InstrumentLibraryItem,
  PluginEntry,
  Track,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '../arrange-api';
import { InstrumentPicker } from './InstrumentPicker';
import { PluginPicker } from './PluginPicker';
import styles from './Devices.module.css';

interface DeviceChainProps {
  track: Track;
  api: ArrangeWorkspaceApi;
  instruments: InstrumentLibraryItem[];
  plugins: PluginEntry[];
  missingDeviceIds: string[];
  selectedDeviceId: string | null;
  onSelectDevice: (id: string) => void;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  runOperation: (operation: Promise<unknown>) => void;
  onDisableMissingPlugin: (id: string) => Promise<void>;
  onReplaceMissingPlugin: (id: string, path: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
}

export function DeviceChain(props: DeviceChainProps) {
  const { track, api } = props;
  const [picker, setPicker] = useState<'instrument' | 'effect' | null>(null);
  const [replaceTarget, setReplaceTarget] = useState<{
    id: string;
    role: 'instrument' | 'effect';
  } | null>(null);
  const devices = [
    ...(track.instrument
      ? [
          {
            ...track.instrument,
            role: 'instrument' as const,
            plugin: track.instrument.source.type === 'vst3' ? track.instrument.source : null,
          },
        ]
      : []),
    ...track.effects.map((device) => ({ ...device, role: 'effect' as const })),
  ];
  const reorder = (id: string, direction: number) => {
    const ids = track.effects.map((device) => device.id);
    const index = ids.indexOf(id);
    [ids[index], ids[index + direction]] = [ids[index + direction], ids[index]];
    props.commit(api.reorderTrackEffects(track.id, ids));
  };
  return (
    <>
      {picker === 'instrument' && (
        <InstrumentPicker
          api={api}
          instruments={props.instruments}
          plugins={props.plugins}
          onSelectInstrument={(id) => {
            props.commit(api.applyInstrument(track.id, id));
            setPicker(null);
          }}
          onSelectVst3={(plugin) => {
            props.commit(api.setTrackVst3Instrument(track.id, plugin.path));
            setPicker(null);
          }}
          onClose={() => setPicker(null)}
        />
      )}
      {(picker === 'effect' || replaceTarget) && (
        <PluginPicker
          api={api}
          plugins={props.plugins}
          role={replaceTarget?.role ?? 'effect'}
          title={replaceTarget ? 'Replace Plugin' : 'Add Effect'}
          onSelect={(plugin) => {
            if (replaceTarget)
              props.runOperation(props.onReplaceMissingPlugin(replaceTarget.id, plugin.path));
            else props.commit(api.addTrackEffect(track.id, plugin.path));
            setReplaceTarget(null);
            setPicker(null);
          }}
          onClose={() => {
            setReplaceTarget(null);
            setPicker(null);
          }}
        />
      )}
      <div className={styles.chain} aria-label="Track device chain">
        {track.kind === 'audio' && <div className={styles.input}>Audio Input →</div>}
        {track.kind === 'instrument' && !track.instrument && (
          <button type="button" onClick={() => setPicker('instrument')}>
            Choose Instrument
          </button>
        )}
        {devices.map((device) => {
          const unavailable = Boolean(
            device.plugin &&
            (device.plugin.disabledPlaceholder || props.missingDeviceIds.includes(device.id)),
          );
          const effectIndex = track.effects.findIndex((effect) => effect.id === device.id);
          return (
            <div
              key={device.id}
              className={styles.card}
              data-selected={props.selectedDeviceId === device.id}
            >
              <button
                type="button"
                className={styles.deviceName}
                aria-pressed={props.selectedDeviceId === device.id}
                onClick={() => props.onSelectDevice(device.id)}
              >
                {device.name}
              </button>
              <small>
                {device.role === 'instrument' ? 'INSTRUMENT' : 'EFFECT'}
                {device.bypassed && ' · BYPASSED'}
              </small>
              {unavailable ? (
                <div className={styles.recovery}>
                  <strong>
                    {device.plugin?.disabledPlaceholder ? 'DISABLED PLACEHOLDER' : 'MISSING PLUGIN'}
                  </strong>
                  <button
                    type="button"
                    onClick={() => props.runOperation(props.onRescanMissingPlugins())}
                  >
                    Re-scan
                  </button>
                  <button
                    type="button"
                    onClick={() => setReplaceTarget({ id: device.id, role: device.role })}
                  >
                    Replace
                  </button>
                  {!device.plugin?.disabledPlaceholder && (
                    <button
                      type="button"
                      onClick={() => props.runOperation(props.onDisableMissingPlugin(device.id))}
                    >
                      Disable
                    </button>
                  )}
                </div>
              ) : (
                <div className={styles.actions}>
                  <button
                    type="button"
                    className={styles.toggle}
                    aria-pressed={device.bypassed}
                    onClick={() =>
                      props.commit(
                        api.setTrackDeviceBypassed(track.id, device.id, !device.bypassed),
                      )
                    }
                  >
                    Bypass
                  </button>
                  {device.role === 'instrument' && (
                    <button type="button" onClick={() => setPicker('instrument')}>
                      Change
                    </button>
                  )}
                </div>
              )}
              <div className={styles.actions}>
                {device.role === 'effect' && (
                  <>
                    <button
                      type="button"
                      aria-label={`Move ${device.name} left`}
                      disabled={effectIndex === 0}
                      onClick={() => reorder(device.id, -1)}
                    >
                      ←
                    </button>
                    <button
                      type="button"
                      aria-label={`Move ${device.name} right`}
                      disabled={effectIndex + 1 === track.effects.length}
                      onClick={() => reorder(device.id, 1)}
                    >
                      →
                    </button>
                  </>
                )}
                <button
                  type="button"
                  className={styles.danger}
                  aria-label={
                    device.role === 'instrument' ? 'Clear instrument' : `Remove ${device.name}`
                  }
                  onClick={() =>
                    props.commit(
                      device.role === 'instrument'
                        ? api.clearTrackInstrument(track.id)
                        : api.removeTrackEffect(track.id, device.id),
                    )
                  }
                >
                  {device.role === 'instrument' ? 'Clear' : 'Remove'}
                </button>
              </div>
            </div>
          );
        })}
        <button type="button" onClick={() => setPicker('effect')}>
          + Effect
        </button>
      </div>
    </>
  );
}
