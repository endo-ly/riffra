import { canonicalState, defaultProjectState, defaultSession } from './fixtures';
import { NativeCommandError } from '@/native/invoke';
import type { NativeApi } from '@/native/native-api';
import type { NativeEventApi } from '@/native/api/events';
import { nativeApiMethodNames } from '@/native/api/native-implementation';

const hostConnection = {
  mode: 'disconnected',
  generation: 1,
  dataRoot: null,
  instanceId: null,
  pid: null,
  reason: 'Native Host is unavailable in browser preview',
} as const;

type BrowserPreviewReads = Pick<
  NativeApi,
  | 'bootstrap'
  | 'getHostConnectionState'
  | 'listLocalHosts'
  | 'onHostConnectionChanged'
  | 'onRuntimeStartupFinished'
  | 'checkForAppUpdate'
  | 'listInstruments'
  | 'listInstrumentCategories'
  | 'listInstrumentCollections'
  | 'listRecordings'
  | 'searchLibrary'
  | 'getMissingDependencies'
  | 'getRigState'
  | keyof NativeEventApi
>;

const noop = () => () => undefined;
const reads: BrowserPreviewReads = {
  bootstrap: async () => ({
    canonical: canonicalState(defaultSession()),
    projectState: defaultProjectState(),
    pluginCatalog: [],
    runtimeStarted: false,
    runtimeStartupFinished: false,
    recovery: { recoveredFromGeneration: false, recoveryCandidates: [] },
    safeMode: false,
    nativeAvailable: false,
    dataRoot: 'Browser preview — native persistence is unavailable',
    vst3Root: 'C:\\Program Files\\Common Files\\VST3',
    scoreSync: { state: 'unlinked' },
    hostConnection,
  }),
  getHostConnectionState: async () => hostConnection,
  listLocalHosts: async () => [],
  onHostConnectionChanged: async () => () => undefined,
  onRuntimeStartupFinished: async () => () => undefined,
  checkForAppUpdate: async () => null,
  listInstruments: async () => [],
  listInstrumentCategories: async () => [],
  listInstrumentCollections: async () => [],
  listRecordings: async () => [],
  searchLibrary: async () => [],
  getMissingDependencies: async () => [],
  getRigState: async () => ({ rigs: [], activeRigId: null, lastPlayedRigId: null }),
  onAudioStatus: noop,
  onAudioMeters: noop,
  onCanonicalStateChanged: noop,
  onProjectStateChanged: noop,
  onProjectActivated: noop,
  onTransportStatus: noop,
  onRuntimeProjectionStatus: noop,
  onRuntimeRestarted: noop,
  onRecordingFinalized: noop,
  onScoreSyncStatusChanged: noop,
  onRigStateChanged: noop,
};

export function createBrowserPreviewApi(): NativeApi {
  const rejected = Object.fromEntries(
    nativeApiMethodNames.map((method) => [
      method,
      async () => {
        throw new NativeCommandError({
          code: 'browserPreview',
          message: `${method} requires the Desktop runtime`,
        });
      },
    ]),
  );
  return { ...rejected, ...reads } as NativeApi;
}
