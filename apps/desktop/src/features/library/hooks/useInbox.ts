import { notify } from '@/shared/notify';
import { useCallback, useEffect, useState } from 'react';
import type { LibraryAsset, RecordingAsset } from '@/model/generated';
import { useSession, useSessionContext } from '@/session/session-context';

/**
 * Drives the Inbox preservation zone (LIB-003): every unorganized take can be
 * previewed, renamed, tagged, promoted into the library, archived for safe
 * keeping, deleted, and grouped by duplicate audio content. Mutations refresh
 * the inbox list through `reload` so the UI always reflects the filesystem.
 */
export function useInbox() {
  const { api } = useSessionContext();
  const hostGeneration = useSession((state) => state.host.connection.generation);
  const projectId = useSession((state) => state.project.boot?.projectState.activeProjectId ?? null);
  const recording = useSession((state) => state.recording);
  const missing = useSession((state) => state.missing);
  const recordings = recording.recordings;
  const reload = recording.reload;
  const onRelocate = missing.clearRelocated;
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [duplicateGroups, setDuplicateGroups] = useState<string[][]>([]);
  useEffect(() => {
    setSelectedId(null);
    setDuplicateGroups([]);
  }, [hostGeneration, projectId]);

  const selected = recordings.find((recording) => recording.id === selectedId) ?? null;

  const rename = useCallback(
    async (id: string, name: string) => {
      try {
        const recording = recordings.find((item) => item.id === id);
        const nextId = await api.renameRecording(id, name);
        if (recording) onRelocate(recording);
        await reload();
        setSelectedId(nextId);
        notify.success('Recording renamed', { message: name });
      } catch (cause) {
        notify.error('Rename failed', { cause });
        throw cause;
      }
    },
    [api, onRelocate, recordings, reload],
  );

  const remove = useCallback(
    async (id: string) => {
      try {
        await api.deleteRecording(id);
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        notify.success('Recording deleted');
      } catch (cause) {
        notify.error('Recording deletion failed', { cause });
        throw cause;
      }
    },
    [api, reload],
  );

  const archive = useCallback(
    async (id: string) => {
      try {
        const recording = recordings.find((item) => item.id === id);
        await api.archiveRecording(id);
        if (recording) onRelocate(recording);
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        notify.success('Recording archived');
      } catch (cause) {
        notify.error('Archive failed', { cause });
        throw cause;
      }
    },
    [api, onRelocate, recordings, reload],
  );

  const promote = useCallback(
    async (id: string) => {
      try {
        const recording = recordings.find((item) => item.id === id);
        await api.promoteRecording(id);
        if (recording) onRelocate(recording);
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        notify.success('Recording added to the library');
      } catch (cause) {
        notify.error('Adding to the library failed', { cause });
        throw cause;
      }
    },
    [api, onRelocate, recordings, reload],
  );

  const tag = useCallback(
    async (id: string, tag: string | null, note: string | null): Promise<LibraryAsset> => {
      try {
        const updated = await api.tagRecording(id, tag, note);
        await reload();
        notify.success('Recording tag saved');
        return updated;
      } catch (cause) {
        notify.error('Tag could not be saved', { cause });
        throw cause;
      }
    },
    [api, reload],
  );

  const preview = useCallback(
    async (recording: RecordingAsset) => {
      try {
        const assetId = recording.processedAssetId ?? recording.rawAssetId;
        if (!assetId) throw new Error('Recording has no canonical audio Asset ID.');
        await api.previewAsset(assetId, {});
      } catch (cause) {
        notify.error('Preview failed', { cause });
        throw cause;
      }
    },
    [api],
  );

  const detectDuplicates = useCallback(async () => {
    try {
      const groups = await api.detectDuplicateRecordings();
      setDuplicateGroups(groups);
      const count = new Set(groups.flat()).size;
      if (groups.length === 0) notify.info('No duplicate recordings found');
      else
        notify.info(`${groups.length} duplicate group${groups.length === 1 ? '' : 's'} found`, {
          message: `${count} recordings`,
        });
    } catch (cause) {
      notify.error('Duplicate detection failed', { cause });
      throw cause;
    }
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
