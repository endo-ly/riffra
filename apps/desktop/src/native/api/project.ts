import type {
  ArrangementMutationResult,
  ProjectActivationResult,
  ProjectExport,
  ProjectState,
} from '@/model/domain';
import type { SessionSettingsPatch } from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface ProjectApi {
  undoSession(): Promise<ArrangementMutationResult>;
  redoSession(): Promise<ArrangementMutationResult>;
  restoreRecoveryGeneration(fileName: string): Promise<ArrangementMutationResult>;
  exportProject(path: string): Promise<ProjectExport>;
  listProjects(): Promise<ProjectState>;
  createProject(name?: string): Promise<ProjectActivationResult>;
  openProject(projectId: string): Promise<ProjectActivationResult>;
  renameProject(name: string): Promise<ProjectState>;
  importProject(path: string): Promise<ProjectActivationResult>;
  updateSessionSettings(patch: SessionSettingsPatch): Promise<ArrangementMutationResult>;
  importScore(path: string, bindings: Record<string, string>): Promise<ProjectActivationResult>;
}

async function undoSession(): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'undo', params: {} });
}

async function redoSession(): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'redo', params: {} });
}

async function restoreRecoveryGeneration(fileName: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'project.restore-generation', params: { fileName } });
}

async function exportProject(path: string): Promise<ProjectExport> {
  return dispatchControl({ command: 'project.export', params: { output: path } });
}

async function listProjects(): Promise<ProjectState> {
  return dispatchControl({ command: 'project.list', params: {} });
}

async function createProject(name?: string): Promise<ProjectActivationResult> {
  return dispatchControl({ command: 'project.create', params: { name: name ?? null } });
}

async function openProject(projectId: string): Promise<ProjectActivationResult> {
  return dispatchControl({ command: 'project.open', params: { projectId } });
}

async function renameProject(name: string): Promise<ProjectState> {
  return dispatchControl({ command: 'project.rename', params: { name } });
}

async function importProject(path: string): Promise<ProjectActivationResult> {
  return dispatchControl({ command: 'project.import', params: { path } });
}

async function updateSessionSettings(
  patch: SessionSettingsPatch,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'session.settings.update', params: patch });
}

async function importScore(
  path: string,
  bindings: Record<string, string>,
): Promise<ProjectActivationResult> {
  return dispatchControl({ command: 'project.import-score', params: { path, bindings } });
}

export const projectApi: ProjectApi = {
  undoSession,
  redoSession,
  restoreRecoveryGeneration,
  exportProject,
  listProjects,
  createProject,
  openProject,
  renameProject,
  importProject,
  updateSessionSettings,
  importScore,
};
