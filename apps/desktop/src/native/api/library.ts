import type {
  InstrumentCollection,
  InstrumentLibraryItem,
  LibraryAsset,
  RecordingAsset,
} from '@/model/domain';
import { invokeHostOrFallback, invokeHost } from '../invoke';

export async function listRecordings(query?: string): Promise<RecordingAsset[]> {
  return invokeHostOrFallback<RecordingAsset[]>('list_recordings', { query: query ?? null }, []);
}

export async function renameRecording(id: string, name: string): Promise<string> {
  return invokeHost<string>('rename_recording', { id, newName: name });
}

export async function deleteRecording(id: string): Promise<void> {
  await invokeHost('delete_recording', { id });
}

export async function archiveRecording(id: string): Promise<string> {
  return await invokeHost<string>('archive_recording', { id });
}

export async function promoteRecording(id: string): Promise<string> {
  return await invokeHost<string>('promote_recording', { id });
}

export async function tagRecording(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  return await invokeHost<LibraryAsset>('tag_recording', { id, tag, note });
}

export async function detectDuplicateRecordings(): Promise<string[][]> {
  return await invokeHost<string[][]>('detect_duplicate_recordings');
}

export async function searchLibrary(query: string): Promise<LibraryAsset[]> {
  if (!query.trim()) return [];
  return invokeHostOrFallback<LibraryAsset[]>('search_library', { query }, []);
}

export async function updateLibraryAsset(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  return invokeHostOrFallback<LibraryAsset | null>('update_library_asset', { id, tag, note }, null);
}

export async function relatedLibraryAssets(id: string): Promise<LibraryAsset[]> {
  return invokeHostOrFallback<LibraryAsset[]>('related_library_assets', { id }, []);
}

export async function listInstruments(): Promise<InstrumentLibraryItem[]> {
  return invokeHostOrFallback<InstrumentLibraryItem[]>('list_instruments', {}, []);
}

export async function setInstrumentFavorite(
  instrumentId: string,
  favorite: boolean,
): Promise<InstrumentLibraryItem> {
  return invokeHost<InstrumentLibraryItem>('set_instrument_favorite', { instrumentId, favorite });
}

export async function setInstrumentCategoryOverride(
  instrumentId: string,
  category: string | null,
): Promise<InstrumentLibraryItem> {
  return invokeHost<InstrumentLibraryItem>('set_instrument_category_override', {
    instrumentId,
    category,
  });
}

export async function setInstrumentUserTags(
  instrumentId: string,
  tags: string[],
): Promise<InstrumentLibraryItem> {
  return invokeHost<InstrumentLibraryItem>('set_instrument_user_tags', { instrumentId, tags });
}

export async function listInstrumentCollections(): Promise<InstrumentCollection[]> {
  return invokeHostOrFallback<InstrumentCollection[]>('list_instrument_collections', {}, []);
}

export async function createInstrumentCollection(name: string): Promise<InstrumentCollection> {
  return invokeHost<InstrumentCollection>('create_instrument_collection', { name });
}

export async function renameInstrumentCollection(
  id: number,
  name: string,
): Promise<InstrumentCollection> {
  return invokeHost<InstrumentCollection>('rename_instrument_collection', { id, name });
}

export async function deleteInstrumentCollection(id: number): Promise<void> {
  await invokeHost('delete_instrument_collection', { id });
}

export async function setInstrumentCollectionMembership(
  collectionId: number,
  instrumentId: string,
  included: boolean,
): Promise<InstrumentLibraryItem> {
  return invokeHost<InstrumentLibraryItem>('set_instrument_collection_membership', {
    collectionId,
    instrumentId,
    included,
  });
}
