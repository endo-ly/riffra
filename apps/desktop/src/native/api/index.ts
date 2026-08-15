import type { NativeApi } from '../native-api';
import * as analysisApi from './analysis';
import * as arrangeApi from './arrange';
import * as audioApi from './audio';
import * as bootstrapApi from './bootstrap';
import { eventApi } from './events';
import * as jobsApi from './jobs';
import * as libraryApi from './library';
import * as missingApi from './missing';
import * as projectApi from './project';
import * as rackApi from './rack';
import * as recordingApi from './recording';
import * as renderApi from './render';
import * as transportApi from './transport';

export function createNativeApi(): NativeApi {
  return {
    ...bootstrapApi,
    ...projectApi,
    ...jobsApi,
    ...libraryApi,
    ...analysisApi,
    ...renderApi,
    ...audioApi,
    ...recordingApi,
    ...arrangeApi,
    ...rackApi,
    ...transportApi,
    ...missingApi,
    ...eventApi,
  };
}

export const defaultNativeApi: NativeApi = createNativeApi();
