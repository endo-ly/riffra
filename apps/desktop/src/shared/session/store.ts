import { createStore } from 'zustand/vanilla';
import type { StoreApi } from 'zustand/vanilla';
import type { NativeApi } from '@/native/native-api';
import { createSessionCommit, type SessionCommit } from './commit';
import { createRuntimeSlice, type RuntimeSlice } from './slices/runtime';
import { createAudioSlice, type AudioSlice } from './slices/audio';
import { createHostSlice, type HostSlice } from './slices/host';
import { createTransportSlice, type TransportSlice } from './slices/transport';
import { createJobSlice, type JobSlice } from './slices/jobs';
import { createRecordingSlice, type RecordingSlice } from './slices/recording';
import { createCatalogSlice, type CatalogSlice } from './slices/catalog';
import { createMissingSlice, type MissingSlice } from './slices/missing';
import {
  createWorkspaceSlice,
  initialWorkspace,
  reconcileWorkspace,
  type WorkspaceSlice,
} from './slices/workspace';
import { createProjectSlice, type ProjectSlice } from './slices/project';

export interface SessionState {
  project: ProjectSlice;
  runtime: RuntimeSlice;
  audio: AudioSlice;
  host: HostSlice;
  transport: TransportSlice;
  jobs: JobSlice;
  recording: RecordingSlice;
  catalog: CatalogSlice;
  missing: MissingSlice;
  workspace: WorkspaceSlice;
  commit: SessionCommit;
}

export type SessionStore = StoreApi<SessionState>;

export function createSessionStore(api: NativeApi): SessionStore {
  return createStore<SessionState>()((set, get) => {
    return {
      host: createHostSlice(
        api,
        () => get().host,
        (patch) => set((state) => ({ host: { ...state.host, ...patch } })),
        () => {
          get().project.reset();
          get().audio.reset();
          get().runtime.reset();
          get().transport.reset();
          get().jobs.reset();
          get().recording.reset();
          get().catalog.reset();
          get().missing.reset();
          get().workspace.reset();
        },
      ),
      workspace: createWorkspaceSlice(
        () => get().workspace,
        (patch) => set((state) => ({ workspace: { ...state.workspace, ...patch } })),
        () => get().project.boot?.canonical.session ?? null,
      ),
      missing: createMissingSlice(
        api,
        () => get().missing,
        (patch) => set((state) => ({ missing: { ...state.missing, ...patch } })),
        (operation, options) => get().commit(operation, options),
        () => get().catalog.scanPlugins(),
      ),
      catalog: createCatalogSlice(
        api,
        () => get().catalog,
        (patch) => set((state) => ({ catalog: { ...state.catalog, ...patch } })),
        () => get().jobs,
        () => get().project.boot?.vst3Root,
        (status) => get().audio.setStatus(status),
      ),
      recording: createRecordingSlice(
        api,
        () => get().recording,
        (patch) => set((state) => ({ recording: { ...state.recording, ...patch } })),
        () => get().audio.status,
        (status) => get().audio.setStatus(status),
        (operation, options) => get().commit(operation, options),
      ),
      jobs: createJobSlice(api, (patch) => set((state) => ({ jobs: { ...state.jobs, ...patch } }))),
      transport: createTransportSlice(
        api,
        () => get().transport,
        (patch) => set((state) => ({ transport: { ...state.transport, ...patch } })),
        () => !!get().project.boot?.canonical.session,
      ),
      audio: createAudioSlice(
        api,
        () => get().audio,
        (patch) => {
          set((state) => ({ audio: { ...state.audio, ...patch } }));
          if (patch.status) get().recording.acceptAudio(patch.status);
        },
      ),
      runtime: createRuntimeSlice(
        api,
        () => get().runtime,
        (patch) => set((state) => ({ runtime: { ...state.runtime, ...patch } })),
        {
          ready: () => {
            const state = get();
            return (
              state.host.connection.mode !== 'disconnected' &&
              !state.host.switching &&
              !!state.project.boot?.nativeAvailable &&
              !state.project.boot.safeMode &&
              state.runtime.startupFinished &&
              !state.jobs.active &&
              !state.jobs.activeJobId.current
            );
          },
          scan: () => get().catalog.scanPlugins(),
          recover: async () => {
            const generation = get().host.connection.generation;
            const audio = await api.retryStartupRuntime();
            if (get().host.connection.generation === generation) get().audio.setStatus(audio);
          },
        },
      ),
      commit: createSessionCommit((canonical) => get().project.acceptCanonical(canonical)),
      project: createProjectSlice(
        api,
        () => get().project,
        (patch) =>
          set((state) => {
            const project = { ...state.project, ...patch };
            if (!('boot' in patch) || project.boot === state.project.boot) return { project };
            const previousId = state.project.boot?.projectState.activeProjectId;
            const nextId = project.boot?.projectState.activeProjectId;
            const workspace =
              previousId !== nextId
                ? { ...state.workspace, ...initialWorkspace() }
                : project.boot
                  ? reconcileWorkspace(state.workspace, project.boot.canonical.session)
                  : state.workspace;
            return { project, workspace };
          }),
        (operation, options) => get().commit(operation, options),
        (started, finished) => get().runtime.acceptBootstrapStartup(started, finished),
        (plugins) => {
          get().catalog.acceptPlugins(plugins);
          void get().catalog.reloadInstruments();
        },
      ),
    };
  });
}
