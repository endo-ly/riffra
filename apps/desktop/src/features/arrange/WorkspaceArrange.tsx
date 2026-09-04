import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from 'react';
import type {
  AudioClip,
  AutomationParameter,
  AudioStatus,
  ArrangementMutationResult,
  CanonicalState,
  CreativeSession,
  Marker,
  MidiClip,
  PluginEntry,
  RuntimeProjectionStatus,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from './arrange-api';
import { ArrangeRuler } from './timeline/ArrangeRuler';
import { ArrangeToolbar } from './timeline/ArrangeToolbar';
import { ArrangeTrack } from './timeline/ArrangeTrack';
import { AutomationLaneView } from './timeline/AutomationLaneView';
import { MidiEditorPanel } from './midi-editor/MidiEditorPanel';
import { ArrangeDetailArea } from './ArrangeDetailArea';
import { ArrangePlayhead } from './components/ArrangePlayhead';
import { PlaySurfacePanel, type PlaySurfaceMode } from './play-surface/PlaySurfacePanel';
import { PluginPicker } from './inspector/PluginPicker';
import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import { ToolbarButton } from '@/shared/ui/Toolbar';
import { clearToast, showToast, toast } from '@/shared/toasts';
import {
  BASE_PIXELS_PER_QUARTER,
  buildTrackTimeline,
  timelineObjectEndTick,
  clipDurationTicks,
  countOffGridNotes,
  formatClock,
  formatMusicalPosition,
  ticksPerBar,
  ticksPerBeat,
  timelineGridDensity,
  snapGridTicks,
  TRACK_HEADER_WIDTH,
  type ArrangeTool,
  type SnapGrid,
  type TrackSize,
} from '@/features/arrange/model/arrange-timeline';
import { RIFFRA_ASSET_MIME } from '@/shared/asset-drag';
import { HostConnectionChangedError, getHostGeneration } from '@/native/invoke';
import { isEditableTarget } from '@/features/arrange/model/interaction';
import { useArrangeEditor, type ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import { useArrangeDetailController } from '@/features/arrange/hooks/useArrangeDetailController';
import { useArrangeRulerController } from '@/features/arrange/hooks/useArrangeRulerController';
import { useArrangeTransport } from '@/features/arrange/hooks/useArrangeTransport';
import { useArrangeDrop } from '@/features/arrange/hooks/useArrangeDrop';
import { useWaveformAnalyses } from '@/features/arrange/hooks/useWaveformAnalyses';
import styles from './WorkspaceArrange.module.css';
import overlayStyles from './WorkspaceArrangeOverlay.module.css';

interface WorkspaceArrangeProps {
  hostGeneration?: number;
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  selection: ArrangeSelection;
  setSelection: (selection: ArrangeSelection) => void;
  api: ArrangeWorkspaceApi;
  audio: AudioStatus;
  focusedTrackId: string | null;
  onFocusTrack: (trackId: string | null) => void;
  onToggleTransport: () => void;
  runtimeProjectionStatus: RuntimeProjectionStatus;
  runtimeProjectionFailure: string | null;
  onRetryRuntimeProjection: () => Promise<void>;
  missingDeviceIds?: string[];
  plugins?: PluginEntry[];
  playSurfaceHost: HTMLElement | null;
}

export function WorkspaceArrange(props: WorkspaceArrangeProps) {
  const { arrangement } = props.session;
  const { timebase } = arrangement;
  const { api } = props;
  const { onToggleTransport } = props;
  const [tool, setTool] = useState<ArrangeTool>('select');
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [zoom, setZoom] = useState(1);
  const trackSize: TrackSize = 'normal';
  const [trackSizes, setTrackSizes] = useState<Record<string, TrackSize>>({});
  const [automationParameters, setAutomationParameters] = useState<
    Partial<Record<string, AutomationParameter>>
  >({});
  const [rulerMode, setRulerMode] = useState<'bars' | 'time'>('bars');
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    items: ContextMenuItem[];
  } | null>(null);
  const [confirmRequest, setConfirmRequest] = useState<{
    title: string;
    message: string;
    confirmLabel?: string;
    danger?: boolean;
    onConfirm: () => void;
  } | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [playSurfaceMode, setPlaySurfaceMode] = useState<PlaySurfaceMode>('closed');
  const [playSurfaceSummary, setPlaySurfaceSummary] = useState('');
  const [emptyDragOver, setEmptyDragOver] = useState(false);
  const [pluginPicker, setPluginPicker] = useState<{
    trackId: string;
    kind: 'effect' | 'instrument';
  } | null>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const programmaticScrollRef = useRef(false);
  const { transport, displayTick, displayTickRef, seekLocally } = useArrangeTransport(
    props.api,
    timebase,
    props.hostGeneration ?? 0,
  );
  const analyses = useWaveformAnalyses(
    props.api,
    arrangement.audioClips,
    props.hostGeneration ?? 0,
  );
  const pixelsPerTick = (BASE_PIXELS_PER_QUARTER * zoom) / timebase.ppq;
  const barTicks = ticksPerBar(timebase);
  const timelineTicks = useMemo(() => {
    const contentEnd = Math.max(
      ...arrangement.audioClips.map((clip) => timelineObjectEndTick(clip, timebase)),
      ...arrangement.midiClips.map((clip) => timelineObjectEndTick(clip, timebase)),
      ...arrangement.automationLanes.flatMap((lane) => lane.points.map((point) => point.tick)),
      ...arrangement.markers.map((marker) => marker.tick),
      arrangement.loopRange.startTick,
      arrangement.loopRange.endTick,
      ...(arrangement.punchRange
        ? [arrangement.punchRange.startTick, arrangement.punchRange.endTick]
        : []),
      0,
    );
    return Math.max(barTicks * 16, contentEnd + barTicks * 2);
  }, [
    arrangement.audioClips,
    arrangement.automationLanes,
    arrangement.loopRange,
    arrangement.markers,
    arrangement.midiClips,
    arrangement.punchRange,
    barTicks,
    timebase,
  ]);
  const timelineWidth = timelineTicks * pixelsPerTick;
  const timelineGridStyle = useMemo(() => {
    const beatWidth = ticksPerBeat(timebase) * pixelsPerTick;
    const barWidth = barTicks * pixelsPerTick;
    const density = timelineGridDensity(timebase, pixelsPerTick);
    const layers = [
      `repeating-linear-gradient(90deg, rgba(211, 232, 235, 0.2) 0 1px, transparent 1px ${barWidth}px)`,
    ];
    if (density.showBeats) {
      layers.push(
        `repeating-linear-gradient(90deg, rgba(211, 232, 235, 0.09) 0 1px, transparent 1px ${beatWidth}px)`,
      );
    }
    if (density.subdivisionTicks) {
      const subdivisionWidth = density.subdivisionTicks * pixelsPerTick;
      layers.push(
        `repeating-linear-gradient(90deg, rgba(211, 232, 235, 0.045) 0 1px, transparent 1px ${subdivisionWidth}px)`,
      );
    }
    return { width: timelineWidth, backgroundImage: layers.join(', ') } as CSSProperties;
  }, [barTicks, pixelsPerTick, timebase, timelineWidth]);
  const editor = useArrangeEditor({
    ...props,
    tool,
    snap,
    pixelsPerTick,
    displayTick,
    analyses,
    onSplitToolUsed: () => setTool('select'),
  });
  const { commit, setMessage } = editor;
  const { handleDrop, isOsFileDrag } = useArrangeDrop({
    api: props.api,
    commit,
    hostGeneration: props.hostGeneration ?? 0,
    pixelsPerTick,
    snapTick: editor.snapTick,
    setMessage,
  });
  const sendMidiPreview = useCallback(
    (trackId: string, bytes: number[]) => api.sendMidiToTrack(trackId, bytes),
    [api],
  );
  const panicMidiPreview = useCallback((trackId: string) => api.panicMidiTrack(trackId), [api]);
  const commitMidiEdit = (operation: Promise<ArrangementMutationResult | null>) =>
    commit(operation);
  // Runtime projection status, rather than Arrangement revision, is the source
  // of truth for playback health. Marker and other authoring-only edits still
  // advance the canonical revision without requiring a new audio graph.
  const playbackOutOfSync =
    props.runtimeProjectionStatus.state === 'failed' || props.runtimeProjectionFailure !== null;
  const unavailableClipCount = transport?.unavailableClipIds?.length ?? 0;
  const missingDeviceCount = transport?.missingDeviceIds?.length ?? 0;
  const selectedClipIds = props.selection.kind === 'clips' ? props.selection.clipIds : [];
  const selectedTrackId = props.selection.kind === 'track' ? props.selection.trackId : null;
  const focusedTrackId = props.focusedTrackId;
  const focusedTrack = arrangement.tracks.find((track) => track.id === focusedTrackId) ?? null;
  const trackClipCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const clip of arrangement.audioClips) {
      counts.set(clip.trackId, (counts.get(clip.trackId) ?? 0) + 1);
    }
    for (const clip of arrangement.midiClips) {
      counts.set(clip.trackId, (counts.get(clip.trackId) ?? 0) + 1);
    }
    return counts;
  }, [arrangement.audioClips, arrangement.midiClips]);
  const ruler = useArrangeRulerController({
    arrangement,
    api,
    commit: editor.commit,
    snapTick: editor.snapTick,
    pixelsPerTick,
    displayTickRef,
    selectedClipCount: selectedClipIds.length,
    seekLocally,
    setMessage: editor.setMessage,
  });
  const detail = useArrangeDetailController({
    midiClips: arrangement.midiClips,
    selectClip: editor.selectClip,
  });
  const { activeMidiClip } = detail;
  const { handleKeyboard: handleRulerKeyboard, timeSelection: rulerTimeSelection } = ruler;
  const activeMidiTrack = activeMidiClip
    ? (arrangement.tracks.find((track) => track.id === activeMidiClip.trackId) ?? null)
    : null;
  const runtimeReady =
    !playbackOutOfSync &&
    props.audio.state !== 'starting' &&
    props.audio.state !== 'faulted' &&
    props.audio.state !== 'offline';
  const activeInstrumentUnavailable = Boolean(
    activeMidiTrack?.instrument &&
    (activeMidiTrack.instrument.disabledPlaceholder ||
      props.missingDeviceIds?.includes(activeMidiTrack.instrument.id) ||
      transport?.missingDeviceIds.includes(activeMidiTrack.instrument.id)),
  );
  const midiPreviewAvailable = Boolean(
    runtimeReady &&
    activeMidiTrack?.kind === 'instrument' &&
    activeMidiTrack.instrument &&
    !activeInstrumentUnavailable,
  );
  const statusMessage = playbackOutOfSync
    ? (props.runtimeProjectionFailure ??
      props.runtimeProjectionStatus.lastError ??
      'Playback runtime is out of sync')
    : unavailableClipCount || missingDeviceCount
      ? `Playback skipped ${unavailableClipCount} missing source${unavailableClipCount === 1 ? '' : 's'} and ${missingDeviceCount} missing device${missingDeviceCount === 1 ? '' : 's'}.`
      : editor.message;
  const statusPersistent = playbackOutOfSync || unavailableClipCount > 0 || missingDeviceCount > 0;
  const retryRuntimeProjection = props.onRetryRuntimeProjection;

  useEffect(() => {
    if (!statusMessage) {
      clearToast('arrange.status');
      return;
    }
    showToast('arrange.status', statusMessage, {
      kind: playbackOutOfSync ? 'error' : 'info',
      persistent: statusPersistent,
      ...(playbackOutOfSync
        ? { action: { label: 'Retry', onClick: () => void retryRuntimeProjection() } }
        : {}),
    });
    return () => clearToast('arrange.status');
  }, [playbackOutOfSync, retryRuntimeProjection, statusMessage, statusPersistent]);

  const detailControls = (
    <>
      <ToolbarButton
        icon={detail.collapsed ? 'expand' : 'collapse'}
        ariaLabel={detail.collapsed ? 'Restore detail area' : 'Collapse detail area'}
        title={detail.collapsed ? 'Restore detail area' : 'Collapse detail area'}
        onClick={() => detail.setCollapsed(!detail.collapsed)}
      />
      <ToolbarButton
        icon={detail.maximized ? 'restore' : 'maximize'}
        ariaLabel={detail.maximized ? 'Restore detail area size' : 'Maximize detail area'}
        title={detail.maximized ? 'Restore detail area size' : 'Maximize detail area'}
        onClick={() => detail.setMaximized(!detail.maximized)}
      />
      <ToolbarButton
        icon="close"
        ariaLabel="Close detail area"
        title="Close detail area"
        onClick={detail.close}
      />
    </>
  );

  const applyZoom = (next: number, clientX?: number) => {
    const bounded = Math.min(4, Math.max(0.35, next));
    const scroller = scrollerRef.current;
    if (!scroller) return setZoom(bounded);
    const bounds = scroller.getBoundingClientRect();
    const cursor = (clientX ?? bounds.left + bounds.width / 2) - bounds.left;
    const tick = Math.max(0, (scroller.scrollLeft + cursor - TRACK_HEADER_WIDTH) / pixelsPerTick);
    setZoom(bounded);
    requestAnimationFrame(() => {
      const nextPixels = (BASE_PIXELS_PER_QUARTER * bounded) / timebase.ppq;
      programmaticScrollRef.current = true;
      scroller.scrollLeft = Math.max(0, TRACK_HEADER_WIDTH + tick * nextPixels - cursor);
    });
  };

  const zoomToRange = useCallback(
    (startTick: number, endTick: number) => {
      const scroller = scrollerRef.current;
      if (!scroller) return;
      const span = Math.max(1, endTick - startTick);
      const usableWidth = Math.max(1, scroller.clientWidth - TRACK_HEADER_WIDTH - 32);
      const bounded = Math.min(
        4,
        Math.max(0.35, (usableWidth / span / BASE_PIXELS_PER_QUARTER) * timebase.ppq),
      );
      setZoom(bounded);
      requestAnimationFrame(() => {
        const nextPixels = (BASE_PIXELS_PER_QUARTER * bounded) / timebase.ppq;
        programmaticScrollRef.current = true;
        scroller.scrollLeft = Math.max(0, TRACK_HEADER_WIDTH + startTick * nextPixels - 16);
      });
    },
    [timebase.ppq],
  );

  // One shell-level keyboard boundary coordinates transport, zoom, and the
  // ruler controller without adding competing window listeners in child hooks.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (handleRulerKeyboard(event) || event.defaultPrevented) return;
      if (event.key === ' ' && !isEditableTarget(event.target)) {
        event.preventDefault();
        onToggleTransport();
        return;
      }
      if (event.key.toLowerCase() === 'z' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        if (!rulerTimeSelection || isEditableTarget(event.target)) return;
        event.preventDefault();
        zoomToRange(rulerTimeSelection.startTick, rulerTimeSelection.endTick);
        return;
      }
      if (event.key.toLowerCase() === 'f' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        if (isEditableTarget(event.target)) return;
        const clipEdges = [
          ...arrangement.audioClips.flatMap((clip) => [
            clip.startTick,
            timelineObjectEndTick(clip, timebase),
          ]),
          ...arrangement.midiClips.flatMap((clip) => [
            clip.startTick,
            timelineObjectEndTick(clip, timebase),
          ]),
        ];
        if (!clipEdges.length) return;
        event.preventDefault();
        zoomToRange(Math.min(...clipEdges), Math.max(...clipEdges));
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [
    arrangement.audioClips,
    arrangement.midiClips,
    handleRulerKeyboard,
    onToggleTransport,
    rulerTimeSelection,
    timebase,
    zoomToRange,
  ]);

  // Track vertical scroll so the ruler and ruler corner stay sticky to the top
  // of the scrolling viewport without leaving the timeline's horizontal flow.
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let frame = 0;
    const onScroll = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setScrollTop(scroller.scrollTop));
    };
    scroller.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      scroller.removeEventListener('scroll', onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);

  // Follow the playhead during playback: once the playhead crosses the follow
  // line the view scrolls continuously to keep it there. A manual scroll pauses
  // the follow while the playhead stays in view; when it leaves the viewport the
  // follow resumes automatically.
  const followPausedRef = useRef(false);
  useEffect(() => {
    if (transport?.state !== 'playing') return;
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let frame = 0;
    const update = () => {
      const playheadX = TRACK_HEADER_WIDTH + displayTickRef.current * pixelsPerTick;
      const left = scroller.scrollLeft;
      const followOffset = scroller.clientWidth * 0.32;
      if (followPausedRef.current) {
        if (playheadX < left || playheadX > left + scroller.clientWidth) {
          followPausedRef.current = false;
        }
      }
      if (!followPausedRef.current && (playheadX < left || playheadX >= left + followOffset)) {
        programmaticScrollRef.current = true;
        scroller.scrollLeft = Math.max(0, playheadX - followOffset);
      }
      frame = requestAnimationFrame(update);
    };
    frame = requestAnimationFrame(update);
    return () => cancelAnimationFrame(frame);
  }, [displayTickRef, pixelsPerTick, transport?.state]);

  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const onScroll = () => {
      if (programmaticScrollRef.current) {
        programmaticScrollRef.current = false;
        return;
      }
      followPausedRef.current = true;
    };
    scroller.addEventListener('scroll', onScroll, { passive: true });
    return () => scroller.removeEventListener('scroll', onScroll);
  }, []);

  const previousDiscontinuityRef = useRef<number | null>(null);
  useEffect(() => {
    if (!transport) return;
    const previous = previousDiscontinuityRef.current;
    previousDiscontinuityRef.current = transport.discontinuity;
    if (previous === null || previous === transport.discontinuity) return;

    const scroller = scrollerRef.current;
    if (!scroller) return;
    const playheadX = TRACK_HEADER_WIDTH + transport.timelineTick * pixelsPerTick;
    programmaticScrollRef.current = true;
    scroller.scrollLeft = Math.max(0, playheadX - scroller.clientWidth * 0.32);
    followPausedRef.current = false;
  }, [pixelsPerTick, transport]);

  const openRulerContextMenu = (event: React.MouseEvent<HTMLDivElement>, tick: number) => {
    event.preventDefault();
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Add Marker Here', onClick: () => ruler.addMarkerAt(tick) },
        {
          label: 'Set Loop to Selection',
          onClick: ruler.setLoopToSelection,
          disabled: !ruler.timeSelection,
        },
        {
          label: 'Set Punch Range',
          onClick: ruler.setPunchToSelection,
          disabled: !ruler.timeSelection,
        },
        { separator: true },
        {
          label: 'Clear Loop',
          onClick: () => ruler.clearRange('loop'),
          disabled: !arrangement.loopRange.enabled,
        },
        {
          label: 'Clear Punch',
          onClick: () => ruler.clearRange('punch'),
          disabled: !arrangement.punchRange,
        },
      ],
    });
  };

  const openRangeContextMenu = (
    event: React.MouseEvent<HTMLDivElement>,
    range: 'loop' | 'punch',
  ) => {
    event.preventDefault();
    ruler.selectRange(range);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            ruler.clearRange(range);
            ruler.clearSelectedRange();
          },
        },
      ],
    });
  };

  const openMarkerContextMenu = (event: React.MouseEvent, marker: Marker) => {
    event.preventDefault();
    ruler.selectMarker(marker.id);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Rename', onClick: () => ruler.renameMarker(marker) },
        { label: 'Delete', danger: true, onClick: () => ruler.removeMarker(marker) },
      ],
    });
  };

  const performDeleteTrack = async (trackId: string) => {
    const requestGeneration = getHostGeneration();
    const deletedTrack = arrangement.tracks.find((track) => track.id === trackId);
    if (
      props.focusedTrackId === trackId &&
      deletedTrack?.kind === 'instrument' &&
      deletedTrack.instrument
    ) {
      try {
        const status = await props.api.panicMidiTrack(trackId);
        if (getHostGeneration() !== requestGeneration) return;
        if (status) editor.setMessage(status.message);
      } catch (error) {
        if (error instanceof HostConnectionChangedError) return;
        editor.setMessage(String(error));
      }
    }
    if (getHostGeneration() !== requestGeneration) return;
    const next = await editor.commit(props.api.removeTrack(trackId));
    if (next) {
      if (props.focusedTrackId === trackId) {
        props.onFocusTrack(null);
      }
      const remaining = new Set([
        ...next.arrangement.audioClips.map((clip) => clip.id),
        ...next.arrangement.midiClips.map((clip) => clip.id),
      ]);
      const clipIds = selectedClipIds.filter((id) => remaining.has(id));
      props.setSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
    }
  };

  const deleteTrack = (trackId: string, name: string, clipCount: number) => {
    const detail = clipCount
      ? ` This also removes ${clipCount} Clip${clipCount === 1 ? '' : 's'} from the Timeline.`
      : '';
    setConfirmRequest({
      title: `Delete ${name}`,
      message: `${detail}\n\nSource assets will be kept.`,
      confirmLabel: 'Delete Track',
      danger: true,
      onConfirm: () => {
        setConfirmRequest(null);
        void performDeleteTrack(trackId);
      },
    });
  };

  const openAudioClipContextMenu = (event: React.MouseEvent, clip: AudioClip) => {
    event.preventDefault();
    event.stopPropagation();
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'Split at Playhead',
          onClick: () => {
            setContextMenu(null);
            void editor.splitClip(clip, displayTick);
          },
        },
        {
          label: 'Duplicate',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(api.duplicateAudioClip(clip.id));
          },
        },
        {
          label: clip.muted ? 'Unmute' : 'Mute',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(api.updateAudioClip(clip.id, { muted: !clip.muted }));
          },
        },
        {
          label: clip.loopEnabled ? 'Disable Loop' : 'Enable Loop',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(api.updateAudioClip(clip.id, { loopEnabled: !clip.loopEnabled }));
          },
        },
        {
          label: 'Merge with Previous',
          disabled: !arrangement.audioClips.some(
            (item) =>
              item.id !== clip.id &&
              item.trackId === clip.trackId &&
              item.assetId === clip.assetId &&
              item.startTick + clipDurationTicks(item, timebase) === clip.startTick &&
              item.sourceRange.end === clip.sourceRange.start,
          ),
          onClick: () => {
            setContextMenu(null);
            void editor.mergeAudioClipWithPrevious(clip);
          },
        },
        {
          label: 'Merge with Next',
          disabled: !arrangement.audioClips.some(
            (item) =>
              item.id !== clip.id &&
              item.trackId === clip.trackId &&
              item.assetId === clip.assetId &&
              item.startTick === clip.startTick + clipDurationTicks(clip, timebase) &&
              item.sourceRange.start === clip.sourceRange.end,
          ),
          onClick: () => {
            setContextMenu(null);
            void editor.mergeAudioClipWithNext(clip);
          },
        },
        { separator: true },
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            setContextMenu(null);
            void editor.commit(api.removeTimelineClips([clip.id], []));
          },
        },
      ],
    });
  };

  const openMidiClipContextMenu = (event: React.MouseEvent, clip: MidiClip) => {
    event.preventDefault();
    event.stopPropagation();
    const gridTicks = snapGridTicks(snap, timebase);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'Open MIDI Editor',
          onClick: () => {
            setContextMenu(null);
            detail.openMidiEditor(clip);
          },
        },
        {
          label: 'Split at Playhead',
          onClick: () => {
            setContextMenu(null);
            void editor.splitMidiClip(clip, displayTick);
          },
        },
        {
          label: 'Duplicate',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(api.duplicateMidiClip(clip.id));
          },
        },
        {
          label: clip.muted ? 'Unmute' : 'Mute',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(api.updateMidiClip(clip.id, { muted: !clip.muted }));
          },
        },
        {
          label: clip.loopEnabled ? 'Disable Loop' : 'Enable Loop',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(api.updateMidiClip(clip.id, { loopEnabled: !clip.loopEnabled }));
          },
        },
        {
          label: 'Quantize',
          disabled: gridTicks === 0,
          onClick: () => {
            setContextMenu(null);
            const offGrid = countOffGridNotes(clip.notes, gridTicks);
            if (offGrid === 0) {
              toast('Notes are already on the grid.');
              return;
            }
            void editor
              .commit(
                api.quantizeMidiNotes(
                  clip.id,
                  clip.notes.map((note) => note.id),
                  gridTicks,
                ),
              )
              .then((next) => {
                if (next)
                  toast(`Quantized ${offGrid} note${offGrid === 1 ? '' : 's'} to the grid.`);
              });
          },
        },
        {
          label: 'Merge with Previous',
          disabled: !arrangement.midiClips.some(
            (item) =>
              item.id !== clip.id &&
              item.trackId === clip.trackId &&
              item.startTick + item.durationTicks === clip.startTick,
          ),
          onClick: () => {
            setContextMenu(null);
            void editor.mergeMidiClipWithPrevious(clip);
          },
        },
        {
          label: 'Merge with Next',
          disabled: !arrangement.midiClips.some(
            (item) =>
              item.id !== clip.id &&
              item.trackId === clip.trackId &&
              item.startTick === clip.startTick + clip.durationTicks,
          ),
          onClick: () => {
            setContextMenu(null);
            void editor.mergeMidiClipWithNext(clip);
          },
        },
        { separator: true },
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            setContextMenu(null);
            void editor.commit(api.removeTimelineClips([], [clip.id]));
          },
        },
      ],
    });
  };

  const setTrackSizeForTrack = (trackId: string, size: TrackSize) => {
    setTrackSizes((value) => ({ ...value, [trackId]: size }));
  };
  const toggleAutomation = (trackId: string) =>
    setAutomationParameters((current) => ({
      ...current,
      [trackId]: current[trackId] ? undefined : 'volume',
    }));

  const createEmptyMidiClip = async (trackId: string, rawTick: number) => {
    const track = arrangement.tracks.find((item) => item.id === trackId);
    if (!track || track.kind !== 'instrument') return;
    const beforeIds = new Set(arrangement.midiClips.map((clip) => clip.id));
    const startTick = ruler.timeSelection
      ? ruler.timeSelection.startTick
      : editor.snapTick(rawTick);
    const durationTicks = ruler.timeSelection
      ? Math.max(1, ruler.timeSelection.endTick - ruler.timeSelection.startTick)
      : Math.max(1, barTicks);
    const next = await editor.commit(api.createMidiClip(trackId, startTick, durationTicks));
    if (!next) return;
    const created = next.arrangement.midiClips.find((clip) => !beforeIds.has(clip.id));
    if (!created) return;
    ruler.clearTimeSelection();
    detail.openMidiEditor(created);
  };

  const seekMidiEditor = (tick: number) => {
    const nextTick = Math.max(0, Math.round(tick));
    seekLocally(nextTick);
    void props.api.seekTimeline(nextTick).catch((error) => {
      if (error instanceof HostConnectionChangedError) return;
      setMessage(String(error));
    });
  };

  const openTrackLaneContextMenu = (event: React.MouseEvent, trackId: string, tick: number) => {
    event.preventDefault();
    const track = arrangement.tracks.find((item) => item.id === trackId);
    if (!track) return;
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'Add Audio Track',
          onClick: () =>
            void editor.commit(
              props.api.addTrack(`Audio ${arrangement.tracks.length + 1}`, 'audio'),
            ),
        },
        {
          label: 'Add Instrument Track',
          onClick: () =>
            void editor.commit(
              props.api.addTrack(`Instrument ${arrangement.tracks.length + 1}`, 'instrument'),
            ),
        },
        { separator: true },
        ...(track.kind === 'instrument'
          ? [
              {
                label: 'Insert MIDI Clip',
                onClick: () => {
                  setContextMenu(null);
                  void createEmptyMidiClip(track.id, tick);
                },
              },
              { separator: true },
            ]
          : []),
        {
          label: track.kind === 'audio' ? 'Add Effect' : 'Choose Instrument',
          onClick: () =>
            setPluginPicker({
              trackId: track.id,
              kind: track.kind === 'audio' ? 'effect' : 'instrument',
            }),
        },
        { separator: true },
        {
          label: 'Delete Track',
          danger: true,
          onClick: () => void deleteTrack(track.id, track.name, trackClipCounts.get(track.id) ?? 0),
        },
      ],
    });
  };

  return (
    <section
      className={styles.workspace}
      aria-label="Arrange timeline"
      data-arrange-workspace
      style={{ '--header-width': `${TRACK_HEADER_WIDTH}px` } as CSSProperties}
    >
      <ArrangeToolbar
        tool={tool}
        snap={snap}
        zoom={zoom}
        rulerMode={rulerMode}
        onTool={setTool}
        onSnap={setSnap}
        onZoom={applyZoom}
        onRulerMode={setRulerMode}
        automationAvailable={selectedTrackId !== null}
        automationOpen={selectedTrackId !== null && Boolean(automationParameters[selectedTrackId])}
        onToggleAutomation={() => {
          if (selectedTrackId) toggleAutomation(selectedTrackId);
        }}
        playSurfaceAvailable={focusedTrack?.kind === 'instrument'}
        playSurfaceOpen={playSurfaceMode !== 'closed'}
        onTogglePlaySurface={() =>
          setPlaySurfaceMode(playSurfaceMode === 'closed' ? 'expanded' : 'closed')
        }
      />

      {pluginPicker && (
        <PluginPicker
          api={props.api}
          plugins={props.plugins}
          title={pluginPicker.kind === 'effect' ? 'Add Effect' : 'Choose Instrument'}
          onSelect={(plugin) => {
            const { trackId, kind } = pluginPicker;
            setPluginPicker(null);
            if (kind === 'effect') {
              void editor.commit(props.api.addTrackEffect(trackId, plugin.path));
            } else {
              void editor.commit(props.api.setTrackInstrument(trackId, plugin.path));
            }
          }}
          onClose={() => setPluginPicker(null)}
        />
      )}

      {ruler.markerRename && (
        <form
          className={overlayStyles.markerDialog}
          aria-label="Rename marker"
          onSubmit={(event) => {
            event.preventDefault();
            ruler.saveMarkerRename();
          }}
        >
          <strong>Rename Marker</strong>
          <label>
            <span>Name</span>
            <input
              autoFocus
              value={ruler.markerRename.name}
              onChange={(event) => ruler.updateMarkerRename(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Escape') ruler.cancelMarkerRename();
              }}
            />
          </label>
          <div>
            <button type="button" onClick={ruler.cancelMarkerRename}>
              Cancel
            </button>
            <button type="submit">Save</button>
          </div>
        </form>
      )}

      {confirmRequest && (
        <ConfirmDialog
          title={confirmRequest.title}
          message={confirmRequest.message}
          confirmLabel={confirmRequest.confirmLabel}
          danger={confirmRequest.danger}
          onConfirm={confirmRequest.onConfirm}
          onCancel={() => setConfirmRequest(null)}
        />
      )}

      <div
        ref={scrollerRef}
        className={styles.scroller}
        onWheel={(event) => {
          if (!event.ctrlKey) return;
          event.preventDefault();
          applyZoom(zoom * (event.deltaY > 0 ? 0.9 : 1.1), event.clientX);
        }}
      >
        <div
          data-arrange-timeline
          className={styles.timeline}
          style={{ width: TRACK_HEADER_WIDTH + timelineWidth }}
          onPointerDown={editor.beginMarquee}
        >
          <ArrangeRuler
            timebase={timebase}
            timelineTicks={timelineTicks}
            timelineWidth={timelineWidth}
            pixelsPerTick={pixelsPerTick}
            mode={rulerMode}
            position={formatMusicalPosition(displayTick, timebase)}
            clock={formatClock(displayTick, timebase)}
            scrollTop={scrollTop}
            loopRange={ruler.loopPreview ?? arrangement.loopRange}
            punchRange={ruler.punchPreview ?? arrangement.punchRange}
            markers={arrangement.markers}
            selectedMarkerId={ruler.selectedMarkerId}
            selectedRange={ruler.selectedRange}
            timeSelection={ruler.timeSelection}
            onPointerDown={ruler.seekFromRuler}
            onLoopHandle={ruler.dragLoopHandle}
            onPunchHandle={ruler.dragPunchHandle}
            onSelectRange={ruler.selectRange}
            onRulerContextMenu={openRulerContextMenu}
            onRangeContextMenu={openRangeContextMenu}
            onMarkerContextMenu={openMarkerContextMenu}
            onAddMarker={ruler.addMarkerAt}
            onMoveMarker={ruler.moveMarker}
            onRenameMarker={ruler.renameMarker}
            onRemoveMarker={ruler.removeMarker}
            onSelectMarker={ruler.selectMarker}
          />
          <div
            data-timeline-grid
            aria-hidden="true"
            className={styles.timelineGrid}
            style={timelineGridStyle}
          />
          {transport && transport.recordingPhase !== 'idle' && (
            <div
              className={styles.recordingPreview}
              style={{
                left: TRACK_HEADER_WIDTH + transport.recordingStartTick * pixelsPerTick,
                width: Math.max(1, displayTick - transport.recordingStartTick) * pixelsPerTick,
              }}
            >
              {transport.recordingPhase.toUpperCase()} ·{' '}
              {transport.armedTrackIds
                .map(
                  (trackId) =>
                    arrangement.tracks.find((track) => track.id === trackId)?.name ?? trackId,
                )
                .join(' · ')}{' '}
              · PASS {transport.recordingPassOrdinal}
            </div>
          )}
          <ArrangePlayhead
            positionRef={displayTickRef}
            positionTick={displayTick}
            pixelsPerTick={pixelsPerTick}
            playing={transport?.state === 'playing'}
          />
          {ruler.timeSelection && (
            <div
              data-time-selection-chip
              className={styles.selectionChip}
              style={{
                left:
                  TRACK_HEADER_WIDTH +
                  ((ruler.timeSelection.startTick + ruler.timeSelection.endTick) / 2) *
                    pixelsPerTick,
              }}
            >
              <span>
                {formatMusicalPosition(ruler.timeSelection.startTick, timebase)} →{' '}
                {formatMusicalPosition(ruler.timeSelection.endTick, timebase)}
              </span>
              <button onClick={ruler.setLoopToSelection}>Set Loop</button>
              <button onClick={ruler.setPunchToSelection}>Set Punch</button>
            </div>
          )}
          {editor.snapGuide != null && (
            <div
              className={styles.snapGuide}
              style={{ left: TRACK_HEADER_WIDTH + editor.snapGuide * pixelsPerTick }}
            />
          )}
          {editor.marquee && <div className={styles.marquee} style={editor.marquee} />}

          {arrangement.tracks.length === 0 ? (
            <div
              className={`${styles.empty} ${emptyDragOver ? styles.emptyDragOver : ''}`}
              onDragOver={(event) => {
                if (!event.dataTransfer.types.includes(RIFFRA_ASSET_MIME) && !isOsFileDrag(event))
                  return;
                event.preventDefault();
                event.dataTransfer.dropEffect = 'copy';
              }}
              onDragEnter={() => setEmptyDragOver(true)}
              onDragLeave={(event) => {
                if (!event.currentTarget.contains(event.relatedTarget as Node))
                  setEmptyDragOver(false);
              }}
              onDrop={(event) => {
                setEmptyDragOver(false);
                handleDrop(event);
              }}
            >
              <span className={styles.emptyIcon}>≋</span>
              <strong>Start arranging</strong>
              <div className={styles.emptyActions}>
                <button onClick={() => void editor.commit(props.api.addTrack('Audio 1', 'audio'))}>
                  ＋ Add Audio Track
                </button>
                <button
                  onClick={() =>
                    void editor.commit(props.api.addTrack('Instrument 1', 'instrument'))
                  }
                >
                  ＋ Add Instrument Track
                </button>
              </div>
            </div>
          ) : (
            arrangement.tracks.map((track, trackIndex) => (
              <Fragment key={track.id}>
                <ArrangeTrack
                  track={track}
                  trackIndex={trackIndex}
                  timeline={buildTrackTimeline(
                    track.id,
                    arrangement.audioClips,
                    arrangement.midiClips,
                    timebase,
                  )}
                  timebase={timebase}
                  analyses={analyses}
                  selectedClipIds={selectedClipIds}
                  unavailableClipIds={transport?.unavailableClipIds ?? []}
                  selected={
                    props.selection.kind === 'track' && props.selection.trackId === track.id
                  }
                  focused={props.focusedTrackId === track.id}
                  onSelectTrack={() => {
                    ruler.clearSelectedRange();
                    props.setSelection({ kind: 'track', trackId: track.id });
                    props.onFocusTrack(track.kind === 'instrument' ? track.id : null);
                  }}
                  timelineWidth={timelineWidth}
                  pixelsPerTick={pixelsPerTick}
                  trackSize={trackSizes[track.id] ?? trackSize}
                  api={props.api}
                  onCommit={editor.commit}
                  onDrop={(event, trackId, trackKind) => {
                    handleDrop(event, trackId, trackKind);
                  }}
                  onContextMenu={openTrackLaneContextMenu}
                  onDoubleClickLane={
                    track.kind === 'instrument'
                      ? (event, trackId, tick) => {
                          event.preventDefault();
                          void createEmptyMidiClip(trackId, tick);
                        }
                      : undefined
                  }
                  onMove={editor.beginMove}
                  onMoveMidi={editor.beginMidiMove}
                  onTrimMidi={editor.beginMidiTrim}
                  onSelect={(clipId, append = false) => {
                    ruler.clearSelectedRange();
                    editor.selectClip(clipId, append);
                    const selectedMidiClip = arrangement.midiClips.find(
                      (clip) => clip.id === clipId,
                    );
                    if (selectedMidiClip && !append && detail.view === 'midiEditor')
                      detail.keepSelectedMidiClipVisible(clipId);
                  }}
                  onTrim={editor.beginTrim}
                  onFade={editor.beginFade}
                  onOpenMidiEditor={(clip) => {
                    detail.openMidiEditor(clip);
                  }}
                  onAudioClipContextMenu={openAudioClipContextMenu}
                  onMidiClipContextMenu={openMidiClipContextMenu}
                  onRename={(name) => void editor.commit(props.api.updateTrack(track.id, { name }))}
                  onDuplicate={() => void editor.commit(props.api.duplicateTrack(track.id))}
                  onDelete={() =>
                    void deleteTrack(track.id, track.name, trackClipCounts.get(track.id) ?? 0)
                  }
                  missingDeviceIds={[
                    ...(props.missingDeviceIds ?? []),
                    ...(transport?.missingDeviceIds ?? []),
                  ]}
                  onAddDevice={() =>
                    setPluginPicker({
                      trackId: track.id,
                      kind: track.kind === 'audio' ? 'effect' : 'instrument',
                    })
                  }
                  onOpenPluginEditor={(deviceId) => {
                    void props.api
                      .openTrackPluginEditor(track.id, deviceId)
                      .catch((error: unknown) => {
                        editor.setMessage(error instanceof Error ? error.message : String(error));
                      });
                  }}
                  onReorder={(sourceTrackId, insertAfter) => {
                    const sourceIndex = arrangement.tracks.findIndex(
                      (candidate) => candidate.id === sourceTrackId,
                    );
                    if (sourceIndex < 0 || sourceIndex === trackIndex) return;
                    const targetIndex = insertAfter
                      ? sourceIndex < trackIndex
                        ? trackIndex
                        : trackIndex + 1
                      : sourceIndex < trackIndex
                        ? trackIndex - 1
                        : trackIndex;
                    void editor.commit(props.api.reorderTrack(sourceTrackId, targetIndex));
                  }}
                  onSetTrackSize={(size) => setTrackSizeForTrack(track.id, size)}
                />
                {automationParameters[track.id] && (
                  <AutomationLaneView
                    track={track}
                    lane={arrangement.automationLanes.find(
                      (lane) =>
                        lane.trackId === track.id &&
                        lane.parameter === automationParameters[track.id],
                    )}
                    parameter={automationParameters[track.id]!}
                    timelineWidth={timelineWidth}
                    pixelsPerTick={pixelsPerTick}
                    snapTick={editor.snapTick}
                    onParameter={(parameter) =>
                      setAutomationParameters((current) => ({
                        ...current,
                        [track.id]: parameter,
                      }))
                    }
                    onCommit={(points) =>
                      void editor.commit(
                        props.api.setTrackAutomation(
                          track.id,
                          automationParameters[track.id]!,
                          points,
                        ),
                      )
                    }
                  />
                )}
              </Fragment>
            ))
          )}
        </div>
      </div>

      <ArrangeDetailArea
        view={detail.view}
        height={detail.height}
        collapsed={detail.collapsed}
        maximized={detail.maximized}
        onCollapsedChange={detail.setCollapsed}
        onHeightChange={detail.setHeight}
        collapsedControls={detailControls}
        midiEditor={
          <MidiEditorPanel
            clip={activeMidiClip}
            timebase={timebase}
            playheadTick={displayTick}
            playheadTickRef={displayTickRef}
            playing={transport?.state === 'playing'}
            onSeek={seekMidiEditor}
            previewAvailable={midiPreviewAvailable}
            onSendMidi={sendMidiPreview}
            onPanicMidi={panicMidiPreview}
            toolbarTrailing={detail.collapsed ? null : detailControls}
            onAddNote={(clipId, startTick, pitch, durationTicks, velocity, channel) =>
              commitMidiEdit(
                props.api.addMidiNote(
                  clipId,
                  Math.max(0, Math.round(startTick)),
                  pitch,
                  Math.max(1, Math.round(durationTicks)),
                  velocity,
                  channel,
                ),
              )
            }
            onUpdateNote={(clipId, note) =>
              commitMidiEdit(
                props.api.updateMidiNote(clipId, note.id, {
                  note: note.note,
                  startTick: note.startTick,
                  durationTicks: note.durationTicks,
                  velocity: note.velocity,
                }),
              )
            }
            onUpdateNotes={(clipId, updates) =>
              commitMidiEdit(
                props.api.updateMidiNotes(
                  clipId,
                  updates.map((update) => ({
                    noteId: update.noteId,
                    patch: {
                      note: update.patch.note,
                      startTick: update.patch.startTick,
                      durationTicks: update.patch.durationTicks,
                      velocity: update.patch.velocity,
                    },
                  })),
                ),
              )
            }
            onRemoveNotes={(clipId, noteIds) =>
              commitMidiEdit(props.api.removeMidiNotes(clipId, noteIds))
            }
            onInsertNotes={(clipId, notes) =>
              commitMidiEdit(props.api.insertMidiNotes(clipId, notes))
            }
            onQuantize={(clipId, noteIds, gridTicks) =>
              editor.commit(props.api.quantizeMidiNotes(clipId, noteIds, gridTicks))
            }
            onDuplicateNotes={(clipId, noteIds, offsetTicks) =>
              editor.commit(props.api.duplicateMidiNotes(clipId, noteIds, offsetTicks))
            }
          />
        }
      />

      <PlaySurfacePanel
        host={props.playSurfaceHost}
        mode={playSurfaceMode}
        track={focusedTrack}
        summary={playSurfaceSummary}
        onModeChange={setPlaySurfaceMode}
        audio={props.audio}
        api={props.api}
        runtimeReady={runtimeReady}
        missingDeviceIds={[
          ...(props.missingDeviceIds ?? []),
          ...(transport?.missingDeviceIds ?? []),
        ]}
        onChooseInstrument={() => {
          if (focusedTrack) setPluginPicker({ trackId: focusedTrack.id, kind: 'instrument' });
        }}
        onSummaryChange={setPlaySurfaceSummary}
      />

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenu.items}
          onClose={() => setContextMenu(null)}
        />
      )}
    </section>
  );
}
