import type { AudioStatus, CreativeSession, PluginEntry } from '@/lib/domain';
import { Icon, Meter } from '../shared/ui';

export function WorkspacePlay({
  session,
  audio,
  plugins,
  missingPluginPaths,
  setSession,
  onTogglePluginBypass,
  onSetPluginParameter,
  onClearPlugin,
  onSaveRack,
  onLoadRack,
  onCaptureSnapshot,
  onRecallSnapshot,
}: {
  session: CreativeSession;
  audio: AudioStatus;
  plugins: PluginEntry[];
  missingPluginPaths: string[];
  setSession: (value: CreativeSession) => void;
  onTogglePluginBypass: (bypassed: boolean) => void;
  onSetPluginParameter: (index: number, value: number) => void;
  onClearPlugin: () => void;
  onSaveRack: () => void;
  onLoadRack: () => void;
  onCaptureSnapshot: (slot: 'A' | 'B') => void;
  onRecallSnapshot: (slot: 'A' | 'B') => void;
}) {
  const missingPaths = new Set(missingPluginPaths);
  const persistedPlugins = session.rack.devices
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
  const visiblePlugins = persistedPlugins.length ? persistedPlugins : plugins.slice(0, 3);
  const loadedBypassed =
    session.rack.devices.find((device) => device.kind === 'plugin')?.bypassed ?? false;
  const hasSnapshotA = session.snapshots.some((snapshot) => snapshot.id === 'snapshot:A');
  const hasSnapshotB = session.snapshots.some((snapshot) => snapshot.id === 'snapshot:B');
  const setMacro = (macroId: string, value: number) => {
    const safeValue = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
    const macro = session.rack.macros.find((item) => item.id === macroId);
    setSession({
      ...session,
      rack: {
        ...session.rack,
        macros: session.rack.macros.map((item) =>
          item.id === macroId ? { ...item, value: safeValue } : item,
        ),
      },
    });
    if (macro?.parameterIndex != null) onSetPluginParameter(macro.parameterIndex, safeValue);
  };
  const mapMacro = (macroId: string, value: string) =>
    setSession({
      ...session,
      rack: {
        ...session.rack,
        macros: session.rack.macros.map((item) =>
          item.id === macroId
            ? { ...item, parameterIndex: value === '' ? null : Number(value) }
            : item,
        ),
      },
    });
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
          <div className="device-face">
            <Meter value={45} />
            <Meter value={40} />
          </div>
          <h3>Input 1</h3>
          <small>Mono · −12.4 dB</small>
        </article>
        {(visiblePlugins.length
          ? visiblePlugins
          : [{ id: 'placeholder', name: 'Add a VST3', vendor: null } as PluginEntry]
        ).map((plugin, index) => (
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
              {plugin.scanState === 'quarantined'
                ? 'Missing dependency'
                : (plugin.vendor ?? 'VST3 discovered')}
            </small>
            <div className="device-controls">
              <button onClick={() => onTogglePluginBypass(!loadedBypassed)}>
                {loadedBypassed ? 'Enable' : 'Bypass'}
              </button>
              <button onClick={onClearPlugin}>Remove</button>
              <strong>0.0 dB</strong>
            </div>
          </article>
        ))}
        <button className="add-device">
          <Icon name="plus" />
          <span>Add Device</span>
        </button>
        <article className="rack-device output-device">
          <span className="device-order">OUT</span>
          <div className="device-face">
            <Meter value={58} />
            <Meter value={51} />
          </div>
          <h3>Main Out</h3>
          <small>Safety limited</small>
        </article>
      </section>
      <section className="section-card rack-library-actions">
        <header>
          <div>
            <span className="eyebrow">RACK LIBRARY</span>
            <h2>Reusable rack definition</h2>
          </div>
          <small>Asset-backed snapshots of the current rack</small>
        </header>
        <div className="button-row">
          <button className="text-button" onClick={onSaveRack}>
            Save Rack
          </button>
          <button className="text-button" onClick={onLoadRack}>
            Load Rack
          </button>
        </div>
      </section>
      {audio.plugin?.loaded && audio.plugin.parameters.length > 0 && (
        <section className="section-card plugin-parameters">
          <header>
            <div>
              <span className="eyebrow">COMMON PARAMETER VIEW</span>
              <h2>{audio.plugin.parameters.length} VST3 parameters</h2>
            </div>
            <small>Native GUI is optional; changes stay inside the isolated rack.</small>
          </header>
          <div className="plugin-parameter-grid">
            {audio.plugin.parameters.slice(0, 48).map((parameter) => (
              <label className="plugin-parameter" key={parameter.index}>
                <span>
                  <strong>{parameter.name || `Parameter ${parameter.index + 1}`}</strong>
                  <small>
                    {Math.round(parameter.value * 100)}%
                    {parameter.automatable ? ' · automatable' : ''}
                  </small>
                </span>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.001"
                  value={parameter.value}
                  onChange={(event) =>
                    onSetPluginParameter(parameter.index, Number(event.target.value))
                  }
                />
              </label>
            ))}
          </div>
          {audio.plugin.parameters.length > 48 && (
            <small className="inspector-copy">
              Showing first 48 parameters; the rest remain available to the plugin.
            </small>
          )}
        </section>
      )}
      <section className="macro-section">
        <header>
          <div>
            <span className="eyebrow">MACROS</span>
            <h2>Performance controls</h2>
          </div>
          <small>
            {session.rack.macros.filter((macro) => macro.parameterIndex != null).length} mapped
          </small>
        </header>
        <div className="macro-grid">
          {session.rack.macros.map((macro) => (
            <label className="macro" key={macro.id}>
              <span
                className="knob"
                style={{ '--turn': `${-120 + macro.value * 240}deg` } as React.CSSProperties}
              >
                <i />
              </span>
              <strong>{macro.name}</strong>
              <input
                type="range"
                min="0"
                max="1"
                step="0.001"
                value={macro.value}
                onChange={(event) => setMacro(macro.id, Number(event.target.value))}
              />
              <small>{Math.round(macro.value * 100)}%</small>
              <select
                aria-label={`${macro.name} target`}
                value={macro.parameterIndex == null ? '' : macro.parameterIndex}
                onChange={(event) => mapMacro(macro.id, event.target.value)}
              >
                <option value="">Unmapped</option>
                {audio.plugin?.parameters.map((parameter) => (
                  <option value={parameter.index} key={parameter.index}>
                    {parameter.name || `Parameter ${parameter.index + 1}`}
                  </option>
                ))}
              </select>
            </label>
          ))}
        </div>
      </section>
      <label className="session-note">
        <span>Session note</span>
        <textarea
          value={session.settings.note}
          onChange={(event) =>
            setSession({ ...session, settings: { ...session.settings, note: event.target.value } })
          }
          placeholder="意図、比較対象、使用場面を記録…"
        />
      </label>
    </div>
  );
}
