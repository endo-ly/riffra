import clsx from 'clsx';
import { useState } from 'react';
import type { PluginEntry, Track } from '@/model/domain';
import { Icon } from '@/shared/ui/primitives';
import styles from '@/features/browser/BrowserPanel.module.css';

interface PluginBrowserSectionProps {
  plugins: PluginEntry[];
  visiblePlugins: PluginEntry[];
  selectedTrack: Track | null;
  projectSwitching: boolean;
  onAddPlugin: (plugin: PluginEntry, target: 'instrument' | 'effect') => void;
}

export function PluginBrowserSection(props: PluginBrowserSectionProps) {
  const [message, setMessage] = useState<string | null>(null);

  return (
    <div className={styles.pluginArea}>
      {props.visiblePlugins.length < props.plugins.length && (
        <small className={styles.scanMessage}>
          Showing {props.visiblePlugins.length} of {props.plugins.length} plugins
        </small>
      )}
      {props.visiblePlugins.slice(0, 12).map((plugin) => (
        <div className={styles.pluginRow} key={plugin.id}>
          <span className={styles.rowIcon}>
            <Icon name="module" />
          </span>
          <div>
            <strong>{plugin.name}</strong>
            <small>{plugin.vendor ?? 'VST3'}</small>
          </div>
          <i className={clsx(styles.stability, styles[plugin.scanState])} />
          <button
            type="button"
            className={styles.pluginAdd}
            aria-label={
              props.selectedTrack
                ? `${
                    props.selectedTrack.kind === 'instrument' && props.selectedTrack.instrument
                      ? 'Replace instrument with'
                      : 'Add'
                  } ${plugin.name} as ${
                    props.selectedTrack.kind === 'instrument' ? 'instrument' : 'effect'
                  } on ${props.selectedTrack.name}`
                : `Select a Track before adding ${plugin.name}`
            }
            onClick={() => {
              if (!props.selectedTrack) {
                setMessage('Select a Track before adding a Plugin.');
                return;
              }
              setMessage(null);
              props.onAddPlugin(
                plugin,
                props.selectedTrack.kind === 'instrument' ? 'instrument' : 'effect',
              );
            }}
            disabled={props.projectSwitching || plugin.scanState !== 'validated'}
            title={
              plugin.scanState === 'validated'
                ? props.selectedTrack
                  ? `${
                      props.selectedTrack.kind === 'instrument' && props.selectedTrack.instrument
                        ? 'Replace instrument with'
                        : 'Add'
                    } ${plugin.name} on ${props.selectedTrack.name}`
                  : `Select a Track before adding ${plugin.name}`
                : `${plugin.name} is ${plugin.scanState} and cannot be loaded`
            }
          >
            <Icon name="plus" />
          </button>
        </div>
      ))}
      {message && <small className={styles.inboxMessage}>{message}</small>}
      {props.visiblePlugins.length === 0 && (
        <div className={styles.libraryEmpty}>
          <span>No plugins match</span>
          <small>Adjust the search or check your VST3 folders.</small>
        </div>
      )}
    </div>
  );
}

export type { PluginBrowserSectionProps };
