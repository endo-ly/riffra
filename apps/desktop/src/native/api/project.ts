import type {
  ArrangementMutationResult,
  AssetId,
  HistoryState,
  ProjectActivationResult,
  ProjectState,
  ProjectExport,
} from '@/model/domain';
import { defaultProjectState, defaultSession } from '../browser-defaults';
import { invokeHostOrFallback, invokeHost } from '../invoke';

export async function undoSession(): Promise<ArrangementMutationResult> {
  const session = defaultSession();
  return invokeHostOrFallback<ArrangementMutationResult>(
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
  return invokeHostOrFallback<ArrangementMutationResult>(
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
  return invokeHostOrFallback<HistoryState>(
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
  return invokeHostOrFallback<ArrangementMutationResult | null>(
    'restore_recovery_generation',
    { fileName },
    null,
  );
}

export async function exportProject(path: string): Promise<ProjectExport | null> {
  return invokeHostOrFallback<ProjectExport | null>('export_project', { path }, null);
}

export async function listProjects(): Promise<ProjectState> {
  return invokeHostOrFallback<ProjectState>('list_projects', {}, defaultProjectState());
}

export async function createProject(name?: string): Promise<ProjectActivationResult> {
  return invokeHostOrFallback<ProjectActivationResult>(
    'create_project',
    { name: name ?? null },
    defaultProjectActivationResult(),
  );
}

export async function openProject(projectId: string): Promise<ProjectActivationResult> {
  return invokeHostOrFallback<ProjectActivationResult>(
    'open_project',
    { projectId },
    defaultProjectActivationResult(),
  );
}

export async function renameProject(name: string): Promise<ProjectState> {
  return invokeHostOrFallback<ProjectState>('rename_project', { name }, defaultProjectState());
}

export async function importProject(path: string): Promise<ProjectActivationResult | null> {
  return invokeHostOrFallback<ProjectActivationResult | null>('import_project', { path }, null);
}

function defaultProjectActivationResult(): ProjectActivationResult {
  return {
    projectState: defaultProjectState(),
    canonical: {
      session: defaultSession(),
      sequence: 0,
      history: { canUndo: false, canRedo: false },
    },
    recovery: { recoveredFromGeneration: false, recoveryCandidates: [] },
  };
}

export async function importMidiFile(path: string, name?: string): Promise<AssetId | null> {
  return invokeHostOrFallback<AssetId | null>(
    'import_midi_file',
    { path, name: name ?? null },
    null,
  );
}

export async function importMidiBytes(name: string, bytes: number[]): Promise<AssetId | null> {
  return invokeHostOrFallback<AssetId | null>('import_midi_bytes', { name, bytes }, null);
}

export async function updateSessionSettings(patch: {
  projectName?: string | null;
  loopEnabled?: boolean;
  countInBeats?: number;
  metronomeEnabled?: boolean;
  note?: string;
}): Promise<ArrangementMutationResult> {
  const result = await invokeHost<ArrangementMutationResult>('update_session_settings', { patch });
  return result;
}
