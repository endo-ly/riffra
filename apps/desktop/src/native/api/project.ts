import type { AssetId, HistoryState, ProjectExport, CreativeSession } from '@/model/domain';
import { defaultSession } from '../browser-defaults';
import { invokeOrFallback, invoke } from '../invoke';

export async function undoSession(): Promise<CreativeSession> {
  return invokeOrFallback<CreativeSession>('undo_session', {}, defaultSession());
}

export async function redoSession(): Promise<CreativeSession> {
  return invokeOrFallback<CreativeSession>('redo_session', {}, defaultSession());
}

export async function getHistoryState(): Promise<HistoryState> {
  return invokeOrFallback<HistoryState>(
    'get_history_state',
    {},
    {
      canUndo: false,
      canRedo: false,
    },
  );
}

export async function restoreRecoveryGeneration(fileName: string): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'restore_recovery_generation',
    { fileName },
    null,
  );
}

export async function exportSession(): Promise<ProjectExport | null> {
  return invokeOrFallback<ProjectExport | null>('export_scratch_session', {}, null);
}

export async function importSession(path: string): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('import_scratch_session', { path }, null);
}

export async function importMidiFile(path: string, name?: string): Promise<AssetId | null> {
  return invokeOrFallback<AssetId | null>('import_midi_file', { path, name: name ?? null }, null);
}

export async function importMidiBytes(name: string, bytes: number[]): Promise<AssetId | null> {
  return invokeOrFallback<AssetId | null>('import_midi_bytes', { name, bytes }, null);
}

export async function updateSessionSettings(patch: {
  projectName?: string | null;
  loopEnabled?: boolean;
  countInBeats?: number;
  metronomeEnabled?: boolean;
  note?: string;
}): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_session_settings', { patch });
}
