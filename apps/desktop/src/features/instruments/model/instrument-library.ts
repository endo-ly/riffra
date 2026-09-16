import type { InstrumentCollection, InstrumentLibraryItem } from '@/model/domain';

interface InstrumentSearchable {
  name: string;
  author?: string | null;
  description?: string | null;
  category?: string;
  defaultCategory?: string;
  defaultTags?: string[];
  userTags?: string[];
  tags?: string[];
  collectionIds?: number[];
}

export interface InstrumentFilters {
  category: string | null;
  tag: string | null;
  collectionId: number | null;
  favoritesOnly: boolean;
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
    item.author ?? '',
    item.description ?? '',
    item.category ?? '',
    item.defaultCategory ?? '',
    ...(item.defaultTags ?? []),
    ...(item.userTags ?? []),
    ...(item.tags ?? []),
    ...collectionLabels,
  ];
}

/** Returns whether a built-in instrument matches a case-insensitive query. */
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

/** Returns sorted unique effective categories for the supplied instruments. */
export function getInstrumentCategories(items: InstrumentSearchable[]): string[] {
  return Array.from(
    new Map(
      items
        .map((item) => item.category ?? item.defaultCategory ?? '')
        .filter(Boolean)
        .map((category) => [category.toLocaleLowerCase(), category]),
    ).values(),
  ).sort((left, right) => left.localeCompare(right));
}

/** Returns sorted unique effective tags for the supplied instruments. */
export function getInstrumentTags(items: InstrumentSearchable[]): string[] {
  return Array.from(
    new Map(
      items
        .flatMap((item) => [
          ...(item.defaultTags ?? []),
          ...(item.userTags ?? []),
          ...(item.tags ?? []),
        ])
        .filter(Boolean)
        .map((tag) => [tag.toLocaleLowerCase(), tag]),
    ).values(),
  ).sort((left, right) => left.localeCompare(right));
}

/** Applies Browser query and instrument-library filters without changing input order. */
export function filterInstruments(
  items: InstrumentLibraryItem[],
  query: string,
  filters: InstrumentFilters = {
    category: null,
    tag: null,
    collectionId: null,
    favoritesOnly: false,
  },
  collections: CollectionLookup = [],
): InstrumentLibraryItem[] {
  const normalizedCategory = filters.category?.trim().toLocaleLowerCase();
  const normalizedTag = filters.tag?.trim().toLocaleLowerCase();
  return items.filter((item) => {
    if (!matchesInstrumentQuery(item, query, collections)) return false;
    if (normalizedCategory && item.category.toLocaleLowerCase() !== normalizedCategory)
      return false;
    if (normalizedTag && !item.tags.some((tag) => tag.toLocaleLowerCase() === normalizedTag)) {
      return false;
    }
    if (filters.collectionId !== null && !item.collectionIds.includes(filters.collectionId)) {
      return false;
    }
    if (filters.favoritesOnly && !item.favorite) return false;
    return true;
  });
}

/** Formats a MIDI note number using the conventional C4 = 60 octave name. */
export function formatMidiNote(note: number): string {
  if (!Number.isInteger(note) || note < 0 || note > 127) return '—';
  const names = ['C', 'C♯', 'D', 'D♯', 'E', 'F', 'F♯', 'G', 'G♯', 'A', 'A♯', 'B'];
  return `${names[note % 12]}${Math.floor(note / 12) - 1}`;
}
