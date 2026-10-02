import type { InstrumentCollection } from '@/model/domain';

interface InstrumentSearchable {
  id?: string;
  name: string;
  description?: string | null;
  category?: string | null;
  defaultCategory?: string | null;
  defaultTags?: string[];
  userTags?: string[];
  tags?: string[];
  collectionIds?: number[];
}

type CollectionLookup = InstrumentCollection[] | ReadonlyMap<number, string>;

function collectionNames(collections: CollectionLookup): Map<number, string> {
  if (Array.isArray(collections)) {
    return new Map(collections.map((collection) => [collection.id, collection.name]));
  }
  return new Map(collections);
}

function searchableValues(item: InstrumentSearchable, collections: CollectionLookup): string[] {
  const collectionLookup = collectionNames(collections);
  const collectionLabels = (item.collectionIds ?? [])
    .map((id) => collectionLookup.get(id))
    .filter((name): name is string => Boolean(name));
  return [
    item.name,
    item.description ?? '',
    item.category ?? '',
    item.defaultCategory ?? '',
    ...(item.defaultTags ?? []),
    ...(item.userTags ?? []),
    ...(item.tags ?? []),
    ...collectionLabels,
  ];
}

/** Returns whether an instrument matches a case-insensitive query. */
export function matchesInstrumentQuery(
  item: InstrumentSearchable,
  query: string,
  collections: CollectionLookup = [],
): boolean {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return true;
  return searchableValues(item, collections).some((value) =>
    value.toLocaleLowerCase().includes(normalized),
  );
}

/** Formats a MIDI note number using the conventional C4 = 60 octave name. */
export function formatMidiNote(note: number): string {
  if (!Number.isInteger(note) || note < 0 || note > 127) return '—';
  const names = ['C', 'C♯', 'D', 'D♯', 'E', 'F', 'F♯', 'G', 'G♯', 'A', 'A♯', 'B'];
  return `${names[note % 12]}${Math.floor(note / 12) - 1}`;
}
