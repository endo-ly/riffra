import type { NativeApi } from '../native-api';
import * as analysisApi from './analysis';
import * as arrangeApi from './arrange';
import * as audioApi from './audio';
import * as bootstrapApi from './bootstrap';
import * as devicesApi from './devices';
import { eventApi } from './events';
import * as jobsApi from './jobs';
import { hostConnectionApi } from './host-connection';
import * as libraryApi from './library';
import * as missingApi from './missing';
import * as projectApi from './project';
import * as recordingApi from './recording';
import * as renderApi from './render';
import * as transportApi from './transport';
import * as updaterApi from './updater';

export function createNativeApi(): NativeApi {
  return {
    ...bootstrapApi,
    ...projectApi,
    ...jobsApi,
    ...hostConnectionApi,
    ...libraryApi,
    ...analysisApi,
    ...renderApi,
    ...audioApi,
    ...recordingApi,
    ...arrangeApi,
    ...devicesApi,
    ...transportApi,
    ...missingApi,
    ...updaterApi,
    ...eventApi,
  };
}

export const defaultNativeApi: NativeApi = createNativeApi();
