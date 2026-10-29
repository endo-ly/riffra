import { useCallback, useEffect, useMemo, useState } from 'react';
import type { MidiClip } from '@/model/generated';

type WorkspaceDetailView = 'midiEditor' | 'devices';
/** Views toggled over the detail views; closing one returns to the detail view below it. */
type WorkspaceOverlayView = 'mixer' | 'playSurface';
export type WorkspaceLowerView = 'closed' | WorkspaceDetailView | WorkspaceOverlayView;

const DEFAULT_HEIGHTS: Record<'detail' | WorkspaceOverlayView, number> = {
  detail: 380,
  mixer: 320,
  playSurface: 260,
};

interface WorkspaceLowerAreaControllerOptions {
  midiClips: MidiClip[];
  selectClip: (clipId: string, append?: boolean) => void;
}

/** Coordinates the shared Arrange lower area and its mutually exclusive views. */
export function useWorkspaceLowerAreaController({
  midiClips,
  selectClip,
}: WorkspaceLowerAreaControllerOptions) {
  const [activeMidiClipId, setActiveMidiClipId] = useState<string | null>(null);
  const [view, setView] = useState<WorkspaceLowerView>('closed');
  const [collapsed, setCollapsedState] = useState(false);
  const [maximized, setMaximizedState] = useState(false);
  const [heights, setHeights] = useState(DEFAULT_HEIGHTS);
  const [returnView, setReturnView] = useState<WorkspaceDetailView | null>(null);
  const activeMidiClip = useMemo(
    () => midiClips.find((clip) => clip.id === activeMidiClipId) ?? null,
    [activeMidiClipId, midiClips],
  );
  const heightKey = view === 'mixer' || view === 'playSurface' ? view : 'detail';
  const height = heights[heightKey];

  const close = useCallback(() => {
    setView('closed');
    setCollapsedState(false);
    setMaximizedState(false);
    setReturnView(null);
  }, []);

  const setCollapsed = useCallback((next: boolean) => {
    setCollapsedState(next);
    if (next) setMaximizedState(false);
  }, []);

  const setMaximized = useCallback((next: boolean) => {
    setCollapsedState(false);
    setMaximizedState(next);
  }, []);

  const toggleOverlay = useCallback(
    (overlay: WorkspaceOverlayView) => {
      setCollapsedState(false);
      if (view === overlay) {
        const nextView =
          returnView === 'devices'
            ? 'devices'
            : returnView === 'midiEditor' && activeMidiClip
              ? 'midiEditor'
              : 'closed';
        setReturnView(null);
        setView(nextView);
        return;
      }
      if (view === 'midiEditor' || view === 'devices') setReturnView(view);
      else if (view === 'closed') setReturnView(null);
      setView(overlay);
    },
    [activeMidiClip, returnView, view],
  );

  const toggleMixer = useCallback(() => toggleOverlay('mixer'), [toggleOverlay]);
  const togglePlaySurface = useCallback(() => toggleOverlay('playSurface'), [toggleOverlay]);

  const openMidiEditor = useCallback(
    (clip: MidiClip) => {
      selectClip(clip.id);
      setActiveMidiClipId(clip.id);
      setReturnView(null);
      setView('midiEditor');
      setCollapsedState(false);
    },
    [selectClip],
  );

  const openDevices = useCallback(() => {
    setReturnView(null);
    setView('devices');
    setCollapsedState(false);
  }, []);

  const keepSelectedMidiClipVisible = useCallback((clipId: string) => {
    setActiveMidiClipId(clipId);
    setCollapsedState(false);
  }, []);

  const setHeight = useCallback(
    (nextHeight: number) => setHeights((current) => ({ ...current, [heightKey]: nextHeight })),
    [heightKey],
  );

  useEffect(() => {
    if (activeMidiClipId !== null && !activeMidiClip) {
      setActiveMidiClipId(null);
      if (view === 'midiEditor') close();
    }
  }, [activeMidiClip, activeMidiClipId, close, view]);

  return {
    activeMidiClip,
    view,
    collapsed,
    maximized,
    height,
    openMidiEditor,
    openDevices,
    keepSelectedMidiClipVisible,
    toggleMixer,
    togglePlaySurface,
    close,
    setCollapsed,
    setMaximized,
    setHeight,
  };
}
