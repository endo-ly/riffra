import { useCallback, useEffect, useRef, useState } from 'react';
import type { LibraryAsset, RecordingAsset } from '@/model/domain';
import { audioCommandSucceeded } from '@/shared/audio/audio-safety';
import type { AudioApi, LibraryApi } from '@/native/native-api';

interface UseInboxOptions {
  hostGeneration?: number;
  reload: () => void | Promise<void>;
  onRelocate?: (recording: RecordingAsset, nextId: string) => void;
}

/**
 * Drives the Inbox preservation zone (LIB-003): every unorganized take can be
 * previewed, renamed, tagged, promoted into the library, archived for safe
 * keeping, deleted, and grouped by duplicate audio content. Mutations refresh
 * the inbox list through `reload` so the UI always reflects the filesystem.
 */
export function useInbox(
  api: LibraryApi & AudioApi,
  recordings: RecordingAsset[],
  { hostGeneration = 0, reload, onRelocate }: UseInboxOptions,
) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [duplicateGroups, setDuplicateGroups] = useState<string[][]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  useEffect(() => {
    setSelectedId(null);
    setDuplicateGroups([]);
    setMessage(null);
    setError(null);
  }, [hostGeneration]);

  const selected = recordings.find((recording) => recording.id === selectedId) ?? null;

  const rename = useCallback(
    async (id: string, name: string) => {
      const requestGeneration = hostGeneration;
      setError(null);
      try {
        const recording = recordings.find((item) => item.id === id);
        const nextId = await api.renameRecording(id, name);
        if (currentHostGeneration.current !== requestGeneration) return;
        if (recording) onRelocate?.(recording, nextId);
        await reload();
        if (currentHostGeneration.current !== requestGeneration) return;
        setSelectedId(nextId);
        setMessage(`Renamed to ${name}.`);
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, hostGeneration, onRelocate, recordings, reload],
  );

  const remove = useCallback(
    async (id: string) => {
      const requestGeneration = hostGeneration;
      setError(null);
      try {
        await api.deleteRecording(id);
        if (currentHostGeneration.current !== requestGeneration) return;
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        if (currentHostGeneration.current !== requestGeneration) return;
        setMessage('Recording deleted.');
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, hostGeneration, reload],
  );

  const archive = useCallback(
    async (id: string) => {
      const requestGeneration = hostGeneration;
      setError(null);
      try {
        const recording = recordings.find((item) => item.id === id);
        const nextId = await api.archiveRecording(id);
        if (currentHostGeneration.current !== requestGeneration) return;
        if (recording) onRelocate?.(recording, nextId);
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        if (currentHostGeneration.current !== requestGeneration) return;
        setMessage('Recording archived.');
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, hostGeneration, onRelocate, recordings, reload],
  );

  const promote = useCallback(
    async (id: string) => {
      const requestGeneration = hostGeneration;
      setError(null);
      try {
        const recording = recordings.find((item) => item.id === id);
        const nextId = await api.promoteRecording(id);
        if (currentHostGeneration.current !== requestGeneration) return;
        if (recording) onRelocate?.(recording, nextId);
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        if (currentHostGeneration.current !== requestGeneration) return;
        setMessage('Recording promoted to the library.');
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, hostGeneration, onRelocate, recordings, reload],
  );

  const tag = useCallback(
    async (id: string, tag: string | null, note: string | null): Promise<LibraryAsset | null> => {
      const requestGeneration = hostGeneration;
      setError(null);
      try {
        const updated = await api.tagRecording(id, tag, note);
        if (currentHostGeneration.current !== requestGeneration) return null;
        if (!updated) throw new Error('The recording tag was not saved.');
        await reload();
        if (currentHostGeneration.current !== requestGeneration) return null;
        setMessage('Recording tag saved.');
        return updated;
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return null;
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, hostGeneration, reload],
  );

  const preview = useCallback(
    async (recording: RecordingAsset) => {
      const requestGeneration = hostGeneration;
      setError(null);
      try {
        const assetId = recording.processedAssetId ?? recording.rawAssetId;
        if (!assetId) throw new Error('Recording has no canonical audio Asset ID.');
        const status = await api.previewAsset(assetId, {});
        if (currentHostGeneration.current !== requestGeneration) return;
        if (!audioCommandSucceeded(status)) {
          throw new Error(status.message || 'The audio engine could not start the preview.');
        }
        setMessage(`Preview started: ${recording.name}.`);
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, hostGeneration],
  );

  const detectDuplicates = useCallback(async () => {
    const requestGeneration = hostGeneration;
    setError(null);
    try {
      const groups = await api.detectDuplicateRecordings();
      if (currentHostGeneration.current !== requestGeneration) return;
      setDuplicateGroups(groups);
      const count = new Set(groups.flat()).size;
      setMessage(
        groups.length === 0
          ? 'No duplicate recordings found.'
          : `${groups.length} duplicate group${groups.length === 1 ? '' : 's'} found (${count} recordings).`,
      );
    } catch (cause) {
      if (currentHostGeneration.current !== requestGeneration) return;
      setError(cause instanceof Error ? cause.message : String(cause));
      throw cause;
    }
  }, [api, hostGeneration]);

  const duplicateIds = new Set(duplicateGroups.flat());

  return {
    selectedId,
    setSelectedId,
    selected,
    duplicateGroups,
    duplicateIds,
    message,
    error,
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
