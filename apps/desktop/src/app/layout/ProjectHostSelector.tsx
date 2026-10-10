import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
  ProjectActivationResult,
  ProjectState,
  ProjectSummary,
} from '@/model/domain';
import type { HostConnectionBootstrap } from '@/native/native-api';
import { openHostDataRoot } from '@/native/dialog';
import { Icon } from '@/shared/ui/primitives';
import styles from './ProjectHostSelector.module.css';

interface ProjectHostSelectorProps {
  state: HostConnectionState;
  hosts: LocalHostInfo[];
  switching: boolean;
  error: string | null;
  onRefresh: () => Promise<unknown>;
  onSwitch: (target: HostTarget) => Promise<HostConnectionBootstrap | null>;
  onReconnect: () => Promise<unknown>;
  onExportProject?: () => void;
  onImportProject?: () => void;
  onImportSonalloyBundle?: () => Promise<ProjectActivationResult | null>;
  projectState?: ProjectState | null;
  projectSwitching?: boolean;
  projectError?: string | null;
  onCreateProject?: (name?: string) => Promise<ProjectActivationResult | null>;
  onOpenProject?: (projectId: string) => Promise<ProjectActivationResult | null>;
  onRenameProject?: (name: string) => Promise<unknown>;
}

export function ProjectHostSelector(props: ProjectHostSelectorProps) {
  const [open, setOpen] = useState(false);
  const [nameDraft, setNameDraft] = useState('');
  const [query, setQuery] = useState('');
  const [refreshing, setRefreshing] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const discardNameDraftRef = useRef(false);
  const { onRefresh, onSwitch } = props;

  const hostLabel = getHostLabel(props.state, props.hosts);
  const projects = props.projectState?.projects ?? [];
  const activeProjectId = props.projectState?.activeProjectId;
  const activeProject = projects.find((project) => project.projectId === activeProjectId);
  const activeName = activeProject?.name ?? '';
  const projectName = props.projectState ? (activeProject?.name ?? 'Unreadable Project') : null;
  const projectActionsDisabled =
    props.switching || props.projectSwitching || props.state.mode === 'disconnected';
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visibleProjects = normalizedQuery
    ? projects.filter((project) => project.name.toLocaleLowerCase().includes(normalizedQuery))
    : projects;

  useEffect(() => {
    if (!open) return;
    setNameDraft(activeName);
  }, [activeName, open]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      const container = containerRef.current;
      if (!container || container.contains(event.target as Node)) return;
      // Blur first so a pending rename commits before the input unmounts.
      if (
        document.activeElement instanceof HTMLElement &&
        container.contains(document.activeElement)
      ) {
        document.activeElement.blur();
      }
      setOpen(false);
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
  }, [open]);

  const refreshSelector = useCallback(() => {
    setRefreshing(true);
    void Promise.resolve(onRefresh()).finally(() => setRefreshing(false));
  }, [onRefresh]);

  useEffect(() => {
    if (!open) return;
    setQuery('');
    refreshSelector();
  }, [open, refreshSelector]);

  const commitProjectName = (draft: string) => {
    const name = draft.trim();
    if (!props.onRenameProject || name === activeName) return;
    void props.onRenameProject(name);
  };

  const closeOnSuccess = (operation: Promise<unknown> | undefined) => {
    if (!operation) {
      setOpen(false);
      return;
    }
    void operation.then((result) => {
      if (result) setOpen(false);
    });
  };

  const openProject = (project: ProjectSummary) => {
    if (project.projectId === activeProjectId) {
      setOpen(false);
      return;
    }
    closeOnSuccess(props.onOpenProject?.(project.projectId));
  };

  const switchHost = (target: HostTarget) => closeOnSuccess(onSwitch(target));

  const hostBusy = props.switching || props.projectSwitching;
  const hostSection = (
    <div className={styles.hostSection}>
      <div className={styles.hostHeader}>
        <span>Host</span>
        {props.state.mode === 'disconnected' && (
          <button
            type="button"
            role="menuitem"
            className={styles.ghostButton}
            disabled={hostBusy}
            onClick={() => void props.onReconnect()}
          >
            Reconnect
          </button>
        )}
        <button
          type="button"
          role="menuitem"
          className={styles.ghostButton}
          disabled={hostBusy}
          onClick={() => {
            void openHostDataRoot()
              .then((dataRoot) => {
                if (dataRoot) switchHost({ type: 'dataRoot', dataRoot });
              })
              .catch(() => undefined);
          }}
        >
          Connect…
        </button>
        <button
          type="button"
          role="menuitem"
          className={styles.ghostButton}
          disabled={hostBusy || refreshing}
          onClick={refreshSelector}
        >
          {refreshing ? 'Refreshing…' : 'Refresh'}
        </button>
      </div>
      <div role="group" aria-label="Hosts">
        <button
          type="button"
          role="menuitem"
          className={styles.hostItem}
          aria-current={props.state.mode === 'embedded' || undefined}
          disabled={hostBusy}
          onClick={() =>
            props.state.mode === 'embedded' ? setOpen(false) : switchHost({ type: 'embedded' })
          }
        >
          <i className={styles.hostDot} data-mode="embedded" />
          <strong>Local Desktop</strong>
          {props.state.mode === 'embedded' && <Icon name="check" />}
        </button>
        {props.hosts.map((host) => {
          const current = host.instanceId === props.state.instanceId;
          return (
            <button
              type="button"
              role="menuitem"
              className={styles.hostItem}
              key={host.instanceId}
              title={host.dataRoot}
              aria-current={current || undefined}
              disabled={hostBusy}
              onClick={() =>
                current
                  ? setOpen(false)
                  : switchHost({ type: 'registration', instanceId: host.instanceId })
              }
            >
              <i className={styles.hostDot} data-mode="attached" />
              <strong>{host.projectName ?? basename(host.dataRoot) ?? host.instanceId}</strong>
              <small>
                PID {host.pid} · {host.safeMode ? 'Safe Mode' : host.status}
              </small>
              {current && <Icon name="check" />}
            </button>
          );
        })}
      </div>
    </div>
  );

  const projectSection = (
    <>
      <div className={styles.current}>
        <div className={styles.nameRow}>
          <input
            className={styles.nameInput}
            aria-label="Project name"
            placeholder="Untitled Project"
            value={nameDraft}
            disabled={projectActionsDisabled}
            onChange={(event) => setNameDraft(event.target.value)}
            onBlur={(event) => {
              if (discardNameDraftRef.current) {
                discardNameDraftRef.current = false;
                setNameDraft(activeName);
                return;
              }
              commitProjectName(event.currentTarget.value);
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.currentTarget.blur();
                setOpen(false);
              } else if (event.key === 'Escape') {
                discardNameDraftRef.current = true;
                event.currentTarget.blur();
              }
            }}
          />
          <button
            type="button"
            role="menuitem"
            className={styles.ghostButton}
            disabled={projectActionsDisabled}
            onClick={() => {
              props.onExportProject?.();
              setOpen(false);
            }}
          >
            Export…
          </button>
        </div>
        <span className={styles.meta}>
          Auto-saved
          {activeProject && ` · Updated ${formatUpdatedAt(activeProject.updatedAtMs)}`}
        </span>
      </div>
      <label className={styles.search}>
        <Icon name="search" />
        <input
          aria-label="Search Projects"
          placeholder="Search Projects"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      </label>
      <div className={styles.list} role="group" aria-label="Projects">
        {visibleProjects.map((project) => {
          const current = project.projectId === activeProjectId;
          return (
            <button
              type="button"
              role="menuitem"
              className={styles.item}
              key={project.projectId}
              aria-current={current || undefined}
              disabled={projectActionsDisabled}
              onClick={() => openProject(project)}
            >
              <span className={styles.itemText}>
                <strong>{project.name}</strong>
                {project.error ? (
                  <small className={styles.itemError}>{project.error}</small>
                ) : (
                  <small>{current ? 'Open' : formatUpdatedAt(project.updatedAtMs)}</small>
                )}
              </span>
              {current && <Icon name="check" />}
            </button>
          );
        })}
        {visibleProjects.length === 0 && <p className={styles.empty}>No matching Projects</p>}
      </div>
      <div className={styles.footer}>
        <button
          type="button"
          role="menuitem"
          className={styles.ghostButton}
          disabled={projectActionsDisabled}
          onClick={() => closeOnSuccess(props.onImportSonalloyBundle?.())}
        >
          Import Sonalloy Bundle…
        </button>
        <button
          type="button"
          role="menuitem"
          className={styles.footerButton}
          disabled={projectActionsDisabled}
          onClick={() => closeOnSuccess(props.onCreateProject?.())}
        >
          <Icon name="plus" />
          New Project
        </button>
        <button
          type="button"
          role="menuitem"
          className={styles.footerButton}
          disabled={projectActionsDisabled}
          onClick={() => {
            props.onImportProject?.();
            setOpen(false);
          }}
        >
          Import…
        </button>
      </div>
    </>
  );

  return (
    <div ref={containerRef} className={styles.selector} data-project-host-selector>
      <button
        type="button"
        className={styles.trigger}
        aria-label={`${projectName ? 'Project' : 'Host'}: ${projectName ?? hostLabel}`}
        aria-expanded={open}
        title={props.state.dataRoot ?? hostLabel}
        onClick={() => setOpen((current) => !current)}
      >
        <span className={styles.triggerText}>
          {projectName ? (
            <>
              <span className={styles.projectName}>
                <i className={styles.saveDot} title="Auto-saved" />
                {props.projectSwitching ? 'Opening…' : projectName}
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
        <div className={styles.panel} role="menu">
          {props.projectState && projectSection}
          {hostSection}
          {(props.error || props.projectError) && (
            <p className={styles.error}>{props.projectError ?? props.error}</p>
          )}
        </div>
      )}
    </div>
  );
}

const relativeTime = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 31_536_000_000],
  ['month', 2_592_000_000],
  ['week', 604_800_000],
  ['day', 86_400_000],
  ['hour', 3_600_000],
  ['minute', 60_000],
];

function formatUpdatedAt(updatedAtMs: number): string {
  const elapsed = updatedAtMs - Date.now();
  for (const [unit, size] of RELATIVE_UNITS) {
    if (Math.abs(elapsed) >= size) return relativeTime.format(Math.round(elapsed / size), unit);
  }
  return 'just now';
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
