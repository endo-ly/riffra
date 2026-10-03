import type {
  InstrumentCollection,
  InstrumentLibraryItem,
  LibraryAsset,
  RecordingAsset,
} from '@/model/domain';
import { dispatchControl, dispatchControlOrFallback } from '../invoke';

export async function listRecordings(query?: string): Promise<RecordingAsset[]> {
  return dispatchControlOrFallback(
    { command: 'record.list', params: { query: query ?? null } },
    [],
  );
}

export async function renameRecording(id: string, name: string): Promise<string> {
  return dispatchControl({ command: 'record.rename', params: { id, newName: name } });
}

export async function deleteRecording(id: string): Promise<void> {
  await dispatchControl({ command: 'record.delete', params: { id } });
}

export async function archiveRecording(id: string): Promise<string> {
  return dispatchControl({ command: 'record.archive', params: { id } });
}

export async function promoteRecording(id: string): Promise<string> {
  return dispatchControl({ command: 'record.promote', params: { id } });
}

export async function tagRecording(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  return dispatchControl({ command: 'record.tag', params: { id, tag, note } });
}

export async function detectDuplicateRecordings(): Promise<string[][]> {
  return dispatchControl({ command: 'record.duplicates', params: {} });
}

export async function searchLibrary(query: string): Promise<LibraryAsset[]> {
  if (!query.trim()) return [];
  return dispatchControlOrFallback({ command: 'library.search', params: { query } }, []);
}

export async function updateLibraryAsset(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  return dispatchControlOrFallback(
    { command: 'library.asset.update', params: { id, tag, note } },
    null,
  );
}

export async function relatedLibraryAssets(id: string): Promise<LibraryAsset[]> {
  return dispatchControlOrFallback({ command: 'library.related', params: { id } }, []);
}

export async function listInstruments(): Promise<InstrumentLibraryItem[]> {
  return dispatchControlOrFallback({ command: 'library.instrument.list', params: {} }, []);
}

export async function setInstrumentFavorite(
  instrumentId: string,
  favorite: boolean,
): Promise<InstrumentLibraryItem> {
  return dispatchControl({
    command: 'library.instrument.favorite.set',
    params: { instrumentId, favorite },
  });
}

export async function setInstrumentCategoryOverride(
  instrumentId: string,
  category: string | null,
): Promise<InstrumentLibraryItem> {
  return dispatchControl({
    command: 'library.instrument.category.set',
    params: { instrumentId, category },
  });
}

export async function setInstrumentUserTags(
  instrumentId: string,
  tags: string[],
): Promise<InstrumentLibraryItem> {
  return dispatchControl({
    command: 'library.instrument.tags.set',
    params: { instrumentId, tags },
  });
}

/** Lists the categories the Host files instruments under, in display order. */
export async function listInstrumentCategories(): Promise<string[]> {
  return dispatchControlOrFallback({ command: 'library.instrument.category.list', params: {} }, []);
}

export async function listInstrumentCollections(): Promise<InstrumentCollection[]> {
  return dispatchControlOrFallback(
    { command: 'library.instrument.collection.list', params: {} },
    [],
  );
}

export async function createInstrumentCollection(name: string): Promise<InstrumentCollection> {
  return dispatchControl({ command: 'library.instrument.collection.create', params: { name } });
}

export async function renameInstrumentCollection(
  id: number,
  name: string,
): Promise<InstrumentCollection> {
  return dispatchControl({ command: 'library.instrument.collection.rename', params: { id, name } });
}

export async function deleteInstrumentCollection(id: number): Promise<void> {
  await dispatchControl({ command: 'library.instrument.collection.delete', params: { id } });
}

export async function setInstrumentCollectionMembership(
  collectionId: number,
  instrumentId: string,
  included: boolean,
): Promise<InstrumentLibraryItem> {
  return dispatchControl({
    command: 'library.instrument.collection.membership.set',
    params: { collectionId, instrumentId, included },
  });
}
