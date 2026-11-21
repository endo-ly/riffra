import { createStore } from 'zustand/vanilla';
import { subscribeWithSelector } from 'zustand/middleware';
import { buildSessionReadModel, type SessionReadModel } from '@/model/session-read-model';
import type { Mutate, StoreApi } from 'zustand/vanilla';
import type { NativeApi } from '@/native/native-api';
import { createSessionCommit, type SessionCommit } from '@/session/commit';
import { createRuntimeSlice, type RuntimeSlice } from '@/session/slices/runtime';
import { createAudioSlice, type AudioSlice } from '@/session/slices/audio';
import { createHostSlice, type HostSlice } from '@/session/slices/host';
import { createTransportSlice, type TransportSlice } from '@/session/slices/transport';
import { createJobSlice, type JobSlice } from '@/session/slices/jobs';
import { createRecordingSlice, type RecordingSlice } from '@/session/slices/recording';
import { createCatalogSlice, type CatalogSlice } from '@/session/slices/catalog';
import { createMissingSlice, type MissingSlice } from '@/session/slices/missing';
import {
  createWorkspaceSlice,
  initialWorkspace,
  reconcileWorkspace,
  type WorkspaceSlice,
} from '@/session/slices/workspace';
import { createProjectSlice, type ProjectSlice } from '@/session/slices/project';
import { selectStartupRestoreReady } from '@/session/selectors';

export interface SessionScope {
  readonly generation: number;
  readonly hostGeneration: number;
  readonly projectId: string | null;
  readonly sessionId: string | null;
}

export interface SessionState {
  scope: SessionScope;
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
  readModel: SessionReadModel | null;
}

export type SessionStore = Mutate<
  StoreApi<SessionState>,
  [['zustand/subscribeWithSelector', never]]
>;

export function createSessionStore(api: NativeApi): SessionStore {
  return createStore<SessionState>()(
    subscribeWithSelector((set, get) => {
      return {
        scope: { generation: 0, hostGeneration: 0, projectId: null, sessionId: null },
        readModel: null,
        host: createHostSlice(
          api,
          () => get().host,
          (patch) =>
            set((state) => {
              const host = { ...state.host, ...patch };
              const scope =
                host.connection.generation === state.scope.hostGeneration
                  ? state.scope
                  : {
                      ...state.scope,
                      generation: state.scope.generation + 1,
                      hostGeneration: host.connection.generation,
                    };
              return { host, scope };
            }),
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
        jobs: createJobSlice(api, (patch) =>
          set((state) => ({ jobs: { ...state.jobs, ...patch } })),
        ),
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
            ready: () => selectStartupRestoreReady(get()),
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
              const session = project.boot?.canonical.session;
              const previousSession = state.project.boot?.canonical.session;
              const sameSession =
                previousId === nextId && previousSession?.sessionId === session?.sessionId;
              const readModel = session
                ? buildSessionReadModel(sameSession ? state.readModel : null, session)
                : null;
              const workspace = !sameSession
                ? { ...state.workspace, ...initialWorkspace() }
                : session && session !== previousSession
                  ? reconcileWorkspace(state.workspace, session, previousSession!)
                  : state.workspace;
              const scope = sameSession
                ? state.scope
                : {
                    generation: state.scope.generation + 1,
                    hostGeneration: state.host.connection.generation,
                    projectId: nextId ?? null,
                    sessionId: session?.sessionId ?? null,
                  };
              return { project, workspace, readModel, scope };
            }),
          (operation, options) => get().commit(operation, options),
          (started, finished) => get().runtime.acceptBootstrapStartup(started, finished),
          (plugins) => {
            get().catalog.acceptPlugins(plugins);
            void get().catalog.reloadInstruments();
          },
        ),
      };
    }),
  );
}
