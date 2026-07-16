import { useCallback, useState } from 'react';
import type { LibraryAsset, RecordingAsset } from '@/lib/domain';
import { audioCommandSucceeded } from '@/lib/audio-safety';
import type { NativeApi } from '@/native/native-api';

interface UseInboxOptions {
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
  api: NativeApi,
  recordings: RecordingAsset[],
  { reload, onRelocate }: UseInboxOptions,
) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [duplicateGroups, setDuplicateGroups] = useState<string[][]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const selected = recordings.find((recording) => recording.id === selectedId) ?? null;

  const rename = useCallback(
    async (id: string, name: string) => {
      setError(null);
      try {
        const recording = recordings.find((item) => item.id === id);
        const nextId = await api.renameRecording(id, name);
        if (recording) onRelocate?.(recording, nextId);
        await reload();
        setSelectedId(nextId);
        setMessage(`Renamed to ${name}.`);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, onRelocate, recordings, reload],
  );

  const remove = useCallback(
    async (id: string) => {
      setError(null);
      try {
        await api.deleteRecording(id);
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        setMessage('Recording deleted.');
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, reload],
  );

  const archive = useCallback(
    async (id: string) => {
      setError(null);
      try {
        const recording = recordings.find((item) => item.id === id);
        const nextId = await api.archiveRecording(id);
        if (recording) onRelocate?.(recording, nextId);
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        setMessage('Recording archived.');
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, onRelocate, recordings, reload],
  );

  const promote = useCallback(
    async (id: string) => {
      setError(null);
      try {
        const recording = recordings.find((item) => item.id === id);
        const nextId = await api.promoteRecording(id);
        if (recording) onRelocate?.(recording, nextId);
        setSelectedId((current) => (current === id ? null : current));
        await reload();
        setMessage('Recording promoted to the library.');
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, onRelocate, recordings, reload],
  );

  const tag = useCallback(
    async (id: string, tag: string | null, note: string | null): Promise<LibraryAsset | null> => {
      setError(null);
      try {
        const updated = await api.tagRecording(id, tag, note);
        if (!updated) throw new Error('The recording tag was not saved.');
        await reload();
        setMessage('Recording tag saved.');
        return updated;
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api, reload],
  );

  const preview = useCallback(
    async (recording: RecordingAsset) => {
      setError(null);
      try {
        const assetId = recording.processedAssetId ?? recording.rawAssetId;
        if (!assetId) throw new Error('Recording has no canonical audio Asset ID.');
        const path = await api.resolveAssetContentLocation(assetId);
        if (!path) throw new Error('Recording has no previewable audio file.');
        const status = await api.previewSample(path, 0, 0);
        if (!audioCommandSucceeded(status)) {
          throw new Error(status.message || 'The audio engine could not start the preview.');
        }
        setMessage(`Preview started: ${recording.name}.`);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        throw cause;
      }
    },
    [api],
  );

  const detectDuplicates = useCallback(async () => {
    setError(null);
    try {
      const groups = await api.detectDuplicateRecordings();
      setDuplicateGroups(groups);
      const count = new Set(groups.flat()).size;
      setMessage(
        groups.length === 0
          ? 'No duplicate recordings found.'
          : `${groups.length} duplicate group${groups.length === 1 ? '' : 's'} found (${count} recordings).`,
      );
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
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
