import { Activity, type CSSProperties, type ReactNode, type RefObject } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSession } from '@/session/session-context';
import { selectionOwner } from '@/model/editor-selection';
import type { useLivePlay } from '@/features/live-input/hooks/useLivePlay';
import { PlaySurface } from '@/features/live-input/PlaySurface';
import { MidiEditorPanel } from '@/features/midi-editor/MidiEditorPanel';
import { DevicesPanel } from '@/features/devices/DevicesPanel';
import { MixerPanel } from '@/features/mixer/MixerPanel';
import { Icon } from '@/shared/ui/Icon';
import { ToolbarButton } from '@/shared/ui/Toolbar';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import styles from './WorkspaceLowerArea.module.css';
import type { LowerPanelView, WorkspaceLowerView } from '@/session/slices/workspace';

const COLLAPSED_HEIGHT = 36;
const MINIMUM_HEIGHT = 180;

interface LowerTab {
  view: LowerPanelView;
  label: string;
  icon: string;
}

const LOWER_TABS: LowerTab[] = [
  { view: 'midiEditor', label: 'MIDI', icon: 'note' },
  { view: 'devices', label: 'Devices', icon: 'module' },
  { view: 'mixer', label: 'Mixer', icon: 'mixer' },
  { view: 'playSurface', label: 'Play', icon: 'keys' },
];

interface WorkspaceLowerAreaProps {
  /** The Workspace element whose height bounds the lower area. */
  workspaceRef: RefObject<HTMLElement | null>;
  view: WorkspaceLowerView;
  height: number;
  collapsed: boolean;
  maximized: boolean;
  onSelectTab: (view: LowerPanelView) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onHeightChange: (height: number) => void;
  controls: ReactNode;
  children: ReactNode;
}

function LowerAreaLayout({
  workspaceRef,
  view,
  height,
  collapsed,
  maximized,
  onSelectTab,
  onCollapsedChange,
  onHeightChange,
  controls,
  children,
}: WorkspaceLowerAreaProps) {
  const open = view !== 'closed';
  const showBody = open && !collapsed;

  const applyHeight = (nextHeight: number) => {
    const maxHeight = Math.max(MINIMUM_HEIGHT, (workspaceRef.current?.clientHeight ?? 900) - 42);
    const clamped = Math.min(maxHeight, nextHeight);
    if (clamped < MINIMUM_HEIGHT) {
      onCollapsedChange(true);
      return;
    }
    onCollapsedChange(false);
    onHeightChange(clamped);
  };

  return (
    <section
      className={`${styles.area}${!showBody ? ` ${styles.collapsed}` : ''}${open && maximized ? ` ${styles.maximized}` : ''}`}
      style={{ '--lower-height': `${height}px` } as CSSProperties}
      aria-label="Arrange lower area"
      data-arrange-lower-area
      data-view={view}
    >
      {open && (
        <ResizeHandle
          orientation="horizontal"
          ariaLabel="Resize Arrange lower area"
          ariaValueMin={COLLAPSED_HEIGHT}
          ariaValueNow={collapsed ? COLLAPSED_HEIGHT : height}
          value={collapsed ? COLLAPSED_HEIGHT : height}
          direction={-1}
          onResize={(startValue, delta) => applyHeight(startValue + delta)}
          style={{ position: 'absolute', top: 0, left: 0, width: '100%' }}
        />
      )}
      <header className={styles.header}>
        <div className={styles.tabs} role="tablist" aria-label="Lower area views">
          {LOWER_TABS.map((tab) => {
            const active = open && view === tab.view;
            return (
              <button
                key={tab.view}
                type="button"
                role="tab"
                aria-selected={active}
                className={styles.tab}
                data-active={active || undefined}
                onClick={() => onSelectTab(tab.view)}
              >
                <Icon name={tab.icon} />
                <span>{tab.label}</span>
              </button>
            );
          })}
        </div>
        {open && (
          <div className={styles.actions} aria-label="Arrange lower area controls">
            {controls}
          </div>
        )}
      </header>
      {open && (
        <Activity mode={collapsed ? 'hidden' : 'visible'}>
          <div className={styles.content}>{children}</div>
        </Activity>
      )}
    </section>
  );
}

export function WorkspaceLowerArea({
  play,
  workspaceRef,
}: {
  play: ReturnType<typeof useLivePlay>;
  workspaceRef: RefObject<HTMLElement | null>;
}) {
  const {
    lower,
    selection,
    focusedTrackId,
    setSelection,
    focusTrack,
    toggleLowerView,
    openMidiEditor,
    setLowerCollapsed,
    setLowerMaximized,
    setLowerHeight,
    closeLower,
  } = useSession(
    useShallow((state) => ({
      lower: state.workspace.lower,
      selection: state.workspace.selection,
      focusedTrackId: state.workspace.focusedTrackId,
      setSelection: state.workspace.setSelection,
      focusTrack: state.workspace.focusTrack,
      toggleLowerView: state.workspace.toggleLowerView,
      openMidiEditor: state.workspace.openMidiEditor,
      setLowerCollapsed: state.workspace.setLowerCollapsed,
      setLowerMaximized: state.workspace.setLowerMaximized,
      setLowerHeight: state.workspace.setLowerHeight,
      closeLower: state.workspace.closeLower,
    })),
  );
  const arrangement = useSession((state) => state.project.boot!.canonical.session.arrangement);

  const selectTab = (nextView: LowerPanelView) => {
    if (lower.view === nextView && !lower.collapsed) {
      closeLower();
      return;
    }
    if (nextView === 'midiEditor') {
      if (lower.activeMidiClipId) {
        toggleLowerView('midiEditor');
        return;
      }
      const selectedClip =
        selection.kind === 'clips'
          ? arrangement.midiClips.find((clip) => selection.clipIds.includes(clip.id))
          : undefined;
      const trackId = selection.kind === 'track' ? selection.trackId : focusedTrackId;
      const trackClip = trackId
        ? arrangement.midiClips.find((clip) => clip.trackId === trackId)
        : undefined;
      const targetClip = selectedClip ?? trackClip ?? arrangement.midiClips[0];
      if (targetClip) {
        openMidiEditor(targetClip.id);
        return;
      }
      toggleLowerView('midiEditor');
      return;
    }
    if (nextView === 'devices' && !selectionOwner(selection)) {
      const fallbackTrackId = focusedTrackId ?? arrangement.tracks[0]?.id ?? null;
      if (fallbackTrackId) setSelection({ kind: 'track', trackId: fallbackTrackId });
    }
    if (nextView === 'playSurface' && !focusedTrackId) {
      const selectedTrack =
        selection.kind === 'track'
          ? arrangement.tracks.find((track) => track.id === selection.trackId)
          : undefined;
      const fallbackInstrument =
        selectedTrack?.kind === 'instrument'
          ? selectedTrack
          : arrangement.tracks.find((track) => track.kind === 'instrument');
      if (fallbackInstrument) focusTrack(fallbackInstrument.id);
    }
    toggleLowerView(nextView);
  };

  const controls = (
    <>
      <ToolbarButton
        icon={lower.collapsed ? 'expand' : 'collapse'}
        ariaLabel={lower.collapsed ? 'Restore lower area' : 'Collapse lower area'}
        title={lower.collapsed ? 'Restore lower area' : 'Collapse lower area'}
        onClick={() => setLowerCollapsed(!lower.collapsed)}
      />
      <ToolbarButton
        icon={lower.maximized ? 'restore' : 'maximize'}
        ariaLabel={lower.maximized ? 'Restore lower area size' : 'Maximize lower area'}
        title={lower.maximized ? 'Restore lower area size' : 'Maximize lower area'}
        onClick={() => setLowerMaximized(!lower.maximized)}
      />
      <ToolbarButton
        icon="close"
        ariaLabel="Close lower area"
        title="Close lower area"
        onClick={closeLower}
      />
    </>
  );
  return (
    <LowerAreaLayout
      workspaceRef={workspaceRef}
      view={lower.view}
      height={lower.height}
      collapsed={lower.collapsed}
      maximized={lower.maximized}
      onSelectTab={selectTab}
      onCollapsedChange={setLowerCollapsed}
      onHeightChange={setLowerHeight}
      controls={controls}
    >
      <>
        {lower.view === 'playSurface' && <PlaySurface play={play} />}
        {lower.view === 'midiEditor' && <MidiEditorPanel />}
        {lower.view === 'devices' && <DevicesPanel />}
        {lower.view === 'mixer' && <MixerPanel />}
      </>
    </LowerAreaLayout>
  );
}
