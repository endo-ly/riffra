import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type {
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
  ProjectActivationResult,
  ProjectState,
  ProjectSummary,
} from '@/model/generated';

import type { HostConnectionBootstrap } from '@/native/api/host-connection';
import { openHostDataRoot } from '@/native/dialog';
import { useSession, useSessionStore } from '@/shared/session/session-context';
import controls from '@/shared/ui/controls.module.css';
import { Icon } from '@/shared/ui/primitives';
import styles from './ProjectHostSelector.module.css';

type ProjectOrder = 'recent' | 'name';

const PROJECT_ORDER_LABELS: Record<ProjectOrder, string> = { recent: 'Recent', name: 'Name' };

const projectNameCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

const PROJECT_ORDERS: Record<ProjectOrder, (a: ProjectSummary, b: ProjectSummary) => number> = {
  recent: (a, b) => b.updatedAtMs - a.updatedAtMs,
  name: (a, b) => projectNameCollator.compare(a.name, b.name),
};

interface ProjectHostSelectorProps {
  state: HostConnectionState;
  hosts: LocalHostInfo[];
  switching: boolean;
  error: string | null;
  onRefresh: () => Promise<unknown>;
  onSwitch: (target: HostTarget) => Promise<HostConnectionBootstrap | null>;
  onReconnect: () => Promise<unknown>;
  onExportProject?: () => void;
  onImportScore?: () => void;
  onImportProject?: () => void;
  projectState?: ProjectState | null;
  projectSwitching?: boolean;
  projectError?: string | null;
  onCreateProject?: (name?: string) => Promise<ProjectActivationResult | null>;
  onOpenProject?: (projectId: string) => Promise<ProjectActivationResult | null>;
  onRenameProject?: (name: string) => Promise<unknown>;
}

/** Viewport position of the open panel, taken from the trigger when it opens. */
interface PanelAnchor {
  left: number;
  top: number;
}

export function ProjectHostSelector() {
  const host = useSession((state) => state.host);
  const project = useSession((state) => state.project);
  const store = useSessionStore();
  const refresh = useCallback(async () => {
    const current = store.getState();
    const tasks: Promise<unknown>[] = [current.host.refresh()];
    if (current.host.connection.mode !== 'disconnected' && !current.host.switching)
      tasks.push(current.project.refreshList());
    await Promise.allSettled(tasks);
  }, [store]);
  return (
    <ProjectHostSelectorView
      state={host.connection}
      hosts={host.hosts}
      switching={host.switching}
      error={host.error}
      onRefresh={refresh}
      onSwitch={host.switchHost}
      onReconnect={host.reconnect}
      projectState={project.boot?.projectState ?? null}
      projectSwitching={project.switching}
      projectError={project.error}
      onCreateProject={project.create}
      onOpenProject={project.open}
      onRenameProject={project.rename}
      onImportProject={() => void project.importPackage()}
      onImportScore={() => void project.importScore()}
      onExportProject={() => void project.exportPackage()}
    />
  );
}

export function ProjectHostSelectorView(props: ProjectHostSelectorProps) {
  const [anchor, setAnchor] = useState<PanelAnchor | null>(null);
  const [nameDraft, setNameDraft] = useState('');
  const [query, setQuery] = useState('');
  const [order, setOrder] = useState<ProjectOrder>('recent');
  const [refreshing, setRefreshing] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const discardNameDraftRef = useRef(false);
  const { onRefresh, onSwitch } = props;
  const open = anchor !== null;
  const close = () => setAnchor(null);

  const hostLabel = getHostLabel(props.state, props.hosts);
  const projects = props.projectState?.projects ?? [];
  const activeProjectId = props.projectState?.activeProjectId;
  const activeProject = projects.find((project) => project.projectId === activeProjectId);
  const activeName = activeProject?.name ?? '';
  const projectName = props.projectState ? (activeProject?.name ?? 'Unreadable Project') : null;
  const projectActionsDisabled =
    props.switching || props.projectSwitching || props.state.mode === 'disconnected';
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visibleProjects = (
    normalizedQuery
      ? projects.filter((project) => project.name.toLocaleLowerCase().includes(normalizedQuery))
      : projects
  ).toSorted(PROJECT_ORDERS[order]);

  useEffect(() => {
    if (!open) return;
    setNameDraft(activeName);
  }, [activeName, open]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || panelRef.current?.contains(target)) return;
      // Blur first so a pending rename commits before the input unmounts.
      if (
        document.activeElement instanceof HTMLElement &&
        panelRef.current?.contains(document.activeElement)
      ) {
        document.activeElement.blur();
      }
      setAnchor(null);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setAnchor(null);
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
      close();
      return;
    }
    void operation.then((result) => {
      if (result) close();
    });
  };

  const openProject = (project: ProjectSummary) => {
    if (project.projectId === activeProjectId) {
      close();
      return;
    }
    closeOnSuccess(props.onOpenProject?.(project.projectId));
  };

  const switchHost = (target: HostTarget) => closeOnSuccess(onSwitch(target));

  const hostBusy = props.switching || props.projectSwitching;
  const hostSection = (
    <section className={styles.section}>
      <header className={styles.sectionHeader}>
        <h3>Host</h3>
        {props.state.mode === 'disconnected' && (
          <button
            type="button"
            role="menuitem"
            className={styles.textButton}
            disabled={hostBusy}
            onClick={() => void props.onReconnect()}
          >
            Reconnect
          </button>
        )}
        <button
          type="button"
          role="menuitem"
          className={styles.textButton}
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
          className={styles.textButton}
          disabled={hostBusy || refreshing}
          onClick={refreshSelector}
        >
          {refreshing ? 'Refreshing…' : 'Refresh'}
        </button>
      </header>
      <div role="group" aria-label="Hosts">
        <button
          type="button"
          role="menuitem"
          className={styles.row}
          aria-current={props.state.mode === 'embedded' || undefined}
          disabled={hostBusy}
          onClick={() =>
            props.state.mode === 'embedded' ? close() : switchHost({ type: 'embedded' })
          }
        >
          <i className={styles.hostDot} data-mode="embedded" />
          <span className={styles.rowName}>Local Desktop</span>
          {props.state.mode === 'embedded' && <Icon name="check" />}
        </button>
        {props.hosts.map((host) => {
          const current = host.instanceId === props.state.instanceId;
          return (
            <button
              type="button"
              role="menuitem"
              className={styles.row}
              key={host.instanceId}
              title={host.dataRoot}
              aria-current={current || undefined}
              disabled={hostBusy}
              onClick={() =>
                current
                  ? close()
                  : switchHost({ type: 'registration', instanceId: host.instanceId })
              }
            >
              <i className={styles.hostDot} data-mode="attached" />
              <span className={styles.rowName}>
                {host.projectName ?? basename(host.dataRoot) ?? host.instanceId}
              </span>
              <small className={styles.rowDetail}>
                PID {host.pid} · {host.safeMode ? 'Safe Mode' : host.status}
              </small>
              {current && <Icon name="check" />}
            </button>
          );
        })}
      </div>
    </section>
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
                close();
              } else if (event.key === 'Escape') {
                discardNameDraftRef.current = true;
                event.currentTarget.blur();
              }
            }}
          />
          <button
            type="button"
            role="menuitem"
            className={controls.smallButton}
            disabled={projectActionsDisabled}
            onClick={() => {
              props.onExportProject?.();
              close();
            }}
          >
            <Icon name="export" />
            Export…
          </button>
        </div>
        <span className={styles.meta}>
          Auto-saved
          {activeProject && ` · Updated ${formatUpdatedAt(activeProject.updatedAtMs)}`}
        </span>
      </div>
      <section className={`${styles.section} ${styles.projects}`}>
        <header className={styles.sectionHeader}>
          <h3>
            Projects <span className={styles.count}>{projects.length}</span>
          </h3>
          <div className={controls.segmented} role="group" aria-label="Project order">
            {(Object.keys(PROJECT_ORDER_LABELS) as ProjectOrder[]).map((value) => (
              <button
                type="button"
                key={value}
                aria-pressed={order === value}
                onClick={() => setOrder(value)}
              >
                {PROJECT_ORDER_LABELS[value]}
              </button>
            ))}
          </div>
        </header>
        <label className={styles.search}>
          <Icon name="search" />
          <input
            aria-label="Search Projects"
            placeholder="Search Projects"
            value={query}
            autoFocus
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              const first = visibleProjects[0];
              // Enter that confirms an IME conversion only finishes typing the query.
              if (event.key !== 'Enter' || event.nativeEvent.isComposing) return;
              if (first && !projectActionsDisabled) openProject(first);
            }}
          />
        </label>
        <div className={styles.list} role="group" aria-label="Projects">
          {visibleProjects.map((project) => {
            const current = project.projectId === activeProjectId;
            return (
              <button
                type="button"
                role="menuitem"
                className={styles.row}
                key={project.projectId}
                title={project.error ?? undefined}
                aria-current={current || undefined}
                disabled={projectActionsDisabled}
                onClick={() => openProject(project)}
              >
                <span className={styles.rowName}>{project.name}</span>
                {project.error ? (
                  <small className={styles.rowError}>{project.error}</small>
                ) : current ? (
                  <Icon name="check" />
                ) : (
                  <small className={styles.rowDetail}>{formatUpdatedAt(project.updatedAtMs)}</small>
                )}
              </button>
            );
          })}
          {visibleProjects.length === 0 && <p className={styles.empty}>No matching Projects</p>}
        </div>
      </section>
      <div className={styles.actions}>
        <button
          type="button"
          role="menuitem"
          className={`${controls.smallButton} ${controls.accent}`}
          disabled={projectActionsDisabled}
          onClick={() => closeOnSuccess(props.onCreateProject?.())}
        >
          <Icon name="plus" />
          New Project
        </button>
        <button
          type="button"
          role="menuitem"
          className={controls.smallButton}
          disabled={projectActionsDisabled}
          onClick={() => {
            props.onImportProject?.();
            close();
          }}
        >
          <Icon name="import" />
          Import…
        </button>
        <button
          type="button"
          role="menuitem"
          className={controls.smallButton}
          title="Import Score YAML"
          disabled={projectActionsDisabled}
          onClick={() => {
            props.onImportScore?.();
            close();
          }}
        >
          <Icon name="import" />
          Score YAML…
        </button>
      </div>
    </>
  );

  return (
    <div className={styles.selector} data-project-host-selector>
      <button
        ref={triggerRef}
        type="button"
        className={styles.trigger}
        aria-label={`${projectName ? 'Project' : 'Host'}: ${projectName ?? hostLabel}`}
        aria-expanded={open}
        title={props.state.dataRoot ?? hostLabel}
        onClick={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          setAnchor((current) => (current ? null : { left: bounds.left, top: bounds.bottom + 4 }));
        }}
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
      {anchor &&
        // The Global Control Bar isolates its stacking, so the panel renders at the document
        // root to stay above the workspace below it.
        createPortal(
          <div
            ref={panelRef}
            className={styles.panel}
            role="menu"
            style={{ left: anchor.left, top: anchor.top }}
          >
            {props.projectState && projectSection}
            {hostSection}
            {(props.error || props.projectError) && (
              <p className={styles.error}>{props.projectError ?? props.error}</p>
            )}
          </div>,
          document.body,
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
