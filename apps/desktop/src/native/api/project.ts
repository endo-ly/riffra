import type {
  ArrangementMutationResult,
  AssetId,
  HistoryState,
  ProjectExport,
} from '@/model/domain';
import { defaultSession } from '../browser-defaults';
import { invokeOrFallback, invoke } from '../invoke';

export async function undoSession(): Promise<ArrangementMutationResult> {
  const session = defaultSession();
  return invokeOrFallback<ArrangementMutationResult>(
    'undo_session',
    {},
    {
      canonical: {
        session,
        sequence: 0,
        history: { canUndo: false, canRedo: false },
      },
      projection: { state: 'notRequired' },
    },
  );
}

export async function redoSession(): Promise<ArrangementMutationResult> {
  const session = defaultSession();
  return invokeOrFallback<ArrangementMutationResult>(
    'redo_session',
    {},
    {
      canonical: {
        session,
        sequence: 0,
        history: { canUndo: false, canRedo: false },
      },
      projection: { state: 'notRequired' },
    },
  );
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

export async function restoreRecoveryGeneration(
  fileName: string,
): Promise<ArrangementMutationResult | null> {
  return invokeOrFallback<ArrangementMutationResult | null>(
    'restore_recovery_generation',
    { fileName },
    null,
  );
}

export async function exportSession(): Promise<ProjectExport | null> {
  return invokeOrFallback<ProjectExport | null>('export_scratch_session', {}, null);
}

export async function importSession(path: string): Promise<ArrangementMutationResult | null> {
  return invokeOrFallback<ArrangementMutationResult | null>(
    'import_scratch_session',
    { path },
    null,
  );
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
}): Promise<ArrangementMutationResult> {
  const result = await invoke<ArrangementMutationResult>('update_session_settings', { patch });
  return result;
}
