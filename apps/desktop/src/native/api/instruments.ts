import type { InstrumentCollection, InstrumentLibraryItem } from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface InstrumentLibraryApi {
  listInstruments(): Promise<InstrumentLibraryItem[]>;
  setInstrumentFavorite(instrumentId: string, favorite: boolean): Promise<InstrumentLibraryItem>;
  setInstrumentCategoryOverride(
    instrumentId: string,
    category: string | null,
  ): Promise<InstrumentLibraryItem>;
  setInstrumentUserTags(instrumentId: string, tags: string[]): Promise<InstrumentLibraryItem>;
  listInstrumentCategories(): Promise<string[]>;
  listInstrumentCollections(): Promise<InstrumentCollection[]>;
  createInstrumentCollection(name: string): Promise<InstrumentCollection>;
  renameInstrumentCollection(id: number, name: string): Promise<InstrumentCollection>;
  deleteInstrumentCollection(id: number): Promise<void>;
  setInstrumentCollectionMembership(
    collectionId: number,
    instrumentId: string,
    included: boolean,
  ): Promise<InstrumentLibraryItem>;
}

async function listInstruments(): Promise<InstrumentLibraryItem[]> {
  return dispatchControl({ command: 'library.instrument.list', params: {} });
}

async function setInstrumentFavorite(
  instrumentId: string,
  favorite: boolean,
): Promise<InstrumentLibraryItem> {
  return dispatchControl({
    command: 'library.instrument.favorite.set',
    params: { instrumentId, favorite },
  });
}

async function setInstrumentCategoryOverride(
  instrumentId: string,
  category: string | null,
): Promise<InstrumentLibraryItem> {
  return dispatchControl({
    command: 'library.instrument.category.set',
    params: { instrumentId, category },
  });
}

async function setInstrumentUserTags(
  instrumentId: string,
  tags: string[],
): Promise<InstrumentLibraryItem> {
  return dispatchControl({
    command: 'library.instrument.tags.set',
    params: { instrumentId, tags },
  });
}

async function listInstrumentCategories(): Promise<string[]> {
  return dispatchControl({ command: 'library.instrument.category.list', params: {} });
}

async function listInstrumentCollections(): Promise<InstrumentCollection[]> {
  return dispatchControl({ command: 'library.instrument.collection.list', params: {} });
}

async function createInstrumentCollection(name: string): Promise<InstrumentCollection> {
  return dispatchControl({ command: 'library.instrument.collection.create', params: { name } });
}

async function renameInstrumentCollection(id: number, name: string): Promise<InstrumentCollection> {
  return dispatchControl({ command: 'library.instrument.collection.rename', params: { id, name } });
}

async function deleteInstrumentCollection(id: number): Promise<void> {
  await dispatchControl({ command: 'library.instrument.collection.delete', params: { id } });
}

async function setInstrumentCollectionMembership(
  collectionId: number,
  instrumentId: string,
  included: boolean,
): Promise<InstrumentLibraryItem> {
  return dispatchControl({
    command: 'library.instrument.collection.membership.set',
    params: { collectionId, instrumentId, included },
  });
}

export const instrumentLibraryApi: InstrumentLibraryApi = {
  listInstruments,
  setInstrumentFavorite,
  setInstrumentCategoryOverride,
  setInstrumentUserTags,
  listInstrumentCategories,
  listInstrumentCollections,
  createInstrumentCollection,
  renameInstrumentCollection,
  deleteInstrumentCollection,
  setInstrumentCollectionMembership,
};
