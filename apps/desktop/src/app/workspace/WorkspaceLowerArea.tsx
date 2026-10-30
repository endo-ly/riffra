import type { CSSProperties, ReactNode } from 'react';
import { useSession } from '@/shared/session/session-context';
import type { useLivePlay } from '@/features/live-input/useLivePlay';
import { PlaySurface } from '@/features/live-input/PlaySurface';
import { MidiEditorPanel } from '@/features/midi-editor/MidiEditorPanel';
import { DevicesPanel } from '@/features/devices/DevicesPanel';
import { MixerPanel } from '@/features/mixer/MixerPanel';
import { resolveDeviceTarget } from '@/features/devices/device-target';
import { selectionOwner } from '@/model/editor-selection';
import { resolveTrackColor } from '@/model/track-color';
import { ToolbarButton } from '@/shared/ui/Toolbar';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import { usePointerGesture } from '@/shared/usePointerGesture';
import styles from './WorkspaceLowerArea.module.css';
import type { WorkspaceLowerView } from '@/shared/session/slices/workspace';

const COLLAPSED_HEIGHT = 48;

interface WorkspaceLowerAreaProps {
  view: WorkspaceLowerView;
  height: number;
  minimumHeight: number;
  collapsed: boolean;
  maximized: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  onHeightChange: (height: number) => void;
  controls: ReactNode;
  midiEditor: ReactNode;
  mixer: ReactNode;
  devices: ReactNode;
  playSurface: ReactNode;
}

function LowerAreaLayout({
  view,
  height,
  minimumHeight,
  collapsed,
  maximized,
  onCollapsedChange,
  onHeightChange,
  controls,
  midiEditor,
  mixer,
  devices,
  playSurface,
}: WorkspaceLowerAreaProps) {
  const startGesture = usePointerGesture();
  if (view === 'closed') return null;

  const applyHeight = (workspace: HTMLElement | null, nextHeight: number) => {
    const maxHeight = Math.max(minimumHeight, (workspace?.clientHeight ?? 900) - 42);
    const clamped = Math.min(maxHeight, nextHeight);
    if (clamped < minimumHeight) {
      onCollapsedChange(true);
      return;
    }
    onCollapsedChange(false);
    onHeightChange(clamped);
  };

  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = collapsed ? COLLAPSED_HEIGHT : height;
    const workspace = event.currentTarget.closest<HTMLElement>('[data-arrange-workspace]');
    startGesture(event, {
      onMove: (pointer) => applyHeight(workspace, startHeight - pointer.clientY + startY),
      onEnd: () => undefined,
    });
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    event.preventDefault();
    const delta = (event.key === 'ArrowUp' ? 1 : -1) * (event.shiftKey ? 24 : 8);
    const workspace = document.querySelector<HTMLElement>('[data-arrange-workspace]');
    applyHeight(workspace, (collapsed ? COLLAPSED_HEIGHT : height) + delta);
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
        onPointerDown={startResize}
        onKeyDown={onKeyDown}
        style={{ position: 'absolute', top: 0, left: 0, width: '100%' }}
      />
      <div className={styles.actions} aria-label="Arrange lower area controls">
        {controls}
      </div>
      <div className={styles.content} hidden={collapsed}>
        {view === 'midiEditor' && midiEditor}
        {view === 'mixer' && mixer}
        {view === 'devices' && devices}
        {view === 'playSurface' && playSurface}
      </div>
    </section>
  );
}

export function WorkspaceLowerArea({ play }: { play: ReturnType<typeof useLivePlay> }) {
  const session = useSession((state) => state.project.boot!.canonical.session);
  const workspace = useSession((state) => state.workspace);
  const audio = useSession((state) => state.audio.status);
  const generation = useSession((state) => state.host.connection.generation);
  const { arrangement } = session;
  const lower = workspace.lower;
  const heightKey = lower.view === 'mixer' || lower.view === 'playSurface' ? lower.view : 'detail';
  const clip = arrangement.midiClips.find((item) => item.id === lower.activeMidiClipId) ?? null;
  const clipTrackIndex = clip
    ? arrangement.tracks.findIndex((track) => track.id === clip.trackId)
    : -1;
  const clipTrack = arrangement.tracks[clipTrackIndex];
  const focusedTrack =
    arrangement.tracks.find((track) => track.id === workspace.focusedTrackId) ?? null;
  const owner = selectionOwner(workspace.selection);
  const target = owner ? resolveDeviceTarget(arrangement, owner) : null;
  const controls = (
    <>
      <ToolbarButton
        icon={lower.collapsed ? 'expand' : 'collapse'}
        ariaLabel={lower.collapsed ? 'Restore lower area' : 'Collapse lower area'}
        title={lower.collapsed ? 'Restore lower area' : 'Collapse lower area'}
        onClick={() => workspace.setLowerCollapsed(!lower.collapsed)}
      />
      <ToolbarButton
        icon={lower.maximized ? 'restore' : 'maximize'}
        ariaLabel={lower.maximized ? 'Restore lower area size' : 'Maximize lower area'}
        title={lower.maximized ? 'Restore lower area size' : 'Maximize lower area'}
        onClick={() => workspace.setLowerMaximized(!lower.maximized)}
      />
      <ToolbarButton
        icon="close"
        ariaLabel="Close lower area"
        title="Close lower area"
        onClick={workspace.closeLower}
      />
    </>
  );
  return (
    <LowerAreaLayout
      view={lower.view}
      height={lower.heights[heightKey]}
      minimumHeight={lower.view === 'mixer' ? 240 : lower.view === 'playSurface' ? 220 : 180}
      collapsed={lower.collapsed}
      maximized={lower.maximized}
      onCollapsedChange={workspace.setLowerCollapsed}
      onHeightChange={workspace.setLowerHeight}
      controls={controls}
      playSurface={
        <PlaySurface
          play={play}
          trackColor={
            focusedTrack
              ? resolveTrackColor(focusedTrack, arrangement.tracks.indexOf(focusedTrack))
              : null
          }
          audio={audio}
          onChooseInstrument={() => {
            if (focusedTrack) {
              workspace.setSelection({ kind: 'track', trackId: focusedTrack.id });
              workspace.openDevices();
            }
          }}
        />
      }
      midiEditor={
        <MidiEditorPanel
          clip={clip}
          trackColor={clipTrack ? resolveTrackColor(clipTrack, clipTrackIndex) : null}
        />
      }
      devices={
        <DevicesPanel key={`${session.sessionId}:${JSON.stringify(target?.owner)}:${generation}`} />
      }
      mixer={<MixerPanel />}
    />
  );
}
