import { useMemo } from 'react';
import type { HostConnectionState, HostTarget, LocalHostInfo } from '@/model/domain';
import { openHostDataRoot } from '@/native/dialog';
import { Icon } from '@/shared/ui/primitives';
import styles from './HostSelector.module.css';

interface HostSelectorProps {
  state: HostConnectionState;
  hosts: LocalHostInfo[];
  switching: boolean;
  error: string | null;
  onRefresh: () => Promise<unknown>;
  onSwitch: (target: HostTarget) => Promise<unknown>;
  onReconnect: () => Promise<unknown>;
}

export function HostSelector(props: HostSelectorProps) {
  const currentHost = useMemo(() => {
    if (props.state.mode === 'embedded') return 'Local Desktop';
    if (props.state.mode === 'disconnected') return 'Disconnected';
    return (
      props.hosts.find((host) => host.instanceId === props.state.instanceId)?.projectName ??
      basename(props.state.dataRoot) ??
      'Attached Host'
    );
  }, [props.hosts, props.state]);

  const switchHost = (target: HostTarget) => {
    void props.onSwitch(target);
  };

  return (
    <details className={styles.selector} data-host-selector>
      <summary
        className={styles.current}
        aria-label={`Current Host: ${currentHost}`}
        title={props.state.dataRoot ?? currentHost}
      >
        <span className={styles.dot} data-mode={props.state.mode} />
        <span className={styles.currentText}>
          <strong>{props.switching ? 'Connecting…' : currentHost}</strong>
          <small>
            {props.state.mode === 'attached'
              ? `PID ${props.state.pid ?? '—'}`
              : props.state.mode === 'disconnected'
                ? (props.state.reason ?? 'Reconnect required')
                : 'This Desktop'}
          </small>
        </span>
        <Icon name="chevron" />
      </summary>
      <div className={styles.menu} role="menu">
        <span className={styles.heading}>Host</span>
        <button
          type="button"
          role="menuitem"
          className={styles.hostItem}
          disabled={props.switching || props.state.mode === 'embedded'}
          onClick={() => switchHost({ type: 'embedded' })}
        >
          <span className={styles.dot} data-mode="embedded" />
          <span>
            <strong>Local Desktop</strong>
            <small>This Desktop</small>
          </span>
        </button>
        {props.hosts.map((host) => (
          <button
            type="button"
            role="menuitem"
            className={styles.hostItem}
            key={host.instanceId}
            disabled={props.switching || host.instanceId === props.state.instanceId}
            onClick={() => switchHost({ type: 'registration', instanceId: host.instanceId })}
          >
            <span className={styles.dot} data-mode="attached" />
            <span>
              <strong>{host.projectName ?? basename(host.dataRoot) ?? host.instanceId}</strong>
              <small>
                PID {host.pid} · {host.safeMode ? 'Safe Mode' : host.status}
              </small>
              <small title={host.dataRoot}>{host.dataRoot}</small>
            </span>
          </button>
        ))}
        {props.state.mode === 'disconnected' && (
          <button
            type="button"
            role="menuitem"
            className={styles.action}
            disabled={props.switching}
            onClick={() => void props.onReconnect()}
          >
            Reconnect
          </button>
        )}
        <button
          type="button"
          role="menuitem"
          className={styles.action}
          disabled={props.switching}
          onClick={() => {
            void openHostDataRoot()
              .then((dataRoot) => {
                if (dataRoot) switchHost({ type: 'dataRoot', dataRoot });
              })
              .catch(() => undefined);
          }}
        >
          Connect to Local Host…
        </button>
        <button
          type="button"
          role="menuitem"
          className={styles.action}
          disabled={props.switching}
          onClick={() => void props.onRefresh()}
        >
          Refresh
        </button>
        {props.error && <p className={styles.error}>{props.error}</p>}
      </div>
    </details>
  );
}

function basename(path: string | null): string | null {
  if (!path) return null;
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? null;
}
