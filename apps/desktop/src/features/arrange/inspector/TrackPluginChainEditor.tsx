import { useState } from 'react';
import clsx from 'clsx';
import type { ArrangementMutationResult, PluginEntry, Track } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { Icon } from '@/shared/ui/primitives';
import { PluginPicker } from './PluginPicker';
import styles from './Inspector.module.css';

interface TrackPluginChainEditorProps {
  track: Track;
  api: ArrangeInspectorApi;
  plugins: PluginEntry[];
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  missingDeviceIds: string[];
  onDisableMissingPlugin: (deviceId: string) => Promise<void>;
  onReplaceMissingPlugin: (deviceId: string, newPath: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
  runOperation: <T>(operation: Promise<T>, apply?: (value: T) => void) => void;
}

export function TrackPluginChainEditor(props: TrackPluginChainEditorProps) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const [replaceTarget, setReplaceTarget] = useState<string | null>(null);
  const addEffect = (plugin: PluginEntry) => {
    props.commit(props.api.addTrackEffect(props.track.id, plugin.path));
  };

  return (
    <>
      {(pickerOpen || replaceTarget) && (
        <PluginPicker
          api={props.api}
          plugins={props.plugins}
          title={replaceTarget ? 'Replace Plugin' : 'Add Effect'}
          onSelect={(plugin) => {
            if (replaceTarget) {
              props.runOperation(props.onReplaceMissingPlugin(replaceTarget, plugin.path));
              setReplaceTarget(null);
            } else {
              setPickerOpen(false);
              addEffect(plugin);
            }
          }}
          onClose={() => {
            setPickerOpen(false);
            setReplaceTarget(null);
          }}
        />
      )}
      <section className={styles.section} aria-label="Track effects">
        <header className={styles.sectionHeader}>
          <strong>EFFECTS</strong>
        </header>
        {props.track.rack.devices.length === 0 && <p className={styles.empty}>No effects</p>}
        {props.track.rack.devices.map((device, index) => {
          const unavailable =
            device.disabledPlaceholder || props.missingDeviceIds.includes(device.id);
          return (
            <div className={styles.device} key={device.id}>
              <div className={styles.deviceRow}>
                <span className={styles.deviceIcon}>
                  <Icon name="module" />
                </span>
                <div className={styles.deviceMeta}>
                  <strong>{device.name}</strong>
                  {device.bypassed && (
                    <span className={clsx(styles.badge, styles.muted)}>BYPASSED</span>
                  )}
                </div>
                {!unavailable && (
                  <div className={styles.deviceActions}>
                    <button
                      type="button"
                      className={clsx(styles.textButton, styles.plain)}
                      aria-pressed={device.bypassed}
                      onClick={() =>
                        props.commit(
                          props.api.setTrackDeviceBypassed(
                            props.track.id,
                            device.id,
                            !device.bypassed,
                          ),
                        )
                      }
                    >
                      {device.bypassed ? 'Enable' : 'Bypass'}
                    </button>
                    <button
                      type="button"
                      className={styles.iconButton}
                      aria-label={`Edit ${device.name}`}
                      onClick={() =>
                        props.runOperation(
                          props.api.openTrackPluginEditor(props.track.id, device.id),
                        )
                      }
                    >
                      <Icon name="pencil" />
                    </button>
                    <button
                      type="button"
                      className={styles.iconButton}
                      aria-label={`Move ${device.name} up`}
                      disabled={index === 0}
                      onClick={() => {
                        const ids = props.track.rack.devices.map((item) => item.id);
                        [ids[index - 1], ids[index]] = [ids[index], ids[index - 1]];
                        props.commit(props.api.reorderTrackEffects(props.track.id, ids));
                      }}
                    >
                      <Icon name="expand" />
                    </button>
                    <button
                      type="button"
                      className={styles.iconButton}
                      aria-label={`Move ${device.name} down`}
                      disabled={index + 1 === props.track.rack.devices.length}
                      onClick={() => {
                        const ids = props.track.rack.devices.map((item) => item.id);
                        [ids[index], ids[index + 1]] = [ids[index + 1], ids[index]];
                        props.commit(props.api.reorderTrackEffects(props.track.id, ids));
                      }}
                    >
                      <Icon name="collapse" />
                    </button>
                    <button
                      type="button"
                      className={clsx(styles.iconButton, styles.danger)}
                      aria-label={`Remove ${device.name}`}
                      onClick={() =>
                        props.commit(props.api.removeTrackEffect(props.track.id, device.id))
                      }
                    >
                      <Icon name="close" />
                    </button>
                  </div>
                )}
              </div>
              {unavailable && (
                <div className={styles.missingState}>
                  <strong>
                    {device.disabledPlaceholder ? 'DISABLED PLACEHOLDER' : 'MISSING PLUGIN'}
                  </strong>
                  <button
                    type="button"
                    className={styles.smallButton}
                    onClick={() => props.runOperation(props.onRescanMissingPlugins())}
                  >
                    Re-scan
                  </button>
                  <button
                    type="button"
                    className={styles.smallButton}
                    onClick={() => setReplaceTarget(device.id)}
                  >
                    Replace
                  </button>
                  {!device.disabledPlaceholder && (
                    <button
                      type="button"
                      className={styles.smallButton}
                      onClick={() => props.runOperation(props.onDisableMissingPlugin(device.id))}
                    >
                      Disable
                    </button>
                  )}
                </div>
              )}
            </div>
          );
        })}
        <button type="button" className={styles.addRow} onClick={() => setPickerOpen(true)}>
          <Icon name="plus" />
          Add Effect
        </button>
      </section>
    </>
  );
}
