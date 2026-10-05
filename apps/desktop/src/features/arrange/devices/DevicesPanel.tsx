import { useEffect, useState } from 'react';
import type {
  ArrangementMutationResult,
  CanonicalState,
  InstrumentLibraryItem,
  PluginEntry,
  Track,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '../arrange-api';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
import { DeviceChain } from './DeviceChain';
import styles from './Devices.module.css';

interface DevicesPanelProps {
  projectId: string;
  track: Track | null;
  api: ArrangeWorkspaceApi;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  instruments: InstrumentLibraryItem[];
  plugins: PluginEntry[];
  missingDeviceIds: string[];
  onDisableMissingPlugin: (id: string) => Promise<void>;
  onReplaceMissingPlugin: (id: string, path: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
}

export function DevicesPanel(props: DevicesPanelProps) {
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { track } = props;
  const device =
    track?.instrument?.id === selectedDeviceId
      ? track.instrument
      : track?.effects.find((item) => item.id === selectedDeviceId);
  useEffect(() => {
    if (!device) setSelectedDeviceId(null);
  }, [device]);
  const runOperation = (operation: Promise<unknown>) => {
    setError(null);
    void operation.catch((failure: unknown) =>
      setError(failure instanceof Error ? failure.message : String(failure)),
    );
  };
  const commit = (operation: Promise<ArrangementMutationResult>) =>
    runOperation(
      operation.then((result) => {
        applyArrangementMutation(result, props.applyCanonicalState, setError);
      }),
    );
  return (
    <div className={styles.panel} aria-label="Devices">
      <header className={styles.header}>
        <strong>DEVICES</strong>
        <span>{track?.name ?? 'Select a Track'}</span>
      </header>
      {track ? (
        <DeviceChain
          {...props}
          track={track}
          selectedDeviceId={selectedDeviceId}
          onSelectDevice={setSelectedDeviceId}
          commit={commit}
          runOperation={runOperation}
        />
      ) : (
        <p>Select a Track to edit its devices.</p>
      )}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
