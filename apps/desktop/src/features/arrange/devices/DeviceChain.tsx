import { Fragment, useState } from 'react';
import type { ArrangementMutationResult, InstrumentLibraryItem, PluginEntry } from '@/model/domain';
import { Icon } from '@/shared/ui/primitives';
import controls from '@/shared/ui/controls.module.css';
import { useSessionContext } from '@/shared/session/session-context';
import { InstrumentPicker } from './InstrumentPicker';
import { PluginPicker } from './PluginPicker';
import type { DeviceTarget } from './device-target';
import styles from './Devices.module.css';

interface DeviceChainProps {
  target: DeviceTarget;
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

/** Arrow between two stages of the signal flow. */
function FlowArrow() {
  return (
    <span className={styles.flow} aria-hidden="true">
      <Icon name="chevron" />
    </span>
  );
}

export function DeviceChain(props: DeviceChainProps) {
  const { target } = props;
  const { api } = useSessionContext();
  const { track, owner, effects } = target;
  const [picker, setPicker] = useState<'instrument' | 'effect' | null>(null);
  const [replaceTarget, setReplaceTarget] = useState<{
    id: string;
    role: 'instrument' | 'effect';
  } | null>(null);
  const devices = [
    ...(track?.instrument
      ? [
          {
            ...track?.instrument,
            role: 'instrument' as const,
            plugin: track?.instrument.source.type === 'vst3' ? track?.instrument.source : null,
          },
        ]
      : []),
    ...effects.map((device) => ({ ...device, role: 'effect' as const })),
  ];
  const reorder = (id: string, direction: number) => {
    const ids = effects.map((device) => device.id);
    const index = ids.indexOf(id);
    [ids[index], ids[index + direction]] = [ids[index + direction], ids[index]];
    props.commit(api.reorderEffects(owner, ids));
  };
  return (
    <>
      {picker === 'instrument' && track && (
        <InstrumentPicker
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
          plugins={props.plugins}
          role={replaceTarget?.role ?? 'effect'}
          title={replaceTarget ? 'Replace Plugin' : 'Add Effect'}
          onSelect={(plugin) => {
            if (replaceTarget)
              props.runOperation(props.onReplaceMissingPlugin(replaceTarget.id, plugin.path));
            else props.commit(api.addEffect(owner, plugin.path));
            setReplaceTarget(null);
            setPicker(null);
          }}
          onClose={() => {
            setReplaceTarget(null);
            setPicker(null);
          }}
        />
      )}
      <div className={styles.chain} aria-label="Channel device chain">
        {track?.kind === 'audio' && <div className={styles.input}>Audio Input →</div>}
        {track?.kind === 'instrument' && !track?.instrument && (
          <>
            <button
              type="button"
              className={styles.addCard}
              onClick={() => setPicker('instrument')}
            >
              <Icon name="keys" />
              Choose Instrument
            </button>
            <FlowArrow />
          </>
        )}
        {devices.map((device, index) => {
          const unavailable = Boolean(
            device.plugin &&
            (device.plugin.disabledPlaceholder || props.missingDeviceIds.includes(device.id)),
          );
          const effectIndex = effects.findIndex((effect) => effect.id === device.id);
          const selected = props.selectedDeviceId === device.id;
          return (
            <Fragment key={device.id}>
              {index > 0 && <FlowArrow />}
              <div
                className={styles.card}
                data-selected={selected}
                data-bypassed={device.bypassed}
                data-unavailable={unavailable}
              >
                <span className={styles.cardIcon}>
                  <Icon name={device.role === 'instrument' ? 'keys' : 'module'} />
                </span>
                <span className={styles.cardText}>
                  <button
                    type="button"
                    className={styles.deviceName}
                    aria-pressed={selected}
                    title={device.name}
                    onClick={() => props.onSelectDevice(device.id)}
                  >
                    {device.name}
                  </button>
                  <small>
                    {device.role === 'instrument' ? 'INSTRUMENT' : 'EFFECT'}
                    {device.bypassed && ' · BYPASSED'}
                  </small>
                </span>
                {unavailable ? (
                  <div className={styles.recovery}>
                    <strong>
                      {device.plugin?.disabledPlaceholder
                        ? 'DISABLED PLACEHOLDER'
                        : 'MISSING PLUGIN'}
                    </strong>
                    <button
                      type="button"
                      className={controls.smallButton}
                      onClick={() => props.runOperation(props.onRescanMissingPlugins())}
                    >
                      Re-scan
                    </button>
                    <button
                      type="button"
                      className={controls.smallButton}
                      onClick={() => setReplaceTarget({ id: device.id, role: device.role })}
                    >
                      Replace
                    </button>
                    {!device.plugin?.disabledPlaceholder && (
                      <button
                        type="button"
                        className={controls.smallButton}
                        onClick={() => props.runOperation(props.onDisableMissingPlugin(device.id))}
                      >
                        Disable
                      </button>
                    )}
                  </div>
                ) : null}
                <div className={styles.cardActions}>
                  {!unavailable && (
                    <>
                      <button
                        type="button"
                        className={styles.bypass}
                        aria-pressed={device.bypassed}
                        onClick={() =>
                          props.commit(api.setDeviceBypassed(owner, device.id, !device.bypassed))
                        }
                      >
                        Bypass
                      </button>
                      {device.role === 'instrument' && (
                        <button
                          type="button"
                          className={controls.smallButton}
                          onClick={() => setPicker('instrument')}
                        >
                          Change
                        </button>
                      )}
                    </>
                  )}
                  <span className={styles.cardTools}>
                    {device.role === 'effect' && (
                      <>
                        <button
                          type="button"
                          className={controls.iconButton}
                          aria-label={`Move ${device.name} left`}
                          title="Move left"
                          disabled={effectIndex === 0}
                          onClick={() => reorder(device.id, -1)}
                        >
                          <Icon name="chevron" className={styles.flipped} />
                        </button>
                        <button
                          type="button"
                          className={controls.iconButton}
                          aria-label={`Move ${device.name} right`}
                          title="Move right"
                          disabled={effectIndex + 1 === effects.length}
                          onClick={() => reorder(device.id, 1)}
                        >
                          <Icon name="chevron" />
                        </button>
                      </>
                    )}
                    <button
                      type="button"
                      className={`${controls.iconButton} ${controls.danger}`}
                      aria-label={
                        device.role === 'instrument' ? 'Clear instrument' : `Remove ${device.name}`
                      }
                      title={device.role === 'instrument' ? 'Clear' : 'Remove'}
                      onClick={() =>
                        props.commit(
                          device.role === 'instrument'
                            ? api.clearTrackInstrument(track!.id)
                            : api.removeEffect(owner, device.id),
                        )
                      }
                    >
                      <Icon name="trash" />
                    </button>
                  </span>
                </div>
              </div>
            </Fragment>
          );
        })}
        {devices.length > 0 && <FlowArrow />}
        <button type="button" className={styles.addCard} onClick={() => setPicker('effect')}>
          <Icon name="plus" />
          Add Effect
        </button>
      </div>
    </>
  );
}
