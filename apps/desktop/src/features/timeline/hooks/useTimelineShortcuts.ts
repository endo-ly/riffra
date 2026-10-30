import { useEffect } from 'react';
import type { Arrangement, ProjectTimebase } from '@/model/generated';
import { clipExtentTicks } from '@/features/timeline/model/timeline-layout';
import { isEditableTypingTarget } from '@/shared/input';

interface TimelineShortcutOptions {
  /** Ruler keys: range and marker handling. */
  handleRulerKeyboard(event: KeyboardEvent): boolean;
  /** Clip editing keys: select all, copy, paste, duplicate, split, delete. */
  handleEditorKeyboard(event: KeyboardEvent): boolean;
  timeSelection: { startTick: number; endTick: number } | null;
  arrangement: Arrangement;
  timebase: ProjectTimebase;
  zoomToRange(startTick: number, endTick: number): void;
}

/** Owns the one window keydown listener of the timeline and its key precedence. */
export function useTimelineShortcuts(options: TimelineShortcutOptions) {
  const { handleRulerKeyboard, handleEditorKeyboard, timeSelection, arrangement, timebase } =
    options;
  const { zoomToRange } = options;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (handleRulerKeyboard(event) || event.defaultPrevented) return;
      if (handleEditorKeyboard(event) || event.defaultPrevented) return;
      if (event.key.toLowerCase() === 'z' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        if (!timeSelection || isEditableTypingTarget(event.target)) return;
        event.preventDefault();
        zoomToRange(timeSelection.startTick, timeSelection.endTick);
        return;
      }
      if (event.key.toLowerCase() === 'f' && !event.ctrlKey && !event.altKey && !event.metaKey) {
        if (isEditableTypingTarget(event.target)) return;
        const extent = clipExtentTicks(arrangement, timebase);
        if (!extent) return;
        event.preventDefault();
        zoomToRange(extent.startTick, extent.endTick);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [
    arrangement,
    handleEditorKeyboard,
    handleRulerKeyboard,
    timebase,
    timeSelection,
    zoomToRange,
  ]);
}
