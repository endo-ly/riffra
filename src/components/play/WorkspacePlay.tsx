import type { AudioStatus, CreativeSession, PluginEntry } from '@/lib/domain';
import { Meter } from '../shared/ui';

export function WorkspacePlay({
  session,
  audio,
  missingPluginPaths,
  onTogglePluginBypass,
  onClearPlugin,
  onCaptureSnapshot,
  onRecallSnapshot,
}: {
  session: CreativeSession;
  audio: AudioStatus;
  missingPluginPaths: string[];
  onTogglePluginBypass: (bypassed: boolean) => void;
  onClearPlugin: () => void;
  onCaptureSnapshot: (slot: 'A' | 'B') => void;
  onRecallSnapshot: (slot: 'A' | 'B') => void;
}) {
  const inputChannel = audio.inputChannels.find((channel) => channel.index === audio.inputChannel);
  const inputDb = audio.inputPeak > 0 ? 20 * Math.log10(audio.inputPeak) : -90;
  const missingPaths = new Set(missingPluginPaths);
  const loadedPlugins = session.rack.devices
    .filter((device) => device.kind === 'plugin')
    .map(
      (device) =>
        ({
          id: device.id,
          name: device.name,
          vendor: null,
          version: null,
          format: 'VST3',
          path: device.path ?? '',
          bundle: true,
          modifiedAtMs: null,
          scanState: device.path && missingPaths.has(device.path) ? 'quarantined' : 'validated',
        }) as PluginEntry,
    );
  const loadedBypassed =
    session.rack.devices.find((device) => device.kind === 'plugin')?.bypassed ?? false;
  const hasSnapshotA = session.snapshots.some((snapshot) => snapshot.id === 'snapshot:A');
  const hasSnapshotB = session.snapshots.some((snapshot) => snapshot.id === 'snapshot:B');
  return (
    <div className="workspace-scroll play-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">LIVE SIGNAL</span>
          <h1>Input → Tone → Output</h1>
        </div>
        <div className="snapshot-tabs">
          <button className={hasSnapshotA ? 'active' : ''} onClick={() => onRecallSnapshot('A')}>
            A
          </button>
          <button className={hasSnapshotB ? 'active' : ''} onClick={() => onRecallSnapshot('B')}>
            B
          </button>
          <button onClick={() => onCaptureSnapshot(hasSnapshotA ? 'B' : 'A')}>＋</button>
        </div>
      </section>
      <div className="signal-line" />
      <section className="rack-flow">
        <article className="rack-device input-device">
          <span className="device-order">IN</span>
          <div className="device-face live-meter-face">
            <span className="meter-label">INPUT LEVEL</span>
            <Meter value={Math.round(audio.inputPeak * 100)} />
          </div>
          <h3>{inputChannel?.name ?? 'No input channel'}</h3>
          <small>{inputDb.toFixed(1)} dBFS</small>
        </article>
        {loadedPlugins.map((plugin, index) => (
          <article
            className={`rack-device ${plugin.scanState === 'quarantined' ? 'missing-dependency' : ''}`}
            key={plugin.id}
          >
            <span className="device-order">{String(index + 1).padStart(2, '0')}</span>
            <div className={`device-face face-${index}`}>
              <span>{plugin.name.slice(0, 2).toUpperCase()}</span>
              <i />
            </div>
            <h3>{plugin.name}</h3>
            <small>
              {plugin.scanState === 'quarantined' ? 'Missing dependency' : 'Loaded in rack'}
            </small>
            <div className="device-controls">
              <button onClick={() => onTogglePluginBypass(!loadedBypassed)}>
                {loadedBypassed ? 'Enable' : 'Bypass'}
              </button>
              <button onClick={onClearPlugin}>Remove</button>
            </div>
          </article>
        ))}
        {loadedPlugins.length === 0 && (
          <article className="rack-device rack-empty">
            <span className="device-order">01</span>
            <div className="device-face">
              <span>—</span>
            </div>
            <h3>No plugin loaded</h3>
            <small>Pick a VST3 from the Library to add it to the rack.</small>
          </article>
        )}
        <article className="rack-device output-device">
          <span className="device-order">OUT</span>
          <div className="device-face">
            <Meter value={Math.round(audio.outputPeak * 100)} />
          </div>
          <h3>Output</h3>
          <small>
            {audio.outputDevice ?? 'No output device'}
            {audio.outputChannels.length > 0
              ? ` · ${audio.outputChannels
                  .slice(0, 2)
                  .map((channel) => channel.name)
                  .join(' + ')}`
              : ''}
          </small>
        </article>
      </section>
    </div>
  );
}
