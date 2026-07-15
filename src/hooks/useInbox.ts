import { useCallback, useState } from 'react';
import type { LibraryAsset, RecordingAsset } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

interface UseInboxOptions {
  reload: () => void | Promise<void>;
}

/**
 * Drives the Inbox preservation zone (LIB-003): every unorganized take can be
 * previewed, renamed, tagged, promoted into the library, archived for safe
 * keeping, deleted, and grouped by duplicate audio content. Mutations refresh
 * the inbox list through `reload` so the UI always reflects the filesystem.
 */
export function useInbox(
  api: NativeApi,
  recordings: RecordingAsset[],
  { reload }: UseInboxOptions,
) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [duplicateGroups, setDuplicateGroups] = useState<string[][]>([]);

  const selected = recordings.find((recording) => recording.id === selectedId) ?? null;

  const rename = useCallback(
    async (id: string, name: string) => {
      await api.renameRecording(id, name);
      await reload();
    },
    [api, reload],
  );

  const remove = useCallback(
    async (id: string) => {
      await api.deleteRecording(id);
      setSelectedId((current) => (current === id ? null : current));
      await reload();
    },
    [api, reload],
  );

  const archive = useCallback(
    async (id: string) => {
      await api.archiveRecording(id);
      setSelectedId((current) => (current === id ? null : current));
      await reload();
    },
    [api, reload],
  );

  const promote = useCallback(
    async (id: string) => {
      await api.promoteRecording(id);
      setSelectedId((current) => (current === id ? null : current));
      await reload();
    },
    [api, reload],
  );

  const tag = useCallback(
    async (id: string, tag: string | null, note: string | null): Promise<LibraryAsset | null> => {
      const updated = await api.tagRecording(id, tag, note);
      if (!updated) return null;
      await reload();
      return updated;
    },
    [api, reload],
  );

  const preview = useCallback(
    async (recording: RecordingAsset) => {
      const path = recording.processedPath ?? recording.rawPath;
      if (!path) throw new Error('Recording has no previewable audio file.');
      await api.previewSample(path, 0, 0);
    },
    [api],
  );

  const detectDuplicates = useCallback(async () => {
    setDuplicateGroups(await api.detectDuplicateRecordings());
  }, [api]);

  const duplicateIds = new Set(duplicateGroups.flat());

  return {
    selectedId,
    setSelectedId,
    selected,
    duplicateGroups,
    duplicateIds,
    rename,
    remove,
    archive,
    promote,
    tag,
    preview,
    detectDuplicates,
  };
}

export type InboxController = ReturnType<typeof useInbox>;
