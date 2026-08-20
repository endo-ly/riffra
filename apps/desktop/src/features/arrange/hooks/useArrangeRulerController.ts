import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MutableRefObject,
  type PointerEvent,
} from 'react';
import type { ArrangementMutationResult, CreativeSession, Marker } from '@/model/domain';
import type { ArrangeApi, TransportApi } from '@/native/native-api';
import { isEditableTarget } from '../model/interaction';

type RangeKind = 'loop' | 'punch';
type ArrangeRulerApi = Pick<
  ArrangeApi,
  | 'addMarker'
  | 'updateMarker'
  | 'removeMarker'
  | 'updateTimelineLoopRange'
  | 'updateTimelinePunchRange'
> &
  Pick<TransportApi, 'seekTimeline'>;

interface TimeSelection {
  startTick: number;
  endTick: number;
}

interface LoopPreview {
  enabled: boolean;
  startTick: number;
  endTick: number;
}

interface PunchPreview {
  startTick: number;
  endTick: number;
}

interface ArrangeRulerControllerOptions {
  arrangement: CreativeSession['arrangement'];
  api: ArrangeRulerApi;
  commit: (operation: Promise<ArrangementMutationResult | null>) => Promise<CreativeSession | null>;
  snapTick: (raw: number, temporaryOff?: boolean) => number;
  pixelsPerTick: number;
  displayTickRef: MutableRefObject<number>;
  selectedClipCount: number;
  seekLocally: (tick: number) => void;
  setMessage: (message: string) => void;
  setFollow: (follow: boolean) => void;
}

export function useArrangeRulerController({
  arrangement,
  api,
  commit,
  snapTick,
  pixelsPerTick,
  displayTickRef,
  selectedClipCount,
  seekLocally,
  setMessage,
  setFollow,
}: ArrangeRulerControllerOptions) {
  const [selectedMarkerId, setSelectedMarkerId] = useState<string | null>(null);
  const [selectedRange, setSelectedRange] = useState<RangeKind | null>(null);
  const [markerRename, setMarkerRename] = useState<{ markerId: string; name: string } | null>(null);
  const [timeSelection, setTimeSelection] = useState<TimeSelection | null>(null);
  const [loopPreview, setLoopPreview] = useState<LoopPreview | null>(null);
  const [punchPreview, setPunchPreview] = useState<PunchPreview | null>(null);
  const gestureCleanupRef = useRef<(() => void) | null>(null);

  const clearRulerSelection = useCallback(() => {
    setTimeSelection(null);
    setSelectedMarkerId(null);
    setSelectedRange(null);
  }, []);

  const selectRange = useCallback((range: RangeKind) => {
    setSelectedRange(range);
    setSelectedMarkerId(null);
  }, []);

  const clearSelectedRange = useCallback(() => setSelectedRange(null), []);
  const clearTimeSelection = useCallback(() => setTimeSelection(null), []);
  const clearMarkerRename = useCallback(() => setMarkerRename(null), []);

  const selectMarker = useCallback((markerId: string | null) => {
    setSelectedMarkerId(markerId);
    setSelectedRange(null);
  }, []);

  const clearRange = useCallback(
    (range: RangeKind) => {
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

  const addMarkerAt = useCallback(
    (tick: number) => {
      const existing = new Set(arrangement.markers.map((marker) => marker.id));
      void commit(api.addMarker(snapTick(tick), `Marker ${arrangement.markers.length + 1}`)).then(
        (next) => {
          if (!next) return;
          const created = next.arrangement.markers.find((marker) => !existing.has(marker.id));
          if (created) setSelectedMarkerId(created.id);
        },
      );
    },
    [api, arrangement.markers, commit, snapTick],
  );

  const moveMarker = useCallback(
    (marker: Marker, tick: number) => {
      void commit(api.updateMarker(marker.id, { tick: snapTick(tick) }));
    },
    [api, commit, snapTick],
  );

  const renameMarker = useCallback((marker: Marker) => {
    setMarkerRename({ markerId: marker.id, name: marker.name });
  }, []);

  const removeMarker = useCallback(
    (marker: Marker) => {
      void commit(api.removeMarker(marker.id)).then((next) => {
        if (next && selectedMarkerId === marker.id) setSelectedMarkerId(null);
      });
    },
    [api, commit, selectedMarkerId],
  );

  const updateMarkerRename = useCallback((name: string) => {
    setMarkerRename((current) => (current ? { ...current, name } : current));
  }, []);

  const saveMarkerRename = useCallback(() => {
    if (!markerRename) return;
    const marker = arrangement.markers.find((item) => item.id === markerRename.markerId);
    const next = markerRename.name.trim();
    setMarkerRename(null);
    if (marker && next && next !== marker.name)
      void commit(api.updateMarker(marker.id, { name: next }));
  }, [api, arrangement.markers, commit, markerRename]);

  const setLoopToSelection = useCallback(() => {
    if (!timeSelection) return;
    void commit(api.updateTimelineLoopRange(true, timeSelection.startTick, timeSelection.endTick));
  }, [api, commit, timeSelection]);

  const setPunchToSelection = useCallback(() => {
    if (!timeSelection) return;
    void commit(api.updateTimelinePunchRange(true, timeSelection.startTick, timeSelection.endTick));
  }, [api, commit, timeSelection]);

  const seekFromRuler = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (
        (event.target as HTMLElement).closest(
          '[data-marker-id], [data-range-band], [data-range-handle]',
        )
      )
        return;
      setSelectedRange(null);
      const bounds = event.currentTarget.getBoundingClientRect();
      const originTick = snapTick((event.clientX - bounds.left) / pixelsPerTick, event.altKey);
      const originX = event.clientX;
      let seeking = true;
      seekLocally(originTick);
      void api.seekTimeline(originTick).catch((error) => setMessage(String(error)));
      setFollow(true);
      const handle = (move: globalThis.PointerEvent) => {
        const tick = snapTick((move.clientX - bounds.left) / pixelsPerTick, move.altKey);
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
          void api.seekTimeline(tick).catch((error) => setMessage(String(error)));
        } else {
          setTimeSelection((current) =>
            current
              ? { startTick: Math.min(originTick, tick), endTick: Math.max(originTick, tick) }
              : null,
          );
        }
      };
      const finish = () => {
        gestureCleanupRef.current?.();
        if (seeking) setTimeSelection(null);
      };
      const cancel = () => {
        gestureCleanupRef.current?.();
        setTimeSelection(null);
      };
      const cleanup = () => {
        window.removeEventListener('pointermove', handle);
        window.removeEventListener('pointerup', finish);
        window.removeEventListener('pointercancel', cancel);
        if (gestureCleanupRef.current === cleanup) gestureCleanupRef.current = null;
      };
      gestureCleanupRef.current?.();
      gestureCleanupRef.current = cleanup;
      window.addEventListener('pointermove', handle);
      window.addEventListener('pointerup', finish);
      window.addEventListener('pointercancel', cancel);
    },
    [api, pixelsPerTick, seekLocally, setFollow, setMessage, snapTick],
  );

  const dragLoopHandle = useCallback(
    (event: PointerEvent<HTMLSpanElement>, boundary: 'start' | 'end') => {
      event.stopPropagation();
      event.preventDefault();
      const originX = event.clientX;
      const range = arrangement.loopRange;
      const origin = boundary === 'start' ? range.startTick : range.endTick;
      const computeNext = (clientX: number, altKey: boolean) =>
        snapTick(origin + (clientX - originX) / pixelsPerTick, altKey);
      const applyPreview = (clientX: number, altKey: boolean) => {
        const next = computeNext(clientX, altKey);
        setLoopPreview({
          enabled: range.enabled,
          startTick: boundary === 'start' ? next : range.startTick,
          endTick: boundary === 'end' ? next : range.endTick,
        });
      };
      const move = (pointer: globalThis.PointerEvent) =>
        applyPreview(pointer.clientX, pointer.altKey);
      const finish = (pointer: globalThis.PointerEvent) => {
        gestureCleanupRef.current?.();
        const next = computeNext(pointer.clientX, pointer.altKey);
        if (next !== origin) {
          void commit(
            api.updateTimelineLoopRange(
              range.enabled,
              boundary === 'start' ? next : range.startTick,
              boundary === 'end' ? next : range.endTick,
            ),
          ).finally(() => setLoopPreview(null));
        } else setLoopPreview(null);
      };
      const cancel = () => {
        gestureCleanupRef.current?.();
        setLoopPreview(null);
      };
      const cleanup = () => {
        window.removeEventListener('pointermove', move);
        window.removeEventListener('pointerup', finish);
        window.removeEventListener('pointercancel', cancel);
        if (gestureCleanupRef.current === cleanup) gestureCleanupRef.current = null;
      };
      gestureCleanupRef.current?.();
      gestureCleanupRef.current = cleanup;
      window.addEventListener('pointermove', move);
      window.addEventListener('pointerup', finish);
      window.addEventListener('pointercancel', cancel);
      applyPreview(event.clientX, event.altKey);
    },
    [api, arrangement.loopRange, commit, pixelsPerTick, snapTick],
  );

  const dragPunchHandle = useCallback(
    (event: PointerEvent<HTMLSpanElement>, boundary: 'start' | 'end') => {
      event.stopPropagation();
      event.preventDefault();
      const originX = event.clientX;
      const range = arrangement.punchRange;
      if (!range) return;
      const origin = boundary === 'start' ? range.startTick : range.endTick;
      const computeNext = (clientX: number, altKey: boolean) =>
        snapTick(origin + (clientX - originX) / pixelsPerTick, altKey);
      const applyPreview = (clientX: number, altKey: boolean) => {
        const next = computeNext(clientX, altKey);
        setPunchPreview({
          startTick: boundary === 'start' ? next : range.startTick,
          endTick: boundary === 'end' ? next : range.endTick,
        });
      };
      const move = (pointer: globalThis.PointerEvent) =>
        applyPreview(pointer.clientX, pointer.altKey);
      const finish = (pointer: globalThis.PointerEvent) => {
        gestureCleanupRef.current?.();
        const next = computeNext(pointer.clientX, pointer.altKey);
        if (next !== origin) {
          void commit(
            api.updateTimelinePunchRange(
              true,
              boundary === 'start' ? next : range.startTick,
              boundary === 'end' ? next : range.endTick,
            ),
          ).finally(() => setPunchPreview(null));
        } else setPunchPreview(null);
      };
      const cancel = () => {
        gestureCleanupRef.current?.();
        setPunchPreview(null);
      };
      const cleanup = () => {
        window.removeEventListener('pointermove', move);
        window.removeEventListener('pointerup', finish);
        window.removeEventListener('pointercancel', cancel);
        if (gestureCleanupRef.current === cleanup) gestureCleanupRef.current = null;
      };
      gestureCleanupRef.current?.();
      gestureCleanupRef.current = cleanup;
      window.addEventListener('pointermove', move);
      window.addEventListener('pointerup', finish);
      window.addEventListener('pointercancel', cancel);
      applyPreview(event.clientX, event.altKey);
    },
    [api, arrangement.punchRange, commit, pixelsPerTick, snapTick],
  );

  const handleKeyboard = useCallback(
    (event: KeyboardEvent) => {
      if (event.defaultPrevented || isEditableTarget(event.target)) return false;
      if (event.key === 'Escape') {
        clearRulerSelection();
        return true;
      }
      if (event.key === 'Delete' && selectedRange && selectedClipCount === 0) {
        const rangeIsActive =
          selectedRange === 'loop'
            ? arrangement.loopRange.enabled
            : Boolean(arrangement.punchRange);
        if (!rangeIsActive) {
          setSelectedRange(null);
          return true;
        }
        event.preventDefault();
        clearRange(selectedRange);
        setSelectedRange(null);
        return true;
      }
      if (event.key === 'Delete' && selectedMarkerId && selectedClipCount === 0) {
        const marker = arrangement.markers.find((item) => item.id === selectedMarkerId);
        if (!marker) return false;
        event.preventDefault();
        removeMarker(marker);
        return true;
      }
      if (event.key.toLowerCase() === 'm' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        event.preventDefault();
        addMarkerAt(displayTickRef.current);
        return true;
      }
      return false;
    },
    [
      addMarkerAt,
      arrangement.loopRange.enabled,
      arrangement.markers,
      arrangement.punchRange,
      clearRange,
      clearRulerSelection,
      displayTickRef,
      removeMarker,
      selectedClipCount,
      selectedMarkerId,
      selectedRange,
    ],
  );

  useEffect(() => {
    if (!timeSelection) return;
    const onClick = (event: MouseEvent) => {
      const target = event.target as HTMLElement;
      if (target.closest('[data-time-selection-chip]')) return;
      if (target.closest('[data-arrange-ruler]')) return;
      if (target.closest('[data-midi-empty-lane]') && !target.closest('[data-clip-id]')) return;
      setTimeSelection(null);
    };
    document.addEventListener('click', onClick);
    return () => document.removeEventListener('click', onClick);
  }, [timeSelection]);

  useEffect(() => () => gestureCleanupRef.current?.(), []);

  return {
    selectedMarkerId,
    selectedRange,
    markerRename,
    timeSelection,
    loopPreview,
    punchPreview,
    clearRulerSelection,
    selectRange,
    clearSelectedRange,
    clearTimeSelection,
    selectMarker,
    clearRange,
    addMarkerAt,
    moveMarker,
    renameMarker,
    removeMarker,
    updateMarkerRename,
    saveMarkerRename,
    cancelMarkerRename: clearMarkerRename,
    setLoopToSelection,
    setPunchToSelection,
    seekFromRuler,
    dragLoopHandle,
    dragPunchHandle,
    handleKeyboard,
  };
}
