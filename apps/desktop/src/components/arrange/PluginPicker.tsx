import { useEffect, useMemo, useState } from 'react';
import type { PluginEntry } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import styles from './WorkspaceArrange.module.css';

interface PluginPickerProps {
  api: NativeApi;
  title?: string;
  plugins?: PluginEntry[];
  onSelect: (plugin: PluginEntry) => void;
  onClose: () => void;
}

export function PluginPicker(props: PluginPickerProps) {
  const [scannedPlugins, setScannedPlugins] = useState<PluginEntry[]>([]);
  const [loading, setLoading] = useState(!props.plugins);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');

  useEffect(() => {
    if (props.plugins) {
      setScannedPlugins([]);
      setLoading(false);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    props.api
      .scanVst3Folder()
      .then((report) => {
        if (cancelled) return;
        setScannedPlugins(report.plugins);
        setError(null);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [props.api, props.plugins]);

  const plugins = props.plugins ?? scannedPlugins;

  const filtered = useMemo(() => {
    const trimmed = query.trim().toLowerCase();
    if (!trimmed) return plugins;
    return plugins.filter(
      (plugin) =>
        plugin.name.toLowerCase().includes(trimmed) ||
        (plugin.vendor ?? '').toLowerCase().includes(trimmed),
    );
  }, [plugins, query]);

  return (
    <div
      className={styles.pluginPickerOverlay}
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        className={styles.pluginPicker}
        role="dialog"
        aria-label={props.title ?? 'Plugin Picker'}
      >
        <header>
          <strong>{props.title ?? 'Choose Plugin'}</strong>
          <button
            className={styles.pluginPickerClose}
            aria-label="Close"
            onClick={props.onClose}
            type="button"
          >
            ×
          </button>
        </header>
        <input
          autoFocus
          type="text"
          placeholder="Search plugins..."
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <div className={styles.pluginPickerList}>
          {loading && <p className={styles.pluginPickerEmpty}>Scanning VST3 plugins...</p>}
          {error && <p className={styles.pluginPickerError}>{error}</p>}
          {!loading && !error && filtered.length === 0 && (
            <p className={styles.pluginPickerEmpty}>
              {query.trim() ? 'No plugins match your search.' : 'No VST3 plugins found.'}
            </p>
          )}
          {filtered.map((plugin) => (
            <button
              key={plugin.id}
              type="button"
              className={styles.pluginPickerItem}
              onClick={() => props.onSelect(plugin)}
            >
              <span>{plugin.name}</span>
              <small>{plugin.vendor ?? 'Unknown vendor'}</small>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
