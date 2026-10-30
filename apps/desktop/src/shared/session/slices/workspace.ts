import type { AutomationTarget, CreativeSession } from '@/model/generated';
import type { EditorSelection } from '@/model/editor-selection';
import { automationOwner } from '@/model/automation';

type DetailView = 'midiEditor' | 'devices';
type OverlayView = 'mixer' | 'playSurface';
export type WorkspaceLowerView = 'closed' | DetailView | OverlayView;
export interface LowerAreaState {
  activeMidiClipId: string | null;
  view: WorkspaceLowerView;
  collapsed: boolean;
  maximized: boolean;
  heights: Record<'detail' | OverlayView, number>;
  returnView: DetailView | null;
}
export interface WorkspaceSlice {
  selection: EditorSelection;
  focusedTrackId: string | null;
  lower: LowerAreaState;
  automation: {
    trackTargets: Partial<Record<string, AutomationTarget>>;
    mixerTarget: AutomationTarget | null;
  };
  reset(): void;
  setSelection(selection: EditorSelection): void;
  focusTrack(id: string | null): void;
  openMidiEditor(id: string): void;
  keepMidiClipVisible(id: string): void;
  openDevices(): void;
  toggleMixer(): void;
  togglePlaySurface(): void;
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
  heights: { detail: 380, mixer: 320, playSurface: 260 },
  returnView: null,
});
export function initialWorkspace() {
  return {
    selection: { kind: 'none' } as EditorSelection,
    focusedTrackId: null,
    lower: initialLower(),
    automation: { trackTargets: {}, mixerTarget: null },
  };
}
export function reconcileWorkspace(
  workspace: WorkspaceSlice,
  session: CreativeSession,
): WorkspaceSlice {
  let { selection, focusedTrackId, lower } = workspace;
  const tracks = session.arrangement.tracks;
  const currentSelection = selection;
  const invalidSelection =
    (currentSelection.kind === 'track' &&
      !tracks.some((track) => track.id === currentSelection.trackId)) ||
    (currentSelection.kind === 'bus' &&
      !session.arrangement.mixer.buses.some((bus) => bus.id === currentSelection.busId));
  if (invalidSelection) {
    selection = { kind: 'none' };
    if (lower.view === 'devices')
      lower = { ...lower, view: 'closed', collapsed: false, maximized: false, returnView: null };
  }
  if (focusedTrackId && !tracks.some((track) => track.id === focusedTrackId)) focusedTrackId = null;
  if (
    lower.activeMidiClipId &&
    !session.arrangement.midiClips.some((clip) => clip.id === lower.activeMidiClipId)
  ) {
    lower = { ...lower, activeMidiClipId: null };
    if (lower.view === 'midiEditor')
      lower = { ...lower, view: 'closed', collapsed: false, maximized: false, returnView: null };
  }
  return { ...workspace, selection, focusedTrackId, lower };
}
export function createWorkspaceSlice(
  get: () => WorkspaceSlice,
  update: (patch: Partial<WorkspaceSlice>) => void,
  session: () => CreativeSession | null,
): WorkspaceSlice {
  const lower = (patch: Partial<LowerAreaState>) => update({ lower: { ...get().lower, ...patch } });
  const toggle = (overlay: OverlayView) => {
    const current = get().lower;
    if (current.view === overlay) {
      const exists = session()?.arrangement.midiClips.some(
        (clip) => clip.id === current.activeMidiClipId,
      );
      const view =
        current.returnView === 'devices'
          ? 'devices'
          : current.returnView === 'midiEditor' && exists
            ? 'midiEditor'
            : 'closed';
      lower({ view, returnView: null, collapsed: false });
      return;
    }
    lower({
      view: overlay,
      collapsed: false,
      returnView:
        current.view === 'midiEditor' || current.view === 'devices'
          ? current.view
          : current.view === 'closed'
            ? null
            : current.returnView,
    });
  };
  return {
    ...initialWorkspace(),
    reset: () => update(initialWorkspace()),
    setSelection: (selection) => update({ selection }),
    focusTrack: (focusedTrackId) => update({ focusedTrackId }),
    openMidiEditor(id) {
      update({
        selection: { kind: 'clips', clipIds: [id] },
        lower: {
          ...get().lower,
          activeMidiClipId: id,
          returnView: null,
          view: 'midiEditor',
          collapsed: false,
        },
      });
    },
    keepMidiClipVisible: (id) => lower({ activeMidiClipId: id, collapsed: false }),
    openDevices: () => lower({ returnView: null, view: 'devices', collapsed: false }),
    toggleMixer: () => toggle('mixer'),
    togglePlaySurface: () => toggle('playSurface'),
    closeLower: () =>
      lower({ view: 'closed', collapsed: false, maximized: false, returnView: null }),
    setLowerCollapsed: (collapsed) =>
      lower({ collapsed, ...(collapsed ? { maximized: false } : {}) }),
    setLowerMaximized: (maximized) => lower({ collapsed: false, maximized }),
    setLowerHeight(height) {
      const current = get().lower;
      const key =
        current.view === 'mixer' || current.view === 'playSurface' ? current.view : 'detail';
      lower({ heights: { ...current.heights, [key]: height } });
    },
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
