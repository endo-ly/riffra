import { errorText } from '@/shared/errors';
import { useEffect, useMemo, useState } from 'react';
import type { PluginEntry, PluginRole } from '@/model/generated';

import { useSessionContext } from '@/shared/session/session-context';
import { Modal } from '@/shared/ui/Modal';
import styles from './Picker.module.css';

interface PluginPickerProps {
  title?: string;
  role: PluginRole;
  plugins?: PluginEntry[];
  onSelect: (plugin: PluginEntry) => void;
  onClose: () => void;
}

export function PluginPicker(props: PluginPickerProps) {
  const { api } = useSessionContext();
  const { onClose } = props;
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
    api
      .scanVst3Folder()
      .then((report) => {
        if (cancelled) return;
        setScannedPlugins(report.plugins);
        setError(null);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(errorText(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [api, props.plugins]);

  const plugins = props.plugins ?? scannedPlugins;

  const filtered = useMemo(() => {
    const trimmed = query.trim().toLowerCase();
    return plugins.filter(
      (plugin) =>
        plugin.role === props.role &&
        (!trimmed ||
          plugin.name.toLowerCase().includes(trimmed) ||
          (plugin.vendor ?? '').toLowerCase().includes(trimmed)),
    );
  }, [plugins, props.role, query]);
  const roleLabel = props.role === 'instrument' ? 'instrument' : 'effect';

  return (
    <Modal
      label={props.title ?? 'Plugin Picker'}
      closeOnBackdrop
      onClose={onClose}
      className={styles.pluginPicker}
    >
      <header>
        <strong>{props.title ?? 'Choose Plugin'}</strong>
        <button type="button" className={styles.pluginPickerCancel} onClick={onClose}>
          Cancel
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
            {query.trim()
              ? 'No VST3 ' + roleLabel + 's match your search.'
              : 'No VST3 ' + roleLabel + 's found.'}
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
    </Modal>
  );
}
