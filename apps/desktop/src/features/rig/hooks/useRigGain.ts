import { useRef, useState } from 'react';
import type { ValueControl } from '@/shared/ui/ValueSlider';

/** A dragged gain and the saved gain it was dragged from. */
interface Draft {
  value: number;
  from: number;
}

/**
 * Drives the rig volume slider: the drag stays local and one update is sent
 * on release. The dragged value shows until the saved gain changes.
 */
export function useRigGain(gainDb: number, save: (gainDb: number) => Promise<boolean>) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const editing = useRef(false);
  const control: ValueControl = {
    value: draft && draft.from === gainDb ? draft.value : gainDb,
    begin() {
      editing.current = true;
      setDraft({ value: gainDb, from: gainDb });
    },
    change(value) {
      setDraft({ value, from: gainDb });
    },
    async commit(value) {
      if (!editing.current) return;
      editing.current = false;
      if (value === gainDb) {
        setDraft(null);
        return;
      }
      if (!(await save(value))) setDraft(null);
    },
    cancel() {
      editing.current = false;
      setDraft(null);
    },
  };
  return control;
}
