import { useState } from 'react';
import type { CreativeSession, PluginEntry, Track } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { PluginPicker } from './PluginPicker';

interface TrackPluginChainEditorProps {
  track: Track;
  api: NativeApi;
  plugins: PluginEntry[];
  commit: (operation: Promise<CreativeSession>, message: string) => void;
  missingDeviceIds: string[];
  onDisableMissingPlugin: (deviceId: string) => Promise<void>;
  onReplaceMissingPlugin: (deviceId: string, newPath: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
  runOperation: <T>(
    operation: Promise<T>,
    successMessage: string,
    apply?: (value: T) => void,
  ) => void;
}

export function TrackPluginChainEditor(props: TrackPluginChainEditorProps) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const addEffect = (plugin: PluginEntry) => {
    props.commit(
      props.api.addTrackEffect(props.track.id, plugin.path),
      `Effect ${plugin.name} added.`,
    );
  };
  return (
    <>
      {pickerOpen && (
        <PluginPicker
          api={props.api}
          plugins={props.plugins}
          title="Add Effect"
          onSelect={(plugin) => {
            setPickerOpen(false);
            addEffect(plugin);
          }}
          onClose={() => setPickerOpen(false)}
        />
      )}
      <section aria-label="Track effects">
        <header>
          <strong>EFFECTS</strong>
        </header>
        {props.track.rack.devices.length === 0 && <p>No effects</p>}
        {props.track.rack.devices.map((device, index) => (
          <div key={device.id}>
            <strong>{device.name}</strong>
            {(device.disabledPlaceholder || props.missingDeviceIds.includes(device.id)) && (
              <>
                <strong>
                  {device.disabledPlaceholder ? 'DISABLED PLACEHOLDER' : 'MISSING PLUGIN'}
                </strong>
                <button
                  onClick={() =>
                    props.runOperation(props.onRescanMissingPlugins(), 'Plugin scan completed.')
                  }
                >
                  Re-scan
                </button>
                <button
                  onClick={() => {
                    const path = window.prompt('Replacement VST3 path', device.path ?? '')?.trim();
                    if (path)
                      props.runOperation(
                        props.onReplaceMissingPlugin(device.id, path),
                        'Missing Plugin replaced.',
                      );
                  }}
                >
                  Replace
                </button>
                {!device.disabledPlaceholder && (
                  <button
                    onClick={() =>
                      props.runOperation(
                        props.onDisableMissingPlugin(device.id),
                        'Missing Plugin disabled.',
                      )
                    }
                  >
                    Disable
                  </button>
                )}
              </>
            )}
            <button
              aria-pressed={device.bypassed}
              disabled={device.disabledPlaceholder || props.missingDeviceIds.includes(device.id)}
              onClick={() =>
                props.commit(
                  props.api.setTrackDeviceBypassed(props.track.id, device.id, !device.bypassed),
                  `${device.name} ${device.bypassed ? 'enabled' : 'bypassed'}.`,
                )
              }
            >
              {device.bypassed ? 'Enable' : 'Bypass'}
            </button>
            <button
              disabled={device.disabledPlaceholder || props.missingDeviceIds.includes(device.id)}
              onClick={() =>
                props.runOperation(
                  props.api.openTrackPluginEditor(props.track.id, device.id),
                  'Plugin Editor opened.',
                )
              }
            >
              Edit
            </button>
            <button
              disabled={index === 0}
              onClick={() => {
                const ids = props.track.rack.devices.map((item) => item.id);
                [ids[index - 1], ids[index]] = [ids[index], ids[index - 1]];
                props.commit(
                  props.api.reorderTrackEffects(props.track.id, ids),
                  'Effects reordered.',
                );
              }}
            >
              ↑
            </button>
            <button
              disabled={index + 1 === props.track.rack.devices.length}
              onClick={() => {
                const ids = props.track.rack.devices.map((item) => item.id);
                [ids[index], ids[index + 1]] = [ids[index + 1], ids[index]];
                props.commit(
                  props.api.reorderTrackEffects(props.track.id, ids),
                  'Effects reordered.',
                );
              }}
            >
              ↓
            </button>
            <button
              onClick={() =>
                props.commit(
                  props.api.removeTrackEffect(props.track.id, device.id),
                  `${device.name} removed.`,
                )
              }
            >
              Remove
            </button>
          </div>
        ))}
        <button onClick={() => setPickerOpen(true)}>＋ Add Effect</button>
      </section>
    </>
  );
}
