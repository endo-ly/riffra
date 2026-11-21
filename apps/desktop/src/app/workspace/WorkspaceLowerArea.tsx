import { Activity, type CSSProperties, type ReactNode, type RefObject } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSession } from '@/session/session-context';
import type { useLivePlay } from '@/features/live-input/useLivePlay';
import { WorkspacePlaySurface } from '@/features/live-input/PlaySurface';
import { WorkspaceMidiEditor } from '@/features/midi-editor/MidiEditorPanel';
import { DevicesPanel } from '@/features/devices/DevicesPanel';
import { MixerPanel } from '@/features/mixer/MixerPanel';
import { ToolbarButton } from '@/shared/ui/Toolbar';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import styles from '@/app/workspace/WorkspaceLowerArea.module.css';
import type { WorkspaceLowerView } from '@/session/slices/workspace';

const COLLAPSED_HEIGHT = 48;

interface WorkspaceLowerAreaProps {
  /** The Workspace element whose height bounds the lower area. */
  workspaceRef: RefObject<HTMLElement | null>;
  view: WorkspaceLowerView;
  height: number;
  minimumHeight: number;
  collapsed: boolean;
  maximized: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  onHeightChange: (height: number) => void;
  controls: ReactNode;
  children: ReactNode;
}

function LowerAreaLayout({
  workspaceRef,
  view,
  height,
  minimumHeight,
  collapsed,
  maximized,
  onCollapsedChange,
  onHeightChange,
  controls,
  children,
}: WorkspaceLowerAreaProps) {
  if (view === 'closed') return null;

  const applyHeight = (nextHeight: number) => {
    const maxHeight = Math.max(minimumHeight, (workspaceRef.current?.clientHeight ?? 900) - 42);
    const clamped = Math.min(maxHeight, nextHeight);
    if (clamped < minimumHeight) {
      onCollapsedChange(true);
      return;
    }
    onCollapsedChange(false);
    onHeightChange(clamped);
  };

  return (
    <section
      className={`${styles.area}${collapsed ? ` ${styles.collapsed}` : ''}${maximized ? ` ${styles.maximized}` : ''}`}
      style={{ '--lower-height': `${height}px` } as CSSProperties}
      aria-label="Arrange lower area"
      data-arrange-lower-area
      data-view={view}
    >
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
      <div className={styles.actions} aria-label="Arrange lower area controls">
        {controls}
      </div>
      <Activity mode={collapsed ? 'hidden' : 'visible'}>
        <div className={styles.content}>{children}</div>
      </Activity>
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
  const { lower, setLowerCollapsed, setLowerMaximized, setLowerHeight, closeLower } = useSession(
    useShallow((state) => ({
      lower: state.workspace.lower,
      setLowerCollapsed: state.workspace.setLowerCollapsed,
      setLowerMaximized: state.workspace.setLowerMaximized,
      setLowerHeight: state.workspace.setLowerHeight,
      closeLower: state.workspace.closeLower,
    })),
  );
  const heightKey = lower.view === 'mixer' || lower.view === 'playSurface' ? lower.view : 'detail';
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
      height={lower.heights[heightKey]}
      minimumHeight={lower.view === 'mixer' ? 240 : lower.view === 'playSurface' ? 220 : 180}
      collapsed={lower.collapsed}
      maximized={lower.maximized}
      onCollapsedChange={setLowerCollapsed}
      onHeightChange={setLowerHeight}
      controls={controls}
    >
      <>
        {lower.view === 'playSurface' && <WorkspacePlaySurface play={play} />}
        {lower.view === 'midiEditor' && <WorkspaceMidiEditor />}
        {lower.view === 'devices' && <DevicesPanel />}
        {lower.view === 'mixer' && <MixerPanel />}
      </>
    </LowerAreaLayout>
  );
}
