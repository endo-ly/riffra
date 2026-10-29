import type { EffectOwner } from '@/model/generated';

/** The selection state owned by the Arrange feature. */
export type ArrangeSelection =
  | { kind: 'none' }
  | { kind: 'track'; trackId: string }
  | { kind: 'clips'; clipIds: string[] }
  | { kind: 'bus'; busId: string }
  | { kind: 'master' };

/** Resolves the selected track, bus, or master to its effect owner. */
export function selectionOwner(selection: ArrangeSelection): EffectOwner | null {
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

/** Converts an effect owner into its corresponding Arrange selection. */
export function ownerSelection(owner: EffectOwner): ArrangeSelection {
  switch (owner.kind) {
    case 'track':
      return { kind: 'track', trackId: owner.id };
    case 'bus':
      return { kind: 'bus', busId: owner.id };
    case 'master':
      return { kind: 'master' };
  }
}
