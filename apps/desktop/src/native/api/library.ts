import type { LibraryAsset, RecordingAsset } from '@/model/domain';
import { invokeOrFallback, invoke } from '../invoke';

export async function listRecordings(query?: string): Promise<RecordingAsset[]> {
  return invokeOrFallback<RecordingAsset[]>('list_recordings', { query: query ?? null }, []);
}

export async function renameRecording(id: string, name: string): Promise<string> {
  return invoke<string>('rename_recording', { id, newName: name });
}

export async function deleteRecording(id: string): Promise<void> {
  await invoke('delete_recording', { id });
}

export async function archiveRecording(id: string): Promise<string> {
  return await invoke<string>('archive_recording', { id });
}

export async function promoteRecording(id: string): Promise<string> {
  return await invoke<string>('promote_recording', { id });
}

export async function tagRecording(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  return await invoke<LibraryAsset>('tag_recording', { id, tag, note });
}

export async function detectDuplicateRecordings(): Promise<string[][]> {
  return await invoke<string[][]>('detect_duplicate_recordings');
}

export async function searchLibrary(query: string): Promise<LibraryAsset[]> {
  if (!query.trim()) return [];
  return invokeOrFallback<LibraryAsset[]>('search_library', { query }, []);
}

export async function updateLibraryAsset(
  id: string,
  tag: string | null,
  note: string | null,
): Promise<LibraryAsset | null> {
  return invokeOrFallback<LibraryAsset | null>('update_library_asset', { id, tag, note }, null);
}

export async function relatedLibraryAssets(id: string): Promise<LibraryAsset[]> {
  return invokeOrFallback<LibraryAsset[]>('related_library_assets', { id }, []);
}
