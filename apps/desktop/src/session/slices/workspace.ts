import type { AutomationTarget, CreativeSession } from '@/model/generated';
import type { EditorSelection } from '@/model/editor-selection';
import { automationOwner } from '@/model/automation';

export type LowerPanelView = 'midiEditor' | 'devices' | 'mixer' | 'playSurface';
export type WorkspaceLowerView = 'closed' | LowerPanelView;
export interface LowerAreaState {
  activeMidiClipId: string | null;
  view: WorkspaceLowerView;
  collapsed: boolean;
  maximized: boolean;
  height: number;
}
export interface WorkspaceSlice {
  selection: EditorSelection;
  focusedTrackId: string | null;
  browserOpen: boolean;
  inspectorOpen: boolean;
  lower: LowerAreaState;
  automation: {
    trackTargets: Partial<Record<string, AutomationTarget>>;
    mixerTarget: AutomationTarget | null;
  };
  reset(): void;
  setSelection(selection: EditorSelection): void;
  focusTrack(id: string | null): void;
  setBrowserOpen(open: boolean): void;
  toggleBrowser(): void;
  setInspectorOpen(open: boolean): void;
  toggleInspector(): void;
  openMidiEditor(id: string): void;
  keepMidiClipVisible(id: string): void;
  openDevices(): void;
  toggleLowerView(view: LowerPanelView): void;
  closeLower(): void;
  setLowerCollapsed(value: boolean): void;
  setLowerMaximized(value: boolean): void;
  setLowerHeight(value: number): void;
  openAutomation(target: AutomationTarget): void;
  toggleTrackAutomation(id: string): void;
  closeTrackAutomation(id: string): void;
  closeMixerAutomation(): void;
}
const initialLower = (): LowerAreaState => ({
  activeMidiClipId: null,
  view: 'closed',
  collapsed: false,
  maximized: false,
  height: 360,
});
export function initialWorkspace() {
  return {
    selection: { kind: 'none' } as EditorSelection,
    focusedTrackId: null,
    browserOpen: false,
    inspectorOpen: false,
    lower: initialLower(),
    automation: { trackTargets: {}, mixerTarget: null },
  };
}
export function reconcileWorkspace(
  workspace: WorkspaceSlice,
  session: CreativeSession,
  previousSession: CreativeSession,
): WorkspaceSlice {
  let { selection, focusedTrackId, lower } = workspace;
  const tracks = session.arrangement.tracks;
  const currentSelection = selection;
  const invalidSelection =
    (currentSelection.kind === 'track' &&
      !tracks.some((track) => track.id === currentSelection.trackId)) ||
    (currentSelection.kind === 'bus' &&
      !session.arrangement.mixer.buses.some((bus) => bus.id === currentSelection.busId));
  if (
    currentSelection.kind === 'clips' &&
    (session.arrangement.audioClips !== previousSession.arrangement.audioClips ||
      session.arrangement.midiClips !== previousSession.arrangement.midiClips)
  ) {
    const remaining = new Set([
      ...session.arrangement.audioClips.map((clip) => clip.id),
      ...session.arrangement.midiClips.map((clip) => clip.id),
    ]);
    const clipIds = currentSelection.clipIds.filter((id) => remaining.has(id));
    if (clipIds.length !== currentSelection.clipIds.length)
      selection = clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' };
  }
  if (invalidSelection) {
    selection = { kind: 'none' };
    if (lower.view === 'devices')
      lower = { ...lower, view: 'closed', collapsed: false, maximized: false };
  }
  if (focusedTrackId && !tracks.some((track) => track.id === focusedTrackId)) focusedTrackId = null;
  if (
    lower.activeMidiClipId &&
    !session.arrangement.midiClips.some((clip) => clip.id === lower.activeMidiClipId)
  ) {
    lower = { ...lower, activeMidiClipId: null };
    if (lower.view === 'midiEditor')
      lower = { ...lower, view: 'closed', collapsed: false, maximized: false };
  }
  return selection === workspace.selection &&
    focusedTrackId === workspace.focusedTrackId &&
    lower === workspace.lower
    ? workspace
    : { ...workspace, selection, focusedTrackId, lower };
}
export function createWorkspaceSlice(
  get: () => WorkspaceSlice,
  update: (patch: Partial<WorkspaceSlice>) => void,
  session: () => CreativeSession | null,
): WorkspaceSlice {
  const lower = (patch: Partial<LowerAreaState>) => update({ lower: { ...get().lower, ...patch } });
  return {
    ...initialWorkspace(),
    reset: () => update(initialWorkspace()),
    setSelection(selection) {
      if (selection.kind === 'track') {
        const track = session()?.arrangement.tracks.find((item) => item.id === selection.trackId);
        if (track) {
          update({
            selection,
            focusedTrackId: track.kind === 'instrument' ? track.id : null,
          });
          return;
        }
      }
      update({ selection });
    },
    focusTrack: (focusedTrackId) => update({ focusedTrackId }),
    setBrowserOpen: (browserOpen) => update({ browserOpen }),
    toggleBrowser: () => update({ browserOpen: !get().browserOpen }),
    setInspectorOpen: (inspectorOpen) => update({ inspectorOpen }),
    toggleInspector: () => update({ inspectorOpen: !get().inspectorOpen }),
    openMidiEditor(id) {
      update({
        selection: { kind: 'clips', clipIds: [id] },
        lower: {
          ...get().lower,
          activeMidiClipId: id,
          view: 'midiEditor',
          collapsed: false,
        },
      });
    },
    keepMidiClipVisible: (id) => lower({ activeMidiClipId: id, collapsed: false }),
    openDevices: () => lower({ view: 'devices', collapsed: false }),
    toggleLowerView(view) {
      const current = get().lower;
      if (current.view === view && !current.collapsed) {
        lower({ view: 'closed', collapsed: false, maximized: false });
        return;
      }
      lower({ view, collapsed: false });
    },
    closeLower: () => lower({ view: 'closed', collapsed: false, maximized: false }),
    setLowerCollapsed: (collapsed) =>
      lower({ collapsed, ...(collapsed ? { maximized: false } : {}) }),
    setLowerMaximized: (maximized) => lower({ collapsed: false, maximized }),
    setLowerHeight: (height) => lower({ height }),
    openAutomation(target) {
      const current = session();
      if (!current) return;
      const owner = automationOwner(current.arrangement, target);
      if (owner?.kind === 'track')
        update({
          automation: {
            ...get().automation,
            trackTargets: { ...get().automation.trackTargets, [owner.id]: target },
          },
        });
      else if (owner) update({ automation: { ...get().automation, mixerTarget: target } });
    },
    toggleTrackAutomation(id) {
      const automation = get().automation;
      update({
        automation: {
          ...automation,
          trackTargets: {
            ...automation.trackTargets,
            [id]: automation.trackTargets[id]
              ? undefined
              : { kind: 'trackMix', trackId: id, parameter: 'volume' },
          },
        },
      });
    },
    closeTrackAutomation(id) {
      const automation = get().automation;
      update({
        automation: {
          ...automation,
          trackTargets: { ...automation.trackTargets, [id]: undefined },
        },
      });
    },
    closeMixerAutomation() {
      update({ automation: { ...get().automation, mixerTarget: null } });
    },
  };
}
