import type { NativeApi, NativeEventApi } from '../native-api';
import { canonicalState, defaultProjectState, defaultSession } from '../browser-defaults';
import { NativeCommandError } from '../invoke';
import { createNativeApi } from './native-implementation';

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
  | 'getHistoryState'
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
  getHistoryState: async () => ({ canUndo: false, canRedo: false }),
  onAudioStatus: noop,
  onAudioMeters: noop,
  onCanonicalStateChanged: noop,
  onProjectStateChanged: noop,
  onProjectActivated: noop,
  onTransportStatus: noop,
  onRuntimeProjectionStatus: noop,
  onRuntimeRestarted: noop,
  onRecordingFinalized: noop,
};

export function createBrowserPreviewApi(): NativeApi {
  const rejected = Object.fromEntries(
    Object.keys(createNativeApi()).map((method) => [
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
