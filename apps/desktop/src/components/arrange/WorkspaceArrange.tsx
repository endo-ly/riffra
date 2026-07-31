import { Fragment, useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from 'react';
import type {
  AudioClip,
  AutomationParameter,
  CreativeSession,
  Marker,
  MidiClip,
  PluginEntry,
} from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { ArrangeRuler } from './ArrangeRuler';
import { ArrangeToolbar } from './ArrangeToolbar';
import { ArrangeTrack } from './ArrangeTrack';
import { AutomationLaneView } from './AutomationLaneView';
import { MidiEditorPanel } from './MidiEditorPanel';
import { PluginPicker } from './PluginPicker';
import { ContextMenu, type ContextMenuItem } from '../shared/ContextMenu';
import {
  BASE_PIXELS_PER_QUARTER,
  timelineObjectEndTick,
  clipDurationTicks,
  formatClock,
  formatMusicalPosition,
  ticksPerBar,
  snapGridTicks,
  TRACK_HEADER_WIDTH,
  type ArrangeTool,
  type SnapGrid,
  type TrackSize,
} from '@/lib/arrange-timeline';
import { RIFFRA_ASSET_MIME } from '@/lib/arrange-drag';
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
  plugins?: PluginEntry[];
  onRecord?: () => void;
  recordingActive?: boolean;
}

export function WorkspaceArrange(props: WorkspaceArrangeProps) {
  const { arrangement } = props.session;
  const { timebase } = arrangement;
  const { api, setSession } = props;
  const [tool, setTool] = useState<ArrangeTool>('select');
  const [snap, setSnap] = useState<SnapGrid>('1/16');
  const [zoom, setZoom] = useState(1);
  const [trackSize, setTrackSize] = useState<TrackSize>('normal');
  const [trackSizes, setTrackSizes] = useState<Record<string, TrackSize>>({});
  const [automationParameters, setAutomationParameters] = useState<
    Partial<Record<string, AutomationParameter>>
  >({});
  const [rulerMode, setRulerMode] = useState<'bars' | 'time'>('bars');
  const [follow, setFollow] = useState(true);
  const [selectedMarkerId, setSelectedMarkerId] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    items: ContextMenuItem[];
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
  const [midiEditorOpen, setMidiEditorOpen] = useState(false);
  const [emptyDragOver, setEmptyDragOver] = useState(false);
  const [pluginPicker, setPluginPicker] = useState<{
    trackId: string;
    kind: 'effect' | 'instrument';
  } | null>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const programmaticScrollRef = useRef(false);
  const { transport, displayTick, seekLocally } = useArrangeTransport(props.api, timebase);
  const analyses = useWaveformAnalyses(props.api, arrangement.audioClips);
  const pixelsPerTick = (BASE_PIXELS_PER_QUARTER * zoom) / timebase.ppq;
  // Accept Standard MIDI Files dragged from the operating system. HTML5 drop
  // delivers the file contents rather than the OS path, so the bytes are
  // imported as a canonical MIDI Asset and then placed as a MIDI Clip.
  const handleOsMidiDrop = useCallback(
    async (files: FileList, trackId?: string): Promise<void> => {
      for (const file of Array.from(files)) {
        if (!/\.midi?$/i.test(file.name)) continue;
        const stem = file.name.replace(/\.(mid|midi)$/i, '');
        try {
          const assetId = await api.importMidiBytes(
            stem,
            Array.from(new Uint8Array(await file.arrayBuffer())),
          );
          if (!assetId) continue;
          const next = await api.addMidiClipToArrangement(assetId, stem, undefined, trackId);
          if (next) setSession(next);
        } catch {
          /* import or placement failure surfaces through the library notice path */
        }
      }
    },
    [api, setSession],
  );
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
  const editor = useArrangeEditor({
    ...props,
    tool,
    snap,
    pixelsPerTick,
    displayTick,
    analyses,
    onSplitToolUsed: () => setTool('select'),
  });
  const playbackOutOfSync =
    editor.runtimeOutOfSync || Boolean(transport && transport.revision !== arrangement.revision);
  const unavailableClipCount = transport?.unavailableClipIds?.length ?? 0;
  const missingDeviceCount = transport?.missingDeviceIds?.length ?? 0;
  const selectedClipIds = props.selection.kind === 'clips' ? props.selection.clipIds : [];
  const selectedTrackId = props.selection.kind === 'track' ? props.selection.trackId : null;

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

  // Keyboard: Delete removes the selected Marker (when no Clips are selected);
  // Escape clears the Time Selection and Marker selection (deselect, not delete).
  useEffect(() => {
    const isEditable = (target: EventTarget | null) => {
      const el = target as HTMLElement | null;
      return (
        !!el &&
        (el.tagName === 'INPUT' ||
          el.tagName === 'TEXTAREA' ||
          el.tagName === 'SELECT' ||
          el.isContentEditable)
      );
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === 'Escape') {
        if (isEditable(event.target)) return;
        setTimeSelection(null);
        setSelectedMarkerId(null);
        return;
      }
      if (event.key === 'Delete' && selectedMarkerId && selectedClipIds.length === 0) {
        if (isEditable(event.target)) return;
        const marker = arrangement.markers.find((item) => item.id === selectedMarkerId);
        if (!marker) return;
        event.preventDefault();
        void api.removeMarker(marker.id).then((next) => {
          setSession(next);
          setSelectedMarkerId(null);
        });
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [selectedMarkerId, selectedClipIds.length, arrangement.markers, api, setSession]);

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
        '[data-marker-id], [data-range-handle], [data-range-close]',
      )
    )
      return;
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
            'Loop range updated.',
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
            'Punch range updated.',
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
    void props.api
      .updateTimelineLoopRange(true, timeSelection.startTick, timeSelection.endTick)
      .then(props.setSession);
  };

  const setPunchToSelection = () => {
    if (!timeSelection) return;
    void props.api
      .updateTimelinePunchRange(true, timeSelection.startTick, timeSelection.endTick)
      .then(props.setSession);
  };

  const clearLoop = () => {
    void props.api
      .updateTimelineLoopRange(
        false,
        arrangement.loopRange.startTick,
        arrangement.loopRange.endTick,
      )
      .then(props.setSession);
  };

  const clearPunch = () => {
    void props.api
      .updateTimelinePunchRange(
        false,
        arrangement.punchRange?.startTick ?? 0,
        arrangement.punchRange?.endTick ?? 0,
      )
      .then(props.setSession);
  };

  const addMarkerAt = (tick: number) => {
    const existing = new Set(arrangement.markers.map((marker) => marker.id));
    void editor
      .commit(
        api.addMarker(editor.snapTick(tick), `Marker ${arrangement.markers.length + 1}`),
        'Marker added. Press Delete to remove.',
      )
      .then((next) => {
        if (!next) return;
        const created = next.arrangement.markers.find((marker) => !existing.has(marker.id));
        if (created) setSelectedMarkerId(created.id);
      });
  };

  const renameMarker = (marker: Marker) => {
    const next = window.prompt('Marker name', marker.name)?.trim();
    if (next && next !== marker.name)
      void editor.commit(props.api.updateMarker(marker.id, { name: next }), 'Marker renamed.');
  };

  const removeMarker = (marker: Marker) => {
    if (!window.confirm(`Delete marker "${marker.name}"?`)) return;
    void editor.commit(props.api.removeMarker(marker.id), 'Marker removed.').then(() => {
      if (selectedMarkerId === marker.id) setSelectedMarkerId(null);
    });
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
        { label: 'Clear Loop', onClick: clearLoop, disabled: !arrangement.loopRange.enabled },
        { label: 'Clear Punch', onClick: clearPunch, disabled: !arrangement.punchRange },
      ],
    });
  };

  const openMarkerContextMenu = (event: React.MouseEvent, marker: Marker) => {
    event.preventDefault();
    setSelectedMarkerId(marker.id);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Rename', onClick: () => renameMarker(marker) },
        { label: 'Delete', danger: true, onClick: () => removeMarker(marker) },
      ],
    });
  };

  const deleteTrack = async (trackId: string, name: string, clipCount: number) => {
    const detail = clipCount
      ? ` This also removes ${clipCount} Clip${clipCount === 1 ? '' : 's'} from the Timeline.`
      : '';
    if (!window.confirm(`Delete ${name}?${detail}\n\nSource Audio Assets will be kept.`)) return;
    const next = await editor.commit(props.api.removeTrack(trackId), `${name} deleted.`);
    if (next) {
      const remaining = new Set(next.arrangement.audioClips.map((clip) => clip.id));
      const clipIds = selectedClipIds.filter((id) => remaining.has(id));
      props.setSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
    }
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
            void editor.commit(api.duplicateAudioClip(clip.id), `${clip.name} duplicated.`);
          },
        },
        {
          label: clip.muted ? 'Unmute' : 'Mute',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(
              api.updateAudioClip(clip.id, { muted: !clip.muted }),
              `${clip.name} ${clip.muted ? 'unmuted' : 'muted'}.`,
            );
          },
        },
        {
          label: clip.loopEnabled ? 'Disable Loop' : 'Enable Loop',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(
              api.updateAudioClip(clip.id, { loopEnabled: !clip.loopEnabled }),
              `${clip.name} loop ${clip.loopEnabled ? 'disabled' : 'enabled'}.`,
            );
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
            void editor.commit(api.removeTimelineClips([clip.id], []), `${clip.name} deleted.`);
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
            void editor.commit(api.duplicateMidiClip(clip.id), `${clip.name} duplicated.`);
          },
        },
        {
          label: clip.muted ? 'Unmute' : 'Mute',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(
              api.updateMidiClip(clip.id, { muted: !clip.muted }),
              `${clip.name} ${clip.muted ? 'unmuted' : 'muted'}.`,
            );
          },
        },
        {
          label: clip.loopEnabled ? 'Disable Loop' : 'Enable Loop',
          onClick: () => {
            setContextMenu(null);
            void editor.commit(
              api.updateMidiClip(clip.id, { loopEnabled: !clip.loopEnabled }),
              `${clip.name} loop ${clip.loopEnabled ? 'disabled' : 'enabled'}.`,
            );
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
              `${clip.name} quantized.`,
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
            void editor.commit(api.removeTimelineClips([], [clip.id]), `${clip.name} deleted.`);
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
  const setGlobalTrackSize = (size: TrackSize) => {
    setTrackSize(size);
    setTrackSizes({});
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
          label: 'Import Audio Asset here',
          onClick: () =>
            editor.setMessage('Drag an Audio Asset from the Library and drop it on this Track.'),
        },
        {
          label: 'Import MIDI Asset here',
          onClick: () =>
            editor.setMessage('Drag a MIDI Asset from the Library and drop it on this Track.'),
        },
        {
          label: 'Create MIDI Clip',
          onClick: () =>
            editor.setMessage(
              'Create a MIDI Clip by recording from a MIDI keyboard or dragging a MIDI Asset from the Library.',
            ),
        },
        { separator: true },
        {
          label: 'Add Audio Track',
          onClick: () =>
            void editor.commit(
              props.api.addTrack(`Audio ${arrangement.tracks.length + 1}`, 'audio'),
              'Audio Track added.',
            ),
        },
        {
          label: 'Add Instrument Track',
          onClick: () =>
            void editor.commit(
              props.api.addTrack(`Instrument ${arrangement.tracks.length + 1}`, 'instrument'),
              'Instrument Track added.',
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
          onClick: () =>
            void deleteTrack(
              track.id,
              track.name,
              arrangement.audioClips.filter((clip) => clip.trackId === track.id).length,
            ),
        },
      ],
    });
  };

  return (
    <section className={styles.workspace} aria-label="Arrange timeline">
      <ArrangeToolbar
        tool={tool}
        snap={snap}
        zoom={zoom}
        trackSize={trackSize}
        rulerMode={rulerMode}
        follow={follow}
        position={formatMusicalPosition(displayTick, timebase)}
        clock={formatClock(displayTick, timebase)}
        bpm={timebase.bpm}
        signature={`${timebase.timeSignatureNumerator}/${timebase.timeSignatureDenominator}`}
        onTool={setTool}
        onSnap={setSnap}
        onZoom={applyZoom}
        onTrackSize={setGlobalTrackSize}
        onRulerMode={setRulerMode}
        onFollow={setFollow}
        onTimebase={(bpm, numerator, denominator) =>
          void editor.commit(
            props.api.updateArrangementTimebase({
              ...timebase,
              bpm,
              timeSignatureNumerator: numerator,
              timeSignatureDenominator: denominator,
            }),
            'Project timebase updated.',
          )
        }
        onAddTrack={(kind) =>
          void editor.commit(
            props.api.addTrack(
              `${kind === 'audio' ? 'Audio' : 'Instrument'} ${arrangement.tracks.length + 1}`,
              kind,
            ),
            `${kind === 'audio' ? 'Audio' : 'Instrument'} Track added.`,
          )
        }
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
              void editor.commit(
                props.api.addTrackEffect(trackId, plugin.path),
                `Effect ${plugin.name} added.`,
              );
            } else {
              void editor.commit(
                props.api.setTrackInstrument(trackId, plugin.path),
                'Instrument updated.',
              );
            }
          }}
          onClose={() => setPluginPicker(null)}
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
            scrollTop={scrollTop}
            loopRange={loopPreview ?? arrangement.loopRange}
            punchRange={punchPreview ?? arrangement.punchRange}
            markers={arrangement.markers}
            selectedMarkerId={selectedMarkerId}
            timeSelection={timeSelection}
            onPointerDown={seekFromRuler}
            onLoopHandle={dragLoopHandle}
            onPunchHandle={dragPunchHandle}
            onClearLoop={clearLoop}
            onClearPunch={clearPunch}
            onRulerContextMenu={openRulerContextMenu}
            onMarkerContextMenu={openMarkerContextMenu}
            onAddMarker={addMarkerAt}
            onMoveMarker={(marker, tick) =>
              void editor.commit(
                props.api.updateMarker(marker.id, { tick: editor.snapTick(tick) }),
                `${marker.name} moved.`,
              )
            }
            onRenameMarker={renameMarker}
            onRemoveMarker={removeMarker}
            onSelectMarker={setSelectedMarkerId}
          />
          {transport && transport.recordingPhase !== 'idle' && (
            <div
              className={styles.recordingPreview}
              style={{
                left: TRACK_HEADER_WIDTH + transport.recordingStartTick * pixelsPerTick,
                width:
                  Math.max(1, transport.recordingCurrentTick - transport.recordingStartTick) *
                  pixelsPerTick,
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
          <div
            className={styles.playhead}
            style={{ left: TRACK_HEADER_WIDTH + displayTick * pixelsPerTick }}
          >
            <span />
          </div>
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
              <button
                className={styles.selectionChipClose}
                aria-label="Clear time selection"
                title="Clear time selection (Esc)"
                onClick={() => setTimeSelection(null)}
              >
                ×
              </button>
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
              <p>Drag audio or MIDI here, or start from an empty track.</p>
              <div className={styles.emptyActions}>
                <button
                  onClick={() =>
                    void editor.commit(props.api.addTrack('Audio 1', 'audio'), 'Audio Track added.')
                  }
                >
                  ＋ Add Audio Track
                </button>
                <button
                  onClick={() =>
                    void editor.commit(
                      props.api.addTrack('Instrument 1', 'instrument'),
                      'Instrument Track added.',
                    )
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
                  clips={arrangement.audioClips.filter((clip) => clip.trackId === track.id)}
                  midiClips={arrangement.midiClips.filter((clip) => clip.trackId === track.id)}
                  session={props.session}
                  analyses={analyses}
                  selectedClipIds={selectedClipIds}
                  unavailableClipIds={transport?.unavailableClipIds ?? []}
                  selected={
                    props.selection.kind === 'track' && props.selection.trackId === track.id
                  }
                  onSelectTrack={() => props.setSelection({ kind: 'track', trackId: track.id })}
                  timelineWidth={timelineWidth}
                  timelineTicks={timelineTicks}
                  pixelsPerTick={pixelsPerTick}
                  trackSize={trackSizes[track.id] ?? trackSize}
                  api={props.api}
                  onCommit={editor.commit}
                  onDrop={(event, trackId) => {
                    if (event.dataTransfer.files?.length) {
                      void handleOsMidiDrop(event.dataTransfer.files, trackId);
                      return;
                    }
                    void editor.dropAsset(event, trackId);
                  }}
                  onContextMenu={openTrackLaneContextMenu}
                  onMove={editor.beginMove}
                  onMoveMidi={editor.beginMidiMove}
                  onTrimMidi={editor.beginMidiTrim}
                  onSelect={editor.selectClip}
                  onTrim={editor.beginTrim}
                  onFade={editor.beginFade}
                  onOpenMidiEditor={(clip) => {
                    setActiveMidiClipId(clip.id);
                    setMidiEditorOpen(true);
                  }}
                  onAudioClipContextMenu={openAudioClipContextMenu}
                  onMidiClipContextMenu={openMidiClipContextMenu}
                  onRename={(name) =>
                    void editor.commit(
                      props.api.updateTrack(track.id, { name }),
                      `Track renamed to ${name}.`,
                    )
                  }
                  onDuplicate={() =>
                    void editor.commit(
                      props.api.duplicateTrack(track.id),
                      `${track.name} duplicated.`,
                    )
                  }
                  onDelete={() =>
                    void deleteTrack(
                      track.id,
                      track.name,
                      arrangement.audioClips.filter((clip) => clip.trackId === track.id).length,
                    )
                  }
                  onReorder={(sourceTrackId) =>
                    void editor.commit(
                      props.api.reorderTrack(sourceTrackId, trackIndex),
                      'Track order updated.',
                    )
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
                        `${track.name} automation updated.`,
                      )
                    }
                  />
                )}
              </Fragment>
            ))
          )}
        </div>
      </div>

      <div className={styles.statusToast} role="status">
        <span className={transport?.state === 'playing' ? styles.playingDot : ''} />
        {playbackOutOfSync
          ? 'Playback runtime is out of sync'
          : unavailableClipCount || missingDeviceCount
            ? `Playback skipped ${unavailableClipCount} missing source${unavailableClipCount === 1 ? '' : 's'} and ${missingDeviceCount} missing device${missingDeviceCount === 1 ? '' : 's'}.`
            : editor.message}
        {playbackOutOfSync && <button onClick={() => void editor.retryRuntimeSync()}>Retry</button>}
        <small>REV {arrangement.revision}</small>
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenu.items}
          onClose={() => setContextMenu(null)}
        />
      )}

      {midiEditorOpen && (
        <MidiEditorPanel
          clip={arrangement.midiClips.find((clip) => clip.id === activeMidiClipId) ?? null}
          timebase={timebase}
          onClose={() => {
            setMidiEditorOpen(false);
            setActiveMidiClipId(null);
          }}
          onAddNote={(clipId, startTick, pitch) =>
            void editor.commit(
              props.api.addMidiNote(clipId, Math.max(0, Math.round(startTick)), pitch, 240, 96, 1),
              'Note added.',
            )
          }
          onUpdateNote={(clipId, note) =>
            void editor.commit(
              props.api.updateMidiNote(clipId, note.id, {
                note: note.note,
                startTick: note.startTick,
                durationTicks: note.durationTicks,
                velocity: note.velocity,
              }),
              'Note updated.',
            )
          }
          onUpdateNotes={(clipId, updates) =>
            void editor.commit(
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
              'MIDI notes updated.',
            )
          }
          onRemoveNote={(clipId, noteId) =>
            void editor.commit(props.api.removeMidiNote(clipId, noteId), 'Note removed.')
          }
          onQuantize={(clipId, noteIds, gridTicks) =>
            void editor.commit(
              props.api.quantizeMidiNotes(clipId, noteIds, gridTicks),
              'MIDI notes quantized.',
            )
          }
          onDuplicateNotes={(clipId, noteIds, offsetTicks) =>
            void editor.commit(
              props.api.duplicateMidiNotes(clipId, noteIds, offsetTicks),
              'MIDI notes duplicated.',
            )
          }
        />
      )}
    </section>
  );
}
