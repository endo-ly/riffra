import type { EffectOwner } from '@/model/generated';

/** The selection state owned by the editor workspace. */
export type EditorSelection =
  | { kind: 'none' }
  | { kind: 'track'; trackId: string }
  | { kind: 'clips'; clipIds: string[] }
  | { kind: 'bus'; busId: string }
  | { kind: 'master' };

/** Resolves the selected track, bus, or master to its effect owner. */
export function selectionOwner(selection: EditorSelection): EffectOwner | null {
  switch (selection.kind) {
    case 'track':
      return { kind: 'track', id: selection.trackId };
    case 'bus':
      return { kind: 'bus', id: selection.busId };
    case 'master':
      return { kind: 'master' };
    case 'none':
    case 'clips':
      return null;
  }
}

/** Converts an effect owner into its corresponding editor selection. */
export function ownerSelection(owner: EffectOwner): EditorSelection {
  switch (owner.kind) {
    case 'track':
      return { kind: 'track', trackId: owner.id };
    case 'bus':
      return { kind: 'bus', busId: owner.id };
    case 'master':
      return { kind: 'master' };
  }
}
