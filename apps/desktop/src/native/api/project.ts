import type {
  ArrangementMutationResult,
  AssetId,
  HistoryState,
  MetronomeSound,
  ProjectActivationResult,
  ProjectState,
  ProjectExport,
} from '@/model/domain';
import { dispatchControl, invokeHost } from '../invoke';

export async function undoSession(): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'undo', params: {} });
}

export async function redoSession(): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'redo', params: {} });
}

export async function getHistoryState(): Promise<HistoryState> {
  return dispatchControl({ command: 'history.get', params: {} });
}

export async function restoreRecoveryGeneration(
  fileName: string,
): Promise<ArrangementMutationResult | null> {
  return dispatchControl({ command: 'project.restore-generation', params: { fileName } });
}

export async function exportProject(path: string): Promise<ProjectExport | null> {
  return dispatchControl({ command: 'project.export', params: { output: path } });
}

export async function listProjects(): Promise<ProjectState> {
  return dispatchControl({ command: 'project.list', params: {} });
}

export async function createProject(name?: string): Promise<ProjectActivationResult> {
  return dispatchControl({ command: 'project.create', params: { name: name ?? null } });
}

export async function openProject(projectId: string): Promise<ProjectActivationResult> {
  return dispatchControl({ command: 'project.open', params: { projectId } });
}

export async function renameProject(name: string): Promise<ProjectState> {
  return dispatchControl({ command: 'project.rename', params: { name } });
}

export async function importProject(path: string): Promise<ProjectActivationResult | null> {
  return dispatchControl({ command: 'project.import', params: { path } });
}

export async function importMidiFile(path: string, name?: string): Promise<AssetId | null> {
  return dispatchControl({ command: 'asset.import-midi', params: { path, name: name ?? null } });
}

export async function importMidiBytes(name: string, bytes: number[]): Promise<AssetId | null> {
  return invokeHost<AssetId | null>('import_midi_bytes', { name, bytes });
}

export async function updateSessionSettings(patch: {
  projectName?: string | null;
  loopEnabled?: boolean;
  countInBeats?: number;
  metronomeEnabled?: boolean;
  metronomeSound?: MetronomeSound;
  note?: string;
}): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'session.settings.update', params: patch });
}

export async function importScore(
  path: string,
  bindings: Record<string, string>,
): Promise<ProjectActivationResult | null> {
  return dispatchControl({ command: 'project.import-score', params: { path, bindings } });
}
