import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type {
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
  ProjectSummary,
} from '@/model/generated';
import { openHostDataRoot } from '@/native/dialog';
import { useSession, useSessionStore } from '@/session/session-context';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import { ContextMenu } from '@/shared/ui/ContextMenu';
import controls from '@/shared/ui/controls.module.css';
import { Icon } from '@/shared/ui/Icon';
import styles from './ProjectHostSelector.module.css';
import { InlineMessage } from '@/shared/ui/message/InlineMessage';
import { fileName } from '@/shared/path';

type ProjectOrder = 'recent' | 'name';

/** Viewport position of the open panel. */
interface PanelAnchor {
  left: number;
  top: number;
}

const PROJECT_ORDER_LABELS: Record<ProjectOrder, string> = { recent: 'Recent', name: 'Name' };

const projectNameCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

const PROJECT_ORDERS: Record<ProjectOrder, (a: ProjectSummary, b: ProjectSummary) => number> = {
  recent: (a, b) => b.updatedAtMs - a.updatedAtMs,
  name: (a, b) => projectNameCollator.compare(a.name, b.name),
};

export function ProjectHostSelector() {
  const host = useSession((state) => state.host);
  const project = useSession((state) => state.project);
  const store = useSessionStore();
  const state = host.connection;
  const hosts = host.hosts;
  const switching = host.switching;
  const error = host.error;
  const projectState = project.boot?.projectState ?? null;
  const projectSwitching = project.switching;
  const projectError = project.error;
  const refresh = useCallback(async () => {
    const current = store.getState();
    const tasks: Promise<unknown>[] = [current.host.refresh()];
    if (current.host.connection.mode !== 'disconnected' && !current.host.switching)
      tasks.push(current.project.refreshList());
    await Promise.allSettled(tasks);
  }, [store]);
  const onRefresh = refresh;
  const onSwitch = host.switchHost;
  const onReconnect = host.reconnect;
  const onCreateProject = project.create;
  const onOpenProject = project.open;
  const onDeleteProject = project.delete;
  const onRenameProject = project.rename;
  const onImportProject = () => void project.importPackage();
  const onImportScore = () => void project.importScore();
  const onExportProject = () => void project.exportPackage();

  const [anchor, setAnchor] = useState<PanelAnchor | null>(null);
  const [nameDraft, setNameDraft] = useState('');
  const [query, setQuery] = useState('');
  const [order, setOrder] = useState<ProjectOrder>('recent');
  const [refreshing, setRefreshing] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<ProjectSummary | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; project: ProjectSummary } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const discardNameDraftRef = useRef(false);
  const open = anchor !== null;
  const close = () => {
    if (panelRef.current?.contains(document.activeElement)) triggerRef.current?.focus();
    setAnchor(null);
  };

  const hostLabel = getHostLabel(state, hosts);
  const projects = projectState?.projects ?? [];
  const activeProjectId = projectState?.activeProjectId;
  const activeProject = projects.find((project) => project.projectId === activeProjectId);
  const activeName = activeProject?.name ?? '';
  const projectName = projectState ? (activeProject?.name ?? 'Unreadable Project') : null;
  const projectActionsDisabled = switching || projectSwitching || state.mode === 'disconnected';
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
      if (event.key === 'Escape') {
        setAnchor(null);
        triggerRef.current?.focus();
      }
    };
    window.addEventListener('mousedown', onPointerDown);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('mousedown', onPointerDown);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  useLayoutEffect(() => {
    if (!open) return;
    const reposition = () => {
      const trigger = triggerRef.current;
      const panel = panelRef.current;
      if (!trigger || !panel) return;
      const bounds = trigger.getBoundingClientRect();
      const margin = 4;
      const width = Math.min(320, Math.max(0, window.innerWidth - margin * 2));
      const height = Math.min(panel.scrollHeight, Math.max(0, window.innerHeight - margin * 2));
      const below = window.innerHeight - bounds.bottom - margin;
      const above = bounds.top - margin;
      const top =
        below < height && above > below ? bounds.top - margin - height : bounds.bottom + margin;
      const next = {
        left: Math.max(margin, Math.min(bounds.left, window.innerWidth - width - margin)),
        top: Math.max(margin, Math.min(top, window.innerHeight - height - margin)),
      };
      setAnchor((current) =>
        current && current.left === next.left && current.top === next.top ? current : next,
      );
    };
    reposition();
    const panel = panelRef.current;
    if (panel && !panel.contains(document.activeElement)) {
      (panel.querySelector<HTMLButtonElement>('button:not(:disabled)') ?? panel).focus();
    }
    window.addEventListener('resize', reposition);
    window.addEventListener('scroll', reposition, true);
    const observer = new ResizeObserver(reposition);
    if (panelRef.current) observer.observe(panelRef.current);
    return () => {
      window.removeEventListener('resize', reposition);
      window.removeEventListener('scroll', reposition, true);
      observer.disconnect();
    };
  }, [open]);

  const refreshSelector = useCallback(() => {
    setRefreshing(true);
    void Promise.resolve(onRefresh()).finally(() => setRefreshing(false));
  }, [onRefresh]);

  useEffect(() => {
    if (!open) return;
    setQuery('');
    setMenu(null);
    refreshSelector();
  }, [open, refreshSelector]);

  const commitProjectName = (draft: string) => {
    const name = draft.trim();
    if (!onRenameProject || name === activeName) return;
    void onRenameProject(name);
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
    closeOnSuccess(onOpenProject(project.projectId));
  };

  const requestDelete = (project: ProjectSummary) => {
    setPendingDelete(project);
    close();
  };

  const switchHost = (target: HostTarget) => closeOnSuccess(onSwitch(target));

  const hostBusy = switching || projectSwitching;
  const hostSection = (
    <section className={styles.section}>
      <header className={styles.sectionHeader}>
        <h3>Host</h3>
        {state.mode === 'disconnected' && (
          <button
            type="button"
            className={styles.textButton}
            disabled={hostBusy}
            onClick={() => void onReconnect()}
          >
            Reconnect
          </button>
        )}
        <button
          type="button"
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
          className={styles.row}
          aria-current={state.mode === 'embedded' || undefined}
          disabled={hostBusy}
          onClick={() => (state.mode === 'embedded' ? close() : switchHost({ type: 'embedded' }))}
        >
          <i className={styles.hostDot} data-mode="embedded" />
          <span className={styles.rowName}>Local Desktop</span>
          {state.mode === 'embedded' && <Icon name="check" />}
        </button>
        {hosts.map((host) => {
          const current = host.instanceId === state.instanceId;
          return (
            <button
              type="button"
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
                {host.projectName ?? fileName(host.dataRoot) ?? host.instanceId}
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
            className={controls.smallButton}
            disabled={projectActionsDisabled}
            onClick={() => {
              onExportProject();
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
                className={styles.row}
                key={project.projectId}
                title={project.error ?? undefined}
                aria-current={current || undefined}
                disabled={projectActionsDisabled}
                onClick={() => openProject(project)}
                onContextMenu={(event) => {
                  if (!onDeleteProject) return;
                  event.preventDefault();
                  setMenu({ x: event.clientX, y: event.clientY, project });
                }}
                onKeyDown={(event) => {
                  if (event.key !== 'Delete' || !onDeleteProject || current) return;
                  // The timeline deletes its selection on a window-level Delete unless it is handled.
                  event.preventDefault();
                  requestDelete(project);
                }}
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
          className={`${controls.smallButton} ${controls.accent}`}
          disabled={projectActionsDisabled}
          onClick={() => closeOnSuccess(onCreateProject())}
        >
          <Icon name="plus" />
          New Project
        </button>
        <button
          type="button"
          className={controls.smallButton}
          disabled={projectActionsDisabled}
          onClick={() => {
            onImportProject();
            close();
          }}
        >
          <Icon name="import" />
          Import…
        </button>
        <button
          type="button"
          className={controls.smallButton}
          title="Import Score YAML"
          disabled={projectActionsDisabled}
          onClick={() => {
            onImportScore();
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
        aria-haspopup="dialog"
        title={state.dataRoot ?? hostLabel}
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
                {projectSwitching ? 'Opening…' : projectName}
              </span>
              <span className={styles.hostLine}>
                {state.mode === 'disconnected' && (
                  <i className={styles.hostDot} data-mode={state.mode} />
                )}
                {switching ? 'Connecting…' : hostLabel}
              </span>
            </>
          ) : (
            <span className={styles.hostLine}>
              <i className={styles.hostDot} data-mode={state.mode} />
              {switching ? 'Connecting…' : hostLabel}
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
            role="dialog"
            tabIndex={-1}
            aria-label="Project and Host"
            style={{ left: anchor.left, top: anchor.top }}
          >
            {projectState && projectSection}
            {hostSection}
            {(error || projectError) && (
              <InlineMessage tone="danger">{projectError ?? error}</InlineMessage>
            )}
            {menu && (
              <ContextMenu
                x={menu.x}
                y={menu.y}
                items={[
                  {
                    label: 'Delete…',
                    danger: true,
                    disabled: menu.project.projectId === activeProjectId,
                    onClick: () => requestDelete(menu.project),
                  },
                ]}
                onClose={() => setMenu(null)}
              />
            )}
          </div>,
          document.body,
        )}
      {pendingDelete && (
        <ConfirmDialog
          title="Delete Project permanently?"
          message={`"${pendingDelete.name}" and its saved generations will be removed. Recordings, renders, and imported Assets in the Data Root are kept.`}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            const { projectId } = pendingDelete;
            setPendingDelete(null);
            void onDeleteProject(projectId);
          }}
          onCancel={() => setPendingDelete(null)}
        />
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
    fileName(state.dataRoot) ??
    'Attached Host'
  );
}
