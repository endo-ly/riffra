import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type CSSProperties,
  type MutableRefObject,
} from 'react';
import type {
  AudioClip,
  AutomationParameter,
  AudioStatus,
  CreativeSession,
  Marker,
  MidiClip,
  PluginEntry,
  TrackKind,
} from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { ArrangeRuler } from './ArrangeRuler';
import { ArrangeToolbar } from './ArrangeToolbar';
import { ArrangeTrack } from './ArrangeTrack';
import { AutomationLaneView } from './AutomationLaneView';
import { MidiEditorPanel } from './MidiEditorPanel';
import { ArrangeLowerPanel, type ArrangeLowerPanelView } from './lower-panel/ArrangeLowerPanel';
import { PlaySurfacePanel } from './lower-panel/PlaySurfacePanel';
import { PluginPicker } from './PluginPicker';
import { ContextMenu, type ContextMenuItem } from '../shared/ContextMenu';
import { ConfirmDialog } from '../shared/ConfirmDialog';
import {
  BASE_PIXELS_PER_QUARTER,
  buildTrackTimeline,
  timelineObjectEndTick,
  clipDurationTicks,
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
} from '@/lib/arrange-timeline';
import { RIFFRA_ASSET_MIME } from '@/lib/arrange-drag';
import { isEditableTarget } from '@/lib/interaction';
import { useArrangeEditor, type ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';
import { useArrangeTransport } from '@/hooks/arrange/useArrangeTransport';
import { useWaveformAnalyses } from '@/hooks/arrange/useWaveformAnalyses';
import styles from './WorkspaceArrange.module.css';

interface WorkspaceArrangeProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selection: ArrangeSelection;
  setSelection: (selection: ArrangeSelection) => void;
  api: NativeApi;
  audio: AudioStatus;
  focusedTrackId: string | null;
  onFocusTrack: (trackId: string | null) => void;
  canonicalOperationPending?: boolean;
  missingDeviceIds?: string[];
  plugins?: PluginEntry[];
  onRecord?: () => void;
  recordingActive?: boolean;
}

export function WorkspaceArrange(props: WorkspaceArrangeProps) {
  const { arrangement } = props.session;
  const { timebase } = arrangement;
  const { api } = props;
  const [tool, setTool] = useState<ArrangeTool>('select');
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [zoom, setZoom] = useState(1);
  const trackSize: TrackSize = 'normal';
  const [trackSizes, setTrackSizes] = useState<Record<string, TrackSize>>({});
  const [automationParameters, setAutomationParameters] = useState<
    Partial<Record<string, AutomationParameter>>
  >({});
  const [rulerMode, setRulerMode] = useState<'bars' | 'time'>('bars');
  const [follow, setFollow] = useState(true);
  const [selectedMarkerId, setSelectedMarkerId] = useState<string | null>(null);
  const [selectedRange, setSelectedRange] = useState<'loop' | 'punch' | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    items: ContextMenuItem[];
  } | null>(null);
  const [markerRename, setMarkerRename] = useState<{ markerId: string; name: string } | null>(null);
  const [confirmRequest, setConfirmRequest] = useState<{
    title: string;
    message: string;
    confirmLabel?: string;
    danger?: boolean;
    onConfirm: () => void;
  } | null>(null);
  const [timeSelection, setTimeSelection] = useState<{ startTick: number; endTick: number } | null>(
    null,
  );
  const [loopPreview, setLoopPreview] = useState<{
    enabled: boolean;
    startTick: number;
    endTick: number;
  } | null>(null);
  const [punchPreview, setPunchPreview] = useState<{
    startTick: number;
    endTick: number;
  } | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [activeMidiClipId, setActiveMidiClipId] = useState<string | null>(null);
  const [lowerPanelView, setLowerPanelView] = useState<ArrangeLowerPanelView>('closed');
  const [lowerPanelCollapsed, setLowerPanelCollapsed] = useState(false);
  const [lowerPanelHeight, setLowerPanelHeight] = useState(260);
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
  );
  const analyses = useWaveformAnalyses(props.api, arrangement.audioClips);
  const pixelsPerTick = (BASE_PIXELS_PER_QUARTER * zoom) / timebase.ppq;
  const isOsFileDrag = (event: DragEvent) => event.dataTransfer.types.includes('Files');
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
  const commitMidiEdit = (operation: Promise<CreativeSession | null>) => commit(operation);
  const canonicalOperationPending =
    editor.canonicalOperationPending || Boolean(props.canonicalOperationPending);
  // Accept Standard MIDI Files dragged from the operating system. HTML5 drop
  // delivers the file contents rather than the OS path, so the bytes are
  // imported as a canonical MIDI Asset and then placed as a MIDI Clip.
  const handleOsMidiDrop = useCallback(
    async (files: FileList, trackId?: string, trackKind?: TrackKind): Promise<void> => {
      if (trackKind === 'audio') {
        setMessage('MIDI Assets can only be placed on an Instrument Track.');
        return;
      }
      for (const file of Array.from(files)) {
        if (!/\.midi?$/i.test(file.name)) continue;
        const stem = file.name.replace(/\.(mid|midi)$/i, '');
        try {
          const assetId = await api.importMidiBytes(
            stem,
            Array.from(new Uint8Array(await file.arrayBuffer())),
          );
          if (!assetId) continue;
          await commit(api.addMidiClipToArrangement(assetId, stem, undefined, trackId));
        } catch {
          /* import or placement failure surfaces through the library notice path */
        }
      }
    },
    [api, commit, setMessage],
  );
  const [revisionMismatchOutOfSync, setRevisionMismatchOutOfSync] = useState(false);
  const revisionMismatch = Boolean(transport && transport.revision !== arrangement.revision);

  // Arrangement edits reach the audio runtime asynchronously, so a transport
  // revision mismatch is expected briefly after a canonical response.
  useEffect(() => {
    if (!revisionMismatch || canonicalOperationPending) {
      setRevisionMismatchOutOfSync(false);
      return;
    }
    const timeout = window.setTimeout(() => setRevisionMismatchOutOfSync(true), 1_000);
    return () => window.clearTimeout(timeout);
  }, [arrangement.revision, canonicalOperationPending, revisionMismatch, transport?.revision]);

  const playbackOutOfSync = editor.runtimeOutOfSync || revisionMismatchOutOfSync;
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
  const activeMidiClip = arrangement.midiClips.find((clip) => clip.id === activeMidiClipId) ?? null;
  const runtimeReady =
    !playbackOutOfSync &&
    props.audio.state !== 'starting' &&
    props.audio.state !== 'faulted' &&
    props.audio.state !== 'offline';
  const statusMessage = playbackOutOfSync
    ? 'Playback runtime is out of sync'
    : unavailableClipCount || missingDeviceCount
      ? `Playback skipped ${unavailableClipCount} missing source${unavailableClipCount === 1 ? '' : 's'} and ${missingDeviceCount} missing device${missingDeviceCount === 1 ? '' : 's'}.`
      : editor.message;
  const showStatus = Boolean(statusMessage);

  const clearRange = useCallback(
    (range: 'loop' | 'punch') => {
      const operation =
        range === 'loop'
          ? api.updateTimelineLoopRange(
              false,
              arrangement.loopRange.startTick,
              arrangement.loopRange.endTick,
            )
          : api.updateTimelinePunchRange(
              false,
              arrangement.punchRange?.startTick ?? 0,
              arrangement.punchRange?.endTick ?? 0,
            );
      void commit(operation);
    },
    [api, arrangement.loopRange, arrangement.punchRange, commit],
  );

  const selectRange = (range: 'loop' | 'punch') => {
    setSelectedRange(range);
    setSelectedMarkerId(null);
  };

  useEffect(() => {
    if (activeMidiClipId !== null && !activeMidiClip) {
      setActiveMidiClipId(null);
      if (lowerPanelView === 'midiEditor') setLowerPanelView('closed');
    }
  }, [activeMidiClip, activeMidiClipId, lowerPanelView]);

  const openPlaySurface = (trackId: string) => {
    props.setSelection({ kind: 'track', trackId });
    props.onFocusTrack(trackId);
    setLowerPanelView('playSurface');
    setLowerPanelCollapsed(false);
  };

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

  // Follow Playhead: during playback, keep the playhead in view. Manual scroll
  // pauses follow until the user seeks via the ruler or re-enables the toggle.
  useEffect(() => {
    if (!follow || transport?.state !== 'playing') return;
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const playheadX = TRACK_HEADER_WIDTH + displayTick * pixelsPerTick;
    const left = scroller.scrollLeft;
    const right = left + scroller.clientWidth;
    const margin = Math.min(160, scroller.clientWidth * 0.18);
    if (playheadX < left + margin || playheadX > right - margin) {
      const target = Math.max(0, playheadX - scroller.clientWidth * 0.32);
      programmaticScrollRef.current = true;
      scroller.scrollLeft = target;
    }
  }, [displayTick, follow, pixelsPerTick, transport?.state]);

  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    let frame = 0;
    const onScroll = () => {
      if (programmaticScrollRef.current) {
        programmaticScrollRef.current = false;
        return;
      }
      if (frame) {
        cancelAnimationFrame(frame);
        frame = 0;
      }
      frame = requestAnimationFrame(() => {
        if (transport?.state === 'playing') setFollow(false);
      });
    };
    scroller.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      scroller.removeEventListener('scroll', onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [transport?.state]);

  // Keyboard: Delete removes the selected Marker or Loop/Punch range when no Clips are selected.
  // Escape clears the ruler selection.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === 'Escape') {
        if (isEditableTarget(event.target)) return;
        setTimeSelection(null);
        setSelectedMarkerId(null);
        setSelectedRange(null);
        return;
      }
      if (event.key === 'Delete' && selectedRange && selectedClipIds.length === 0) {
        if (isEditableTarget(event.target)) return;
        const rangeIsActive =
          selectedRange === 'loop'
            ? arrangement.loopRange.enabled
            : Boolean(arrangement.punchRange);
        if (!rangeIsActive) {
          setSelectedRange(null);
          return;
        }
        event.preventDefault();
        clearRange(selectedRange);
        setSelectedRange(null);
        return;
      }
      if (event.key === 'Delete' && selectedMarkerId && selectedClipIds.length === 0) {
        if (isEditableTarget(event.target)) return;
        const marker = arrangement.markers.find((item) => item.id === selectedMarkerId);
        if (!marker) return;
        event.preventDefault();
        void commit(api.removeMarker(marker.id)).then((next) => {
          if (!next) return;
          setSelectedMarkerId(null);
        });
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [
    selectedMarkerId,
    selectedRange,
    selectedClipIds.length,
    arrangement.markers,
    arrangement.loopRange.enabled,
    arrangement.punchRange,
    api,
    clearRange,
    commit,
  ]);

  // Close the time selection chip when clicking outside the ruler or the chip itself.
  useEffect(() => {
    if (!timeSelection) return;
    const onClick = (event: MouseEvent) => {
      const target = event.target as HTMLElement;
      if (target.closest('[data-time-selection-chip]')) return;
      if (target.closest('[data-arrange-ruler]')) return;
      setTimeSelection(null);
    };
    document.addEventListener('click', onClick);
    return () => document.removeEventListener('click', onClick);
  }, [timeSelection]);

  const seekFromRuler = (event: React.PointerEvent<HTMLDivElement>) => {
    if (
      (event.target as HTMLElement).closest(
        '[data-marker-id], [data-range-band], [data-range-handle]',
      )
    )
      return;
    setSelectedRange(null);
    const bounds = event.currentTarget.getBoundingClientRect();
    const originTick = editor.snapTick((event.clientX - bounds.left) / pixelsPerTick, event.altKey);
    const originX = event.clientX;
    let seeking = true;
    seekLocally(originTick);
    void props.api.seekTimeline(originTick).catch((error) => editor.setMessage(String(error)));
    setFollow(true);
    const handle = (move: PointerEvent) => {
      const tick = editor.snapTick((move.clientX - bounds.left) / pixelsPerTick, move.altKey);
      if (seeking && Math.abs(move.clientX - originX) > 4) {
        seeking = false;
        setTimeSelection({
          startTick: Math.min(originTick, tick),
          endTick: Math.max(originTick, tick),
        });
        return;
      }
      if (seeking) {
        seekLocally(tick);
        void props.api.seekTimeline(tick).catch((error) => editor.setMessage(String(error)));
      } else {
        setTimeSelection((current) =>
          current
            ? { startTick: Math.min(originTick, tick), endTick: Math.max(originTick, tick) }
            : null,
        );
      }
    };
    const finish = () => {
      window.removeEventListener('pointermove', handle);
      window.removeEventListener('pointerup', finish);
      if (seeking) setTimeSelection(null);
    };
    window.addEventListener('pointermove', handle);
    window.addEventListener('pointerup', finish);
  };

  const dragLoopHandle = (
    event: React.PointerEvent<HTMLSpanElement>,
    boundary: 'start' | 'end',
  ) => {
    event.stopPropagation();
    event.preventDefault();
    const originX = event.clientX;
    const range = arrangement.loopRange;
    const origin = boundary === 'start' ? range.startTick : range.endTick;
    const computeNext = (clientX: number, altKey: boolean) =>
      editor.snapTick(origin + (clientX - originX) / pixelsPerTick, altKey);
    const applyPreview = (clientX: number, altKey: boolean) => {
      const next = computeNext(clientX, altKey);
      setLoopPreview({
        enabled: range.enabled,
        startTick: boundary === 'start' ? next : range.startTick,
        endTick: boundary === 'end' ? next : range.endTick,
      });
    };
    const move = (pointer: PointerEvent) => applyPreview(pointer.clientX, pointer.altKey);
    const finish = (pointer: PointerEvent) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      const next = computeNext(pointer.clientX, pointer.altKey);
      if (next !== origin) {
        void editor
          .commit(
            props.api.updateTimelineLoopRange(
              range.enabled,
              boundary === 'start' ? next : range.startTick,
              boundary === 'end' ? next : range.endTick,
            ),
          )
          .finally(() => setLoopPreview(null));
      } else {
        setLoopPreview(null);
      }
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    applyPreview(event.clientX, event.altKey);
  };

  const dragPunchHandle = (
    event: React.PointerEvent<HTMLSpanElement>,
    boundary: 'start' | 'end',
  ) => {
    event.stopPropagation();
    event.preventDefault();
    const originX = event.clientX;
    const range = arrangement.punchRange;
    if (!range) return;
    const origin = boundary === 'start' ? range.startTick : range.endTick;
    const computeNext = (clientX: number, altKey: boolean) =>
      editor.snapTick(origin + (clientX - originX) / pixelsPerTick, altKey);
    const applyPreview = (clientX: number, altKey: boolean) => {
      const next = computeNext(clientX, altKey);
      setPunchPreview({
        startTick: boundary === 'start' ? next : range.startTick,
        endTick: boundary === 'end' ? next : range.endTick,
      });
    };
    const move = (pointer: PointerEvent) => applyPreview(pointer.clientX, pointer.altKey);
    const finish = (pointer: PointerEvent) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      const next = computeNext(pointer.clientX, pointer.altKey);
      if (next !== origin) {
        void editor
          .commit(
            api.updateTimelinePunchRange(
              true,
              boundary === 'start' ? next : range.startTick,
              boundary === 'end' ? next : range.endTick,
            ),
          )
          .finally(() => setPunchPreview(null));
      } else {
        setPunchPreview(null);
      }
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    applyPreview(event.clientX, event.altKey);
  };

  const setLoopToSelection = () => {
    if (!timeSelection) return;
    void commit(
      props.api.updateTimelineLoopRange(true, timeSelection.startTick, timeSelection.endTick),
    );
  };

  const setPunchToSelection = () => {
    if (!timeSelection) return;
    void commit(
      props.api.updateTimelinePunchRange(true, timeSelection.startTick, timeSelection.endTick),
    );
  };

  const addMarkerAt = (tick: number) => {
    const existing = new Set(arrangement.markers.map((marker) => marker.id));
    void editor
      .commit(api.addMarker(editor.snapTick(tick), `Marker ${arrangement.markers.length + 1}`))
      .then((next) => {
        if (!next) return;
        const created = next.arrangement.markers.find((marker) => !existing.has(marker.id));
        if (created) setSelectedMarkerId(created.id);
      });
  };

  const renameMarker = (marker: Marker) => {
    setMarkerRename({ markerId: marker.id, name: marker.name });
  };

  const removeMarker = (marker: Marker) => {
    void commit(props.api.removeMarker(marker.id)).then((next) => {
      if (!next) return;
      if (selectedMarkerId === marker.id) setSelectedMarkerId(null);
    });
  };

  const saveMarkerRename = () => {
    if (!markerRename) return;
    const marker = arrangement.markers.find((item) => item.id === markerRename.markerId);
    const next = markerRename.name.trim();
    setMarkerRename(null);
    if (marker && next && next !== marker.name) {
      void editor.commit(props.api.updateMarker(marker.id, { name: next }));
    }
  };

  const openRulerContextMenu = (event: React.MouseEvent<HTMLDivElement>, tick: number) => {
    event.preventDefault();
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Add Marker Here', onClick: () => addMarkerAt(tick) },
        { label: 'Set Loop to Selection', onClick: setLoopToSelection, disabled: !timeSelection },
        { label: 'Set Punch Range', onClick: setPunchToSelection, disabled: !timeSelection },
        { separator: true },
        {
          label: 'Clear Loop',
          onClick: () => clearRange('loop'),
          disabled: !arrangement.loopRange.enabled,
        },
        {
          label: 'Clear Punch',
          onClick: () => clearRange('punch'),
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
    selectRange(range);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            clearRange(range);
            setSelectedRange(null);
          },
        },
      ],
    });
  };

  const openMarkerContextMenu = (event: React.MouseEvent, marker: Marker) => {
    event.preventDefault();
    setSelectedMarkerId(marker.id);
    setSelectedRange(null);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Rename', onClick: () => renameMarker(marker) },
        { label: 'Delete', danger: true, onClick: () => removeMarker(marker) },
      ],
    });
  };

  const performDeleteTrack = async (trackId: string) => {
    const deletedTrack = arrangement.tracks.find((track) => track.id === trackId);
    if (
      props.focusedTrackId === trackId &&
      deletedTrack?.kind === 'instrument' &&
      deletedTrack.instrument
    ) {
      try {
        const status = await props.api.panicMidiTrack(trackId);
        if (status) editor.setMessage(status.message);
      } catch (error) {
        editor.setMessage(String(error));
      }
    }
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
          onClick: () => {
            setContextMenu(null);
            void editor.commit(
              api.quantizeMidiNotes(
                clip.id,
                clip.notes.map((note) => note.id),
                gridTicks,
              ),
            );
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

  const cycleTrackSize = (trackId: string) => {
    const sizes: TrackSize[] = ['compact', 'normal', 'large'];
    const current = trackSizes[trackId] ?? trackSize;
    setTrackSizes((value) => ({
      ...value,
      [trackId]: sizes[(sizes.indexOf(current) + 1) % sizes.length],
    }));
  };
  const setTrackSizeForTrack = (trackId: string, size: TrackSize) => {
    setTrackSizes((value) => ({ ...value, [trackId]: size }));
  };
  const toggleAutomation = (trackId: string) =>
    setAutomationParameters((current) => ({
      ...current,
      [trackId]: current[trackId] ? undefined : 'volume',
    }));

  const openTrackLaneContextMenu = (event: React.MouseEvent, trackId: string, _tick: number) => {
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
      style={{ '--header-width': `${TRACK_HEADER_WIDTH}px` } as CSSProperties}
    >
      <ArrangeToolbar
        tool={tool}
        snap={snap}
        zoom={zoom}
        rulerMode={rulerMode}
        follow={follow}
        onTool={setTool}
        onSnap={setSnap}
        onZoom={applyZoom}
        onRulerMode={setRulerMode}
        onFollow={setFollow}
        automationAvailable={selectedTrackId !== null}
        automationOpen={selectedTrackId !== null && Boolean(automationParameters[selectedTrackId])}
        onToggleAutomation={() => {
          if (selectedTrackId) toggleAutomation(selectedTrackId);
        }}
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

      {markerRename && (
        <form
          className={styles.markerDialog}
          aria-label="Rename marker"
          onSubmit={(event) => {
            event.preventDefault();
            saveMarkerRename();
          }}
        >
          <strong>Rename Marker</strong>
          <label>
            <span>Name</span>
            <input
              autoFocus
              value={markerRename.name}
              onChange={(event) => {
                const value = event.currentTarget.value;
                setMarkerRename((current) => (current ? { ...current, name: value } : current));
              }}
              onKeyDown={(event) => {
                if (event.key === 'Escape') setMarkerRename(null);
              }}
            />
          </label>
          <div>
            <button type="button" onClick={() => setMarkerRename(null)}>
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
            loopRange={loopPreview ?? arrangement.loopRange}
            punchRange={punchPreview ?? arrangement.punchRange}
            markers={arrangement.markers}
            selectedMarkerId={selectedMarkerId}
            selectedRange={selectedRange}
            timeSelection={timeSelection}
            onPointerDown={seekFromRuler}
            onLoopHandle={dragLoopHandle}
            onPunchHandle={dragPunchHandle}
            onSelectRange={selectRange}
            onRulerContextMenu={openRulerContextMenu}
            onRangeContextMenu={openRangeContextMenu}
            onMarkerContextMenu={openMarkerContextMenu}
            onAddMarker={addMarkerAt}
            onMoveMarker={(marker, tick) =>
              void editor.commit(props.api.updateMarker(marker.id, { tick: editor.snapTick(tick) }))
            }
            onRenameMarker={renameMarker}
            onRemoveMarker={removeMarker}
            onSelectMarker={(markerId) => {
              setSelectedMarkerId(markerId);
              setSelectedRange(null);
            }}
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
            pixelsPerTick={pixelsPerTick}
            playing={transport?.state === 'playing'}
          />
          {timeSelection && (
            <div
              data-time-selection-chip
              className={styles.selectionChip}
              style={{
                left:
                  TRACK_HEADER_WIDTH +
                  ((timeSelection.startTick + timeSelection.endTick) / 2) * pixelsPerTick,
              }}
            >
              <span>
                {formatMusicalPosition(timeSelection.startTick, timebase)} →{' '}
                {formatMusicalPosition(timeSelection.endTick, timebase)}
              </span>
              <button onClick={setLoopToSelection}>Set Loop</button>
              <button onClick={setPunchToSelection}>Set Punch</button>
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
                if (event.dataTransfer.files?.length) {
                  void handleOsMidiDrop(event.dataTransfer.files);
                  return;
                }
                void editor.dropAsset(event);
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
                {props.onRecord && (
                  <button
                    className={styles.emptyRecord}
                    onClick={() => props.onRecord?.()}
                    title="Arm a Track or drop an Asset to start recording"
                  >
                    ● Record
                  </button>
                )}
              </div>
            </div>
          ) : (
            arrangement.tracks.map((track, trackIndex) => (
              <Fragment key={track.id}>
                <ArrangeTrack
                  track={track}
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
                    setSelectedRange(null);
                    props.setSelection({ kind: 'track', trackId: track.id });
                    props.onFocusTrack(track.kind === 'instrument' ? track.id : null);
                  }}
                  onOpenPlaySurface={
                    track.kind === 'instrument' ? () => openPlaySurface(track.id) : undefined
                  }
                  timelineWidth={timelineWidth}
                  pixelsPerTick={pixelsPerTick}
                  trackSize={trackSizes[track.id] ?? trackSize}
                  api={props.api}
                  onCommit={editor.commit}
                  onDrop={(event, trackId, trackKind) => {
                    if (event.dataTransfer.files?.length) {
                      void handleOsMidiDrop(event.dataTransfer.files, trackId, trackKind);
                      return;
                    }
                    void editor.dropAsset(event, trackId, trackKind);
                  }}
                  onContextMenu={openTrackLaneContextMenu}
                  onMove={editor.beginMove}
                  onMoveMidi={editor.beginMidiMove}
                  onTrimMidi={editor.beginMidiTrim}
                  onSelect={(clipId) => {
                    setSelectedRange(null);
                    editor.selectClip(clipId);
                  }}
                  onTrim={editor.beginTrim}
                  onFade={editor.beginFade}
                  onOpenMidiEditor={(clip) => {
                    setActiveMidiClipId(clip.id);
                    setLowerPanelView('midiEditor');
                    setLowerPanelCollapsed(false);
                  }}
                  onAudioClipContextMenu={openAudioClipContextMenu}
                  onMidiClipContextMenu={openMidiClipContextMenu}
                  onRename={(name) => void editor.commit(props.api.updateTrack(track.id, { name }))}
                  onDuplicate={() => void editor.commit(props.api.duplicateTrack(track.id))}
                  onDelete={() =>
                    void deleteTrack(track.id, track.name, trackClipCounts.get(track.id) ?? 0)
                  }
                  onReorder={(sourceTrackId) =>
                    void editor.commit(props.api.reorderTrack(sourceTrackId, trackIndex))
                  }
                  onResize={() => cycleTrackSize(track.id)}
                  onSetTrackSize={(size) => setTrackSizeForTrack(track.id, size)}
                  automationOpen={Boolean(automationParameters[track.id])}
                  onToggleAutomation={() => toggleAutomation(track.id)}
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

      <ArrangeLowerPanel
        view={lowerPanelView}
        collapsed={lowerPanelCollapsed}
        height={lowerPanelHeight}
        activeMidiClip={activeMidiClip}
        onViewChange={(view) => {
          setLowerPanelView(view);
          setLowerPanelCollapsed(false);
        }}
        onCollapsedChange={setLowerPanelCollapsed}
        onHeightChange={setLowerPanelHeight}
        playSurfaceSummary={playSurfaceSummary}
        playSurface={
          <PlaySurfacePanel
            track={focusedTrack}
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
        }
        midiEditor={
          <MidiEditorPanel
            clip={activeMidiClip}
            timebase={timebase}
            onAddNote={(clipId, startTick, pitch) =>
              void editor.commit(
                props.api.addMidiNote(
                  clipId,
                  Math.max(0, Math.round(startTick)),
                  pitch,
                  240,
                  96,
                  1,
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
            onRemoveNote={(clipId, noteId) =>
              void editor.commit(props.api.removeMidiNote(clipId, noteId))
            }
            onQuantize={(clipId, noteIds, gridTicks) =>
              void editor.commit(props.api.quantizeMidiNotes(clipId, noteIds, gridTicks))
            }
            onDuplicateNotes={(clipId, noteIds, offsetTicks) =>
              void editor.commit(props.api.duplicateMidiNotes(clipId, noteIds, offsetTicks))
            }
          />
        }
      />

      {showStatus && (
        <div className={styles.statusToast} role="status">
          <span className={transport?.state === 'playing' ? styles.playingDot : ''} />
          {statusMessage}
          {playbackOutOfSync && (
            <button onClick={() => void editor.retryRuntimeSync()}>Retry</button>
          )}
        </div>
      )}

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

function ArrangePlayhead({
  positionRef,
  pixelsPerTick,
  playing,
}: {
  positionRef: MutableRefObject<number>;
  pixelsPerTick: number;
  playing: boolean;
}) {
  const elementRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const update = () => {
      const element = elementRef.current;
      if (element) {
        element.style.transform = `translate3d(${
          TRACK_HEADER_WIDTH + positionRef.current * pixelsPerTick
        }px, 0, 0)`;
      }
    };
    if (!playing) {
      update();
      return;
    }
    let frame = requestAnimationFrame(function animate() {
      update();
      frame = requestAnimationFrame(animate);
    });
    return () => cancelAnimationFrame(frame);
  }, [pixelsPerTick, playing, positionRef]);

  return (
    <div ref={elementRef} className={styles.playhead}>
      <span />
    </div>
  );
}
