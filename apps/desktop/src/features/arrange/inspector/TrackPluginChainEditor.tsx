import { useState } from 'react';
import type { ArrangementMutationResult, PluginEntry, Track } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { PluginPicker } from './PluginPicker';
import styles from './TrackPluginChainEditor.module.css';

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
        {props.track.rack.devices.map((device, index) => (
          <div className={styles.device} key={device.id}>
            <div className={styles.deviceHeading}>
              <strong>{device.name}</strong>
              {device.bypassed && <span>BYPASSED</span>}
            </div>
            {(device.disabledPlaceholder || props.missingDeviceIds.includes(device.id)) && (
              <div className={styles.missingState}>
                <strong>
                  {device.disabledPlaceholder ? 'DISABLED PLACEHOLDER' : 'MISSING PLUGIN'}
                </strong>
                <div className={styles.actions}>
                  <button
                    type="button"
                    onClick={() => props.runOperation(props.onRescanMissingPlugins())}
                  >
                    Re-scan
                  </button>
                  <button type="button" onClick={() => setReplaceTarget(device.id)}>
                    Replace
                  </button>
                  {!device.disabledPlaceholder && (
                    <button
                      type="button"
                      onClick={() => props.runOperation(props.onDisableMissingPlugin(device.id))}
                    >
                      Disable
                    </button>
                  )}
                </div>
              </div>
            )}
            <div className={styles.controls}>
              <button
                type="button"
                aria-pressed={device.bypassed}
                disabled={device.disabledPlaceholder || props.missingDeviceIds.includes(device.id)}
                onClick={() =>
                  props.commit(
                    props.api.setTrackDeviceBypassed(props.track.id, device.id, !device.bypassed),
                  )
                }
              >
                {device.bypassed ? 'Enable' : 'Bypass'}
              </button>
              <button
                type="button"
                disabled={device.disabledPlaceholder || props.missingDeviceIds.includes(device.id)}
                onClick={() =>
                  props.runOperation(props.api.openTrackPluginEditor(props.track.id, device.id))
                }
              >
                Edit
              </button>
              <button
                type="button"
                aria-label={`Move ${device.name} up`}
                disabled={index === 0}
                onClick={() => {
                  const ids = props.track.rack.devices.map((item) => item.id);
                  [ids[index - 1], ids[index]] = [ids[index], ids[index - 1]];
                  props.commit(props.api.reorderTrackEffects(props.track.id, ids));
                }}
              >
                ↑
              </button>
              <button
                type="button"
                aria-label={`Move ${device.name} down`}
                disabled={index + 1 === props.track.rack.devices.length}
                onClick={() => {
                  const ids = props.track.rack.devices.map((item) => item.id);
                  [ids[index], ids[index + 1]] = [ids[index + 1], ids[index]];
                  props.commit(props.api.reorderTrackEffects(props.track.id, ids));
                }}
              >
                ↓
              </button>
              <button
                type="button"
                className={styles.remove}
                onClick={() => props.commit(props.api.removeTrackEffect(props.track.id, device.id))}
              >
                Remove
              </button>
            </div>
          </div>
        ))}
        <button type="button" className={styles.addButton} onClick={() => setPickerOpen(true)}>
          ＋ Add Effect
        </button>
      </section>
    </>
  );
}
