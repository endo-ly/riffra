import type { AssetId, LibraryAsset, RecordingAsset } from '@/model/domain';
import { dispatchControl, invokeHost } from '../invoke';

export interface LibraryApi {
  listRecordings(query?: string): Promise<RecordingAsset[]>;
  renameRecording(id: string, name: string): Promise<string>;
  deleteRecording(id: string): Promise<void>;
  archiveRecording(id: string): Promise<string>;
  promoteRecording(id: string): Promise<string>;
  tagRecording(id: string, tag: string | null, note: string | null): Promise<LibraryAsset>;
  detectDuplicateRecordings(): Promise<string[][]>;
  searchLibrary(query: string): Promise<LibraryAsset[]>;
  updateLibraryAsset(id: string, tag: string | null, note: string | null): Promise<LibraryAsset>;
  importMidiFile(path: string, name?: string): Promise<AssetId>;
  importMidiBytes(name: string, bytes: number[]): Promise<AssetId>;
}

async function listRecordings(query?: string): Promise<RecordingAsset[]> {
  return dispatchControl({ command: 'record.list', params: { query: query ?? null } });
}

async function renameRecording(id: string, name: string): Promise<string> {
  return dispatchControl({ command: 'record.rename', params: { id, newName: name } });
}

async function deleteRecording(id: string): Promise<void> {
  await dispatchControl({ command: 'record.delete', params: { id } });
}

async function archiveRecording(id: string): Promise<string> {
  return dispatchControl({ command: 'record.archive', params: { id } });
}

async function promoteRecording(id: string): Promise<string> {
  return dispatchControl({ command: 'record.promote', params: { id } });
}

async function tagRecording(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset> {
  return dispatchControl({ command: 'record.tag', params: { id, tag, note } });
}

async function detectDuplicateRecordings(): Promise<string[][]> {
  return dispatchControl({ command: 'record.duplicates', params: {} });
}

async function searchLibrary(query: string): Promise<LibraryAsset[]> {
  if (!query.trim()) return [];
  return dispatchControl({ command: 'library.search', params: { query } });
}

async function updateLibraryAsset(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset> {
  return dispatchControl({ command: 'library.asset.update', params: { id, tag, note } });
}

async function importMidiFile(path: string, name?: string): Promise<AssetId> {
  return dispatchControl({ command: 'asset.import-midi', params: { path, name: name ?? null } });
}

async function importMidiBytes(name: string, bytes: number[]): Promise<AssetId> {
  return invokeHost<AssetId>('import_midi_bytes', { name, bytes });
}

export const libraryApi: LibraryApi = {
  listRecordings,
  renameRecording,
  deleteRecording,
  archiveRecording,
  promoteRecording,
  tagRecording,
  detectDuplicateRecordings,
  searchLibrary,
  updateLibraryAsset,
  importMidiFile,
  importMidiBytes,
};
