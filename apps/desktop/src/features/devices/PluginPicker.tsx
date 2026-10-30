import { matchesInstrumentQuery, matchesPluginQuery } from '@/model/instrument-search';
import type { InstrumentLibraryItem, PluginEntry, PluginRole } from '@/model/generated';
import { Modal } from '@/shared/ui/Modal';
import { useMemo, useState } from 'react';
import styles from './Picker.module.css';

type PickerInstrument = Pick<InstrumentLibraryItem, 'id' | 'name' | 'description' | 'origin'>;

interface PluginPickerProps {
  title: string;
  role: PluginRole;
  plugins: PluginEntry[];
  /** Built-in and user instruments listed before the VST3 instruments. */
  instruments?: PickerInstrument[];
  onSelectPlugin: (plugin: PluginEntry) => void;
  onSelectInstrument?: (instrumentId: string) => void;
  onClose: () => void;
}

export function PluginPicker(props: PluginPickerProps) {
  const [query, setQuery] = useState('');
  const hasInstruments = props.instruments !== undefined;
  const filteredInstruments = useMemo(
    () =>
      (props.instruments ?? []).filter((instrument) => matchesInstrumentQuery(instrument, query)),
    [props.instruments, query],
  );
  const filteredPlugins = useMemo(
    () =>
      props.plugins.filter(
        (plugin) => plugin.role === props.role && matchesPluginQuery(plugin, query),
      ),
    [props.plugins, props.role, query],
  );
  const roleLabel = props.role === 'instrument' ? 'instrument' : 'effect';

  return (
    <Modal
      label={props.title}
      closeOnBackdrop
      onClose={props.onClose}
      className={styles.pluginPicker}
    >
      <header>
        <strong>{props.title}</strong>
        <button type="button" className={styles.pluginPickerCancel} onClick={props.onClose}>
          Cancel
        </button>
      </header>
      <input
        autoFocus
        type="text"
        placeholder={hasInstruments ? 'Search instruments and plugins...' : 'Search plugins...'}
        value={query}
        onChange={(event) => setQuery(event.currentTarget.value)}
      />
      <div className={styles.pluginPickerList}>
        {hasInstruments && (
          <>
            <strong>Instruments</strong>
            {filteredInstruments.map((instrument) => (
              <button
                key={instrument.id}
                type="button"
                className={styles.pluginPickerItem}
                aria-label={`${instrument.name}${instrument.description ? ` — ${instrument.description}` : ''}`}
                onClick={() => props.onSelectInstrument?.(instrument.id)}
              >
                <span>{instrument.name}</span>
                {instrument.description && <small>{instrument.description}</small>}
              </button>
            ))}
            {!filteredInstruments.length && (
              <p className={styles.pluginPickerEmpty}>No instruments match your search.</p>
            )}
            <strong>External Instruments</strong>
          </>
        )}
        {!filteredPlugins.length && (
          <p className={styles.pluginPickerEmpty}>
            {hasInstruments
              ? query.trim()
                ? 'No external instruments match your search.'
                : 'No VST3 instruments found.'
              : query.trim()
                ? `No VST3 ${roleLabel}s match your search.`
                : `No VST3 ${roleLabel}s found.`}
          </p>
        )}
        {filteredPlugins.map((plugin) => (
          <button
            key={plugin.id}
            type="button"
            className={styles.pluginPickerItem}
            aria-label={`${plugin.name} — ${plugin.vendor ?? 'Unknown vendor'}`}
            onClick={() => props.onSelectPlugin(plugin)}
          >
            <span>{plugin.name}</span>
            <small>{plugin.vendor ?? 'Unknown vendor'}</small>
          </button>
        ))}
      </div>
    </Modal>
  );
}
