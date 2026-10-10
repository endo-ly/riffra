import type {
  ArrangementMutationResult,
  AssetId,
  HistoryState,
  ProjectActivationResult,
  ProjectState,
  ProjectExport,
} from '@/model/domain';
import { defaultProjectState, defaultSession } from '../browser-defaults';
import { dispatchControl, dispatchControlOrFallback, invokeHostOrFallback } from '../invoke';

function defaultArrangementMutation(): ArrangementMutationResult {
  return {
    canonical: {
      projectId: defaultProjectState().activeProjectId,
      session: defaultSession(),
      sequence: 0,
      history: { canUndo: false, canRedo: false },
    },
    createdEntityIds: {},
    projection: { state: 'notRequired' },
  };
}

export async function undoSession(): Promise<ArrangementMutationResult> {
  return dispatchControlOrFallback({ command: 'undo', params: {} }, defaultArrangementMutation());
}

export async function redoSession(): Promise<ArrangementMutationResult> {
  return dispatchControlOrFallback({ command: 'redo', params: {} }, defaultArrangementMutation());
}

export async function getHistoryState(): Promise<HistoryState> {
  return dispatchControlOrFallback(
    { command: 'history.get', params: {} },
    { canUndo: false, canRedo: false },
  );
}

export async function restoreRecoveryGeneration(
  fileName: string,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'project.restore-generation', params: { fileName } },
    null,
  );
}

export async function exportProject(path: string): Promise<ProjectExport | null> {
  return dispatchControlOrFallback({ command: 'project.export', params: { output: path } }, null);
}

export async function listProjects(): Promise<ProjectState> {
  return dispatchControlOrFallback({ command: 'project.list', params: {} }, defaultProjectState());
}

export async function createProject(name?: string): Promise<ProjectActivationResult> {
  return dispatchControlOrFallback(
    { command: 'project.create', params: { name: name ?? null } },
    defaultProjectActivationResult(),
  );
}

export async function openProject(projectId: string): Promise<ProjectActivationResult> {
  return dispatchControlOrFallback(
    { command: 'project.open', params: { projectId } },
    defaultProjectActivationResult(),
  );
}

export async function renameProject(name: string): Promise<ProjectState> {
  return dispatchControlOrFallback(
    { command: 'project.rename', params: { name } },
    defaultProjectState(),
  );
}

export async function importProject(path: string): Promise<ProjectActivationResult | null> {
  return dispatchControlOrFallback({ command: 'project.import', params: { path } }, null);
}

export async function importSonalloyBundle(path: string): Promise<ProjectActivationResult | null> {
  return dispatchControlOrFallback({ command: 'project.import-sonalloy', params: { path } }, null);
}

function defaultProjectActivationResult(): ProjectActivationResult {
  return {
    projectState: defaultProjectState(),
    canonical: {
      projectId: defaultProjectState().activeProjectId,
      session: defaultSession(),
      sequence: 0,
      history: { canUndo: false, canRedo: false },
    },
    recovery: { recoveredFromGeneration: false, recoveryCandidates: [] },
  };
}

export async function importMidiFile(path: string, name?: string): Promise<AssetId | null> {
  return dispatchControlOrFallback(
    { command: 'asset.import-midi', params: { path, name: name ?? null } },
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
  return dispatchControl({ command: 'session.settings.update', params: patch });
}
