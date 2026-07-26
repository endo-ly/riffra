import type { CreativeSession, Track } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

interface TrackPluginChainEditorProps {
  track: Track;
  api: NativeApi;
  commit: (operation: Promise<CreativeSession>, message: string) => void;
  missingDeviceIds: string[];
  onDisableMissingPlugin: (deviceId: string) => Promise<void>;
  onReplaceMissingPlugin: (deviceId: string, newPath: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
}

export function TrackPluginChainEditor(props: TrackPluginChainEditorProps) {
  const addEffect = () => {
    const path = window.prompt('VST3 effect path')?.trim();
    if (path) props.commit(props.api.addTrackEffect(props.track.id, path), 'Effect added.');
  };
  return (
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
              <button onClick={() => void props.onRescanMissingPlugins()}>Re-scan</button>
              <button
                onClick={() => {
                  const path = window.prompt('Replacement VST3 path', device.path ?? '')?.trim();
                  if (path) void props.onReplaceMissingPlugin(device.id, path);
                }}
              >
                Replace
              </button>
              {!device.disabledPlaceholder && (
                <button onClick={() => void props.onDisableMissingPlugin(device.id)}>
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
            onClick={() => void props.api.openTrackPluginEditor(props.track.id, device.id)}
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
      <button onClick={addEffect}>＋ Add Effect</button>
    </section>
  );
}
