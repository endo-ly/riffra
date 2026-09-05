import { useEffect, useMemo, useState } from 'react';
import type { BuiltInInstrumentSummary, PluginEntry } from '@/model/domain';
import type { JobApi } from '@/native/native-api';
import styles from '../WorkspaceArrangeOverlay.module.css';

interface InstrumentPickerProps {
  api?: Pick<JobApi, 'scanVst3Folder'>;
  builtInInstruments: BuiltInInstrumentSummary[];
  plugins?: PluginEntry[];
  onSelectBuiltIn: (presetId: string) => void;
  onSelectVst3: (plugin: PluginEntry) => void;
  onClose: () => void;
}

export function InstrumentPicker(props: InstrumentPickerProps) {
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
    if (!props.api) {
      setLoading(false);
      setError('External plugin catalog is unavailable.');
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
      .catch((reason) => {
        if (cancelled) return;
        setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [props.api, props.plugins]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  const plugins = props.plugins ?? scannedPlugins;
  const trimmedQuery = query.trim().toLowerCase();
  const filteredBuiltIns = useMemo(
    () =>
      trimmedQuery
        ? props.builtInInstruments.filter(
            (instrument) =>
              instrument.name.toLowerCase().includes(trimmedQuery) ||
              (instrument.description ?? '').toLowerCase().includes(trimmedQuery),
          )
        : props.builtInInstruments,
    [props.builtInInstruments, trimmedQuery],
  );
  const filteredPlugins = useMemo(
    () =>
      trimmedQuery
        ? plugins.filter(
            (plugin) =>
              plugin.name.toLowerCase().includes(trimmedQuery) ||
              (plugin.vendor ?? '').toLowerCase().includes(trimmedQuery),
          )
        : plugins,
    [plugins, trimmedQuery],
  );

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
        aria-label="Choose Instrument"
        aria-modal="true"
      >
        <header>
          <strong>Choose Instrument</strong>
          <button type="button" className={styles.pluginPickerCancel} onClick={props.onClose}>
            Cancel
          </button>
        </header>
        <input
          autoFocus
          type="text"
          placeholder="Search instruments and plugins..."
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <div className={styles.pluginPickerList}>
          <strong>Built-in Instruments</strong>
          {filteredBuiltIns.map((instrument) => (
            <button
              key={instrument.id}
              type="button"
              className={styles.pluginPickerItem}
              aria-label={`${instrument.name}${instrument.description ? ` — ${instrument.description}` : ''}`}
              onClick={() => props.onSelectBuiltIn(instrument.id)}
            >
              <span>{instrument.name}</span>
              {instrument.description && <small>{instrument.description}</small>}
            </button>
          ))}
          {!filteredBuiltIns.length && (
            <p className={styles.pluginPickerEmpty}>No built-in instruments match your search.</p>
          )}

          <strong>External Plugins</strong>
          {loading && <p className={styles.pluginPickerEmpty}>Scanning VST3 plugins...</p>}
          {error && <p className={styles.pluginPickerError}>{error}</p>}
          {!loading && !error && !filteredPlugins.length && (
            <p className={styles.pluginPickerEmpty}>
              {trimmedQuery ? 'No plugins match your search.' : 'No VST3 plugins found.'}
            </p>
          )}
          {filteredPlugins.map((plugin) => (
            <button
              key={plugin.id}
              type="button"
              className={styles.pluginPickerItem}
              aria-label={`${plugin.name} — ${plugin.vendor ?? 'Unknown vendor'}`}
              onClick={() => props.onSelectVst3(plugin)}
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
