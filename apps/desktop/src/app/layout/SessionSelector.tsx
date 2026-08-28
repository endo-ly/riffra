import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type {
  CreativeSession,
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
} from '@/model/domain';
import { openHostDataRoot } from '@/native/dialog';
import { Icon } from '@/shared/ui/primitives';
import styles from './SessionSelector.module.css';

interface SessionSelectorProps {
  session: CreativeSession | null;
  state: HostConnectionState;
  hosts: LocalHostInfo[];
  switching: boolean;
  error: string | null;
  onRefresh: () => Promise<unknown>;
  onSwitch: (target: HostTarget) => Promise<unknown>;
  onReconnect: () => Promise<unknown>;
  onRenameSession?: (name: string) => void;
  onExportSession?: () => void;
  onImportSession?: () => void;
}

export function SessionSelector(props: SessionSelectorProps) {
  const [open, setOpen] = useState(false);
  const [placement, setPlacement] = useState<'below' | 'above'>('below');
  const [nameDraft, setNameDraft] = useState('');
  const [refreshing, setRefreshing] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  const hostLabel = getHostLabel(props.state, props.hosts);
  const sessionName = props.session ? (props.session.projectName ?? 'Untitled Scratch') : null;

  useEffect(() => {
    if (!open) return;
    setNameDraft(props.session?.projectName ?? '');
    const onPointerDown = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    window.addEventListener('mousedown', onPointerDown);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('mousedown', onPointerDown);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [open, props.session]);

  useLayoutEffect(() => {
    if (!open) return;
    const panel = panelRef.current;
    const container = containerRef.current;
    if (!panel || !container) return;
    const spaceBelow = window.innerHeight - container.getBoundingClientRect().bottom;
    setPlacement(panel.offsetHeight > spaceBelow - 8 ? 'above' : 'below');
  }, [open]);

  const commitSessionName = () => {
    if (!props.session || !props.onRenameSession) return;
    const name = nameDraft.trim().slice(0, 160);
    if (name === (props.session.projectName ?? '')) return;
    props.onRenameSession(name);
  };

  const refreshHosts = () => {
    setRefreshing(true);
    void Promise.resolve(props.onRefresh()).finally(() => setRefreshing(false));
  };

  return (
    <div ref={containerRef} className={styles.selector} data-session-selector>
      <button
        type="button"
        className={styles.trigger}
        aria-label={`Session: ${sessionName ?? hostLabel}`}
        aria-expanded={open}
        title={props.state.dataRoot ?? hostLabel}
        onClick={() => setOpen((current) => !current)}
      >
        <span className={styles.triggerText}>
          {sessionName ? (
            <>
              <span className={styles.sessionName}>
                <i className={styles.saveDot} title="Auto-saved" />
                {sessionName}
              </span>
              <span className={styles.hostLine}>
                {props.state.mode === 'disconnected' && (
                  <i className={styles.hostDot} data-mode={props.state.mode} />
                )}
                {props.switching ? 'Connecting…' : hostLabel}
              </span>
            </>
          ) : (
            <span className={styles.hostLine}>
              <i className={styles.hostDot} data-mode={props.state.mode} />
              {props.switching ? 'Connecting…' : hostLabel}
            </span>
          )}
        </span>
        <Icon name="chevron" />
      </button>
      {open && (
        <div ref={panelRef} className={styles.panel} data-placement={placement} role="menu">
          {props.session && (
            <>
              <span className={styles.heading}>Session</span>
              <input
                className={styles.renameInput}
                aria-label="Session name"
                placeholder="Untitled Scratch"
                value={nameDraft}
                maxLength={160}
                onChange={(event) => setNameDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    commitSessionName();
                    setOpen(false);
                  }
                }}
              />
              <button
                type="button"
                role="menuitem"
                className={styles.action}
                onClick={() => {
                  props.onExportSession?.();
                  setOpen(false);
                }}
              >
                Export Project
              </button>
              <button
                type="button"
                role="menuitem"
                className={styles.action}
                onClick={() => {
                  props.onImportSession?.();
                  setOpen(false);
                }}
              >
                Import Project…
              </button>
            </>
          )}
          <span className={styles.heading}>Host</span>
          <button
            type="button"
            role="menuitem"
            className={styles.hostItem}
            disabled={props.switching || props.state.mode === 'embedded'}
            onClick={() => {
              void props.onSwitch({ type: 'embedded' });
              setOpen(false);
            }}
          >
            <span className={styles.hostDot} data-mode="embedded" />
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
              onClick={() => {
                void props.onSwitch({ type: 'registration', instanceId: host.instanceId });
                setOpen(false);
              }}
            >
              <span className={styles.hostDot} data-mode="attached" />
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
                  if (dataRoot) void props.onSwitch({ type: 'dataRoot', dataRoot });
                })
                .catch(() => undefined);
              setOpen(false);
            }}
          >
            Connect to Local Host…
          </button>
          <button
            type="button"
            role="menuitem"
            className={styles.action}
            disabled={props.switching || refreshing}
            onClick={refreshHosts}
          >
            {refreshing ? 'Refreshing…' : 'Refresh'}
          </button>
          {props.error && <p className={styles.error}>{props.error}</p>}
        </div>
      )}
    </div>
  );
}

function getHostLabel(state: HostConnectionState, hosts: LocalHostInfo[]): string {
  if (state.mode === 'embedded') return 'Local Desktop';
  if (state.mode === 'disconnected') return 'Disconnected';
  return (
    hosts.find((host) => host.instanceId === state.instanceId)?.projectName ??
    basename(state.dataRoot) ??
    'Attached Host'
  );
}

function basename(path: string | null): string | null {
  if (!path) return null;
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? null;
}
